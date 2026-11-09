//! ccez-keys for the Mac: while you type English anywhere, a small panel
//! under the cursor shows the sentence in the languages you're learning
//! (docs/mac.md). A menu bar app, on at login.
//!
//! Main thread: AppKit, a poll of the focused field (Accessibility) every
//! 150 ms, the panel. A worker thread translates on the Mac (Apple's
//! on-device translator, through a small Swift helper).

#[cfg(target_os = "macos")]
mod ax;
#[cfg(target_os = "macos")]
mod prefs;
#[cfg(target_os = "macos")]
mod view;
mod worker;

#[cfg(target_os = "macos")]
mod app {
    use std::cell::{Cell, RefCell};
    use std::io::Write;
    use std::ptr::NonNull;
    use std::time::{Duration, Instant};

    use block2::RcBlock;
    use objc2::MainThreadMarker;
    use objc2::rc::autoreleasepool;
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSWorkspace};
    use objc2_foundation::NSTimer;
    use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
    use tray_icon::TrayIconBuilder;

    use ccez_keys_core::text::{looks_secret, sentence_of};

    use crate::view::View;
    use crate::worker::{Ask, Worker};
    use crate::{ax, worker};

    /// How long the preview stays after typing stops (⌥ brings it back).
    const LINGER: Duration = Duration::from_secs(4);
    /** The pause in typing that asks for a translation. */
    const PAUSE: Duration = Duration::from_millis(200);

    /// Apps where nothing shows (docs/mac.md, decision 4): terminals, code
    /// editors, password managers, Claude.
    const QUIET: &[&str] = &[
        "com.apple.Terminal", "com.googlecode.iterm2", "com.mitchellh.ghostty", "net.kovidgoyal.kitty", "dev.warp.Warp-Stable",
        "io.alacritty", "com.microsoft.VSCode", "com.todesktop.230313mzl4w4u92", "dev.zed.Zed", "com.apple.dt.Xcode",
        "com.sublimetext.4", "com.1password.1password", "com.bitwarden.desktop",
        "com.apple.keychainaccess", "com.apple.Passwords",
    ];

    struct App {
        view: View,
        worker: Worker,
        langs: Vec<String>,
        flags: std::collections::HashMap<String, String>,
        paused: Cell<bool>,
        prefs: RefCell<crate::prefs::Prefs>,
        /** The app being typed in (bundle id, name): the menu's "Off in …" is about it. */
        typing_in: RefCell<Option<(String, String)>>,
        sentence: RefCell<String>,
        caret: Cell<Option<ax::Rect>>,
        changed: Cell<Instant>,
        seq: Cell<u64>,
        asked: Cell<bool>,
        woken: RefCell<std::collections::HashSet<i32>>,
        line: RefCell<Vec<crate::view::Row>>,
        /** When the preview last changed on screen: it stays LINGER from then. */
        appeared: Cell<Instant>,
    }

    pub fn log(level: &str, what: &str) {
        let dir = dirs();
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("mac.log")) {
            let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            let _ = writeln!(f, "{t} {level} {what}");
        }
    }

    fn dirs() -> std::path::PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        let d = std::path::PathBuf::from(home).join("Library/Application Support/ccez-keys");
        let _ = std::fs::create_dir_all(&d);
        d
    }

    fn flags() -> std::collections::HashMap<String, String> {
        let v: serde_json::Value = serde_json::from_str(include_str!("../../data/languages.json")).unwrap_or_default();
        v["languages"].as_array().into_iter().flatten().filter_map(|l| Some((l[0].as_str()?.to_string(), l[3].as_str()?.to_string()))).collect()
    }

    fn frontmost() -> Option<String> {
        NSWorkspace::sharedWorkspace().frontmostApplication()?.bundleIdentifier().map(|s| s.to_string())
    }

    fn option_held() -> bool {
        let flags: usize = unsafe { objc2::msg_send![objc2::class!(NSEvent), modifierFlags] };
        flags & (1 << 19) != 0
    }

    /// Off in this app: quiet by default (and not turned back on), or turned off from the menu.
    fn quiet(app: &App, bundle: &str) -> bool {
        let p = app.prefs.borrow();
        p.off.iter().any(|b| b == bundle) || (QUIET.contains(&bundle) && !p.on.iter().any(|b| b == bundle))
    }

    fn tick(mtm: MainThreadMarker, app: &App) {
        if let Some(a) = NSWorkspace::sharedWorkspace().frontmostApplication() {
            let bundle = a.bundleIdentifier().map(|s| s.to_string()).unwrap_or_default();
            if bundle != "uk.ccez.keys.mac" && !bundle.is_empty() {
                let name = a.localizedName().map(|s| s.to_string()).unwrap_or_else(|| bundle.clone());
                *app.typing_in.borrow_mut() = Some((bundle, name));
            }
        }
        if app.paused.get() || frontmost().is_some_and(|b| quiet(app, &b)) {
            app.view.hide();
            return;
        }
        // Electron and Chrome apps: asked once to share their fields.
        if let Some(pid) = NSWorkspace::sharedWorkspace().frontmostApplication().map(|a| a.processIdentifier()) {
            if app.woken.borrow_mut().insert(pid) {
                ax::wake(pid);
            }
        }
        let field = ax::focused();
        let Some(field) = field.filter(|f| !f.secure) else {
            app.view.hide();
            app.sentence.borrow_mut().clear();
            app.line.borrow_mut().clear();
            return;
        };
        let sentence = sentence_of(field.before.clone());
        // Nothing secret is ever shown or translated: the sentence, or what's near it.
        let tail: String = field.before.chars().rev().take(300).collect::<Vec<_>>().into_iter().rev().collect();
        if sentence.trim().is_empty() || looks_secret(tail) {
            app.view.hide();
            app.sentence.borrow_mut().clear();
            // A new or emptied field starts blank, not with the last translation.
            app.line.borrow_mut().clear();
            return;
        }
        if *app.sentence.borrow() != sentence {
            // Another sentence (not this one typed on): the old translation goes at once.
            let old = app.sentence.borrow().clone();
            let n = old.chars().count().min(sentence.chars().count()).min(12);
            if n == 0 || old.chars().take(n).ne(sentence.chars().take(n)) {
                app.line.borrow_mut().clear();
            }
            *app.sentence.borrow_mut() = sentence.clone();
            app.caret.set(field.caret);
            app.changed.set(Instant::now());
            app.asked.set(false);
        } else if field.caret.is_some() {
            app.caret.set(field.caret);
        }
        // Translated once typing pauses a moment, not for every letter.
        if !app.asked.get() && app.changed.get().elapsed() >= PAUSE {
            app.asked.set(true);
            app.seq.set(app.seq.get() + 1);
            let shown = app.prefs.borrow().shown;
            let langs: Vec<String> = if shown == 0 { app.langs.clone() } else { app.langs.iter().take(shown + 2).cloned().collect() };
            app.worker.ask(Ask { seq: app.seq.get(), text: sentence, from: "en".into(), langs, max: if shown == 0 { usize::MAX } else { shown } });
        }
        if let Some(shown) = app.worker.out.lock().unwrap().take() {
            if shown.seq == app.seq.get() {
                let line = render(app, &shown);
                *app.line.borrow_mut() = line;
                app.appeared.set(Instant::now());
            } else {
                // An older sentence's: put back for nothing, wait for the newest.
            }
        }
        // From when it showed, not from the last key: translating takes a moment.
        let fresh = app.changed.get().elapsed() < LINGER || app.appeared.get().elapsed() < LINGER || option_held();
        let line = app.line.borrow();
        if fresh && !line.is_empty() {
            app.view.show(mtm, &line, app.caret.get());
        } else {
            app.view.hide();
        }
    }

    /// One line per language, in your order: "🇫🇷 je te retrouve …"; Japanese
    /// and Chinese with their readings over the words.
    fn render(app: &App, shown: &worker::Shown) -> Vec<crate::view::Row> {
        let p = app.prefs.borrow();
        shown
            .lines
            .iter()
            .take(if p.shown == 0 { usize::MAX } else { p.shown })
            .map(|l| crate::view::Row {
                flag: app.flags.get(&l.lang).cloned().unwrap_or_else(|| l.lang.clone()),
                text: l.text.trim().to_string(),
                ruby: if p.readings { l.ruby.clone() } else { vec![] },
            })
            .collect()
    }    /// `--demo "line"`: the preview with that line at the screen's foot for a few
    /// seconds, for screenshots (no permission, no translation needed).
    fn demo(mtm: MainThreadMarker, line: String) {
        let nsapp = NSApplication::sharedApplication(mtm);
        nsapp.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        let view: &'static View = Box::leak(Box::new(View::new(mtm)));
        let lines: Vec<crate::view::Row> = line.split('|').map(|t| crate::view::Row { flag: String::new(), text: t.to_string(), ruby: vec![] }).collect();
        view.show(mtm, &lines, Some(ax::Rect { x: 300.0, y: 300.0, w: 2.0, h: 18.0 }));
        let quit = RcBlock::new(move |_t: NonNull<NSTimer>| std::process::exit(0));
        unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(6.0, false, &quit) };
        nsapp.run();
    }

    /// `--type "text"`: types it into the frontmost app, a key at a time, and quits
    /// (for checks: the app has the Accessibility permission that needs).
    fn type_text(text: &str) {
        use std::ffi::c_void;
        #[link(name = "CoreGraphics", kind = "framework")]
        unsafe extern "C" {
            fn CGEventCreateKeyboardEvent(source: *const c_void, key: u16, down: bool) -> *mut c_void;
            fn CGEventKeyboardSetUnicodeString(event: *mut c_void, len: usize, chars: *const u16);
            fn CGEventPost(tap: u32, event: *mut c_void);
            fn CFRelease(r: *const c_void);
        }
        // Only into TextEdit: a key never lands in whatever else is in front.
        for ch in text.chars() {
            if frontmost().as_deref() != Some("com.apple.TextEdit") {
                return;
            }
            let units: Vec<u16> = ch.to_string().encode_utf16().collect();
            for down in [true, false] {
                unsafe {
                    let e = CGEventCreateKeyboardEvent(std::ptr::null(), 0, down);
                    CGEventKeyboardSetUnicodeString(e, units.len(), units.as_ptr());
                    CGEventPost(0, e);
                    CFRelease(e);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(45));
        }
    }

    pub fn run() {
        let mtm = MainThreadMarker::new().expect("main thread");
        let args: Vec<String> = std::env::args().collect();
        if let Some(i) = args.iter().position(|a| a == "--type") {
            return type_text(args.get(i + 1).map(String::as_str).unwrap_or(""));
        }
        if let Some(i) = args.iter().position(|a| a == "--demo") {
            return demo(mtm, args.get(i + 1).cloned().unwrap_or_default());
        }
        let nsapp = NSApplication::sharedApplication(mtm);
        nsapp.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        let trusted = ax::trusted(true);
        log("info", &format!("start; accessibility {}", if trusted { "granted" } else { "asked" }));

        let engine = ccez_keys_core::Engine::new(dirs().to_string_lossy().into_owned());
        let langs = engine.setting_strings("languages.learning".into());
        let app: &'static App = Box::leak(Box::new(App {
            view: View::new(mtm),
            worker: Worker::start(|l, w| log(l, w)),
            langs,
            flags: flags(),
            paused: Cell::new(false),
            prefs: RefCell::new(crate::prefs::Prefs::load(&dirs())),
            typing_in: RefCell::default(),
            sentence: RefCell::default(),
            caret: Cell::new(None),
            changed: Cell::new(Instant::now()),
            seq: Cell::new(0),
            asked: Cell::new(true),
            woken: RefCell::default(),
            line: RefCell::default(),
            appeared: Cell::new(Instant::now()),
        }));

        // The menu: pause, how many languages, readings, off in this app, the
        // Mac's translation languages, open at login, quit.
        let menu = Menu::new();
        let pause = MenuItem::new("Pause", true, None);
        let shown_menu = Submenu::new("Languages shown", true);
        let counts: Vec<(usize, CheckMenuItem)> = [2usize, 4, 6, 8, 0]
            .iter()
            .map(|&n| (n, CheckMenuItem::new(if n == 0 { "All".to_string() } else { n.to_string() }, true, app.prefs.borrow().shown == n, None)))
            .collect();
        for (_, c) in &counts {
            let _ = shown_menu.append(c);
        }
        let readings = CheckMenuItem::new("Readings over Japanese and Chinese", true, app.prefs.borrow().readings, None);
        let off_here = CheckMenuItem::new("Off in this app", true, false, None);
        let languages = MenuItem::new("Translation languages…", true, None);
        let login = CheckMenuItem::new("Open at login", true, login_on(), None);
        let quit = MenuItem::new("Quit", true, None);
        let sep = PredefinedMenuItem::separator();
        let sep2 = PredefinedMenuItem::separator();
        let sep3 = PredefinedMenuItem::separator();
        let _ = menu.append_items(&[&pause, &sep, &shown_menu, &readings, &off_here, &sep2, &languages, &login, &sep3, &quit]);
        let tray = TrayIconBuilder::new().with_menu(Box::new(menu)).with_title("文").with_tooltip("ccez-keys").build();
        // Bold and a size up: the menu bar's regular text left 文 thin and small.
        if let Ok(t) = &tray
            && let Some(item) = t.ns_status_item()
            && let Some(button) = item.button(mtm)
        {
            // A touch lower than the font sits by itself (owner, 2026-11-09: it rode high).
            use objc2_foundation::{NSAttributedString, NSDictionary, NSNumber};
            let font = objc2_app_kit::NSFont::boldSystemFontOfSize(15.0);
            let attrs: objc2::rc::Retained<NSDictionary<objc2_foundation::NSString, objc2::runtime::AnyObject>> = unsafe {
                NSDictionary::from_slices(
                    &[objc2_app_kit::NSFontAttributeName, objc2_app_kit::NSBaselineOffsetAttributeName],
                    &[&*font as &objc2::runtime::AnyObject, &*NSNumber::new_f64(-1.5) as &objc2::runtime::AnyObject],
                )
            };
            let title = unsafe { NSAttributedString::new_with_attributes(&objc2_foundation::NSString::from_str("文"), &attrs) };
            button.setAttributedTitle(&title);
        }
        let _tray = tray;
        let (pause_id, lang_id, login_id, quit_id) = (pause.id().clone(), languages.id().clone(), login.id().clone(), quit.id().clone());
        let (readings_id, off_id) = (readings.id().clone(), off_here.id().clone());
        let menu_tick = Cell::new(0u32);

        let tick_block = RcBlock::new(move |_t: NonNull<NSTimer>| {
            autoreleasepool(|_| {
                while let Ok(e) = MenuEvent::receiver().try_recv() {
                    if e.id == pause_id {
                        app.paused.set(!app.paused.get());
                        pause.set_text(if app.paused.get() { "Resume" } else { "Pause" });
                    } else if e.id == lang_id {
                        let _ = std::process::Command::new("open").arg("x-apple.systempreferences:com.apple.Localization-Settings.extension").spawn();
                    } else if e.id == login_id {
                        set_login(login.is_checked());
                    } else if e.id == quit_id {
                        std::process::exit(0);
                    } else if e.id == readings_id {
                        app.prefs.borrow_mut().readings = readings.is_checked();
                        app.prefs.borrow().save(&dirs());
                        app.sentence.borrow_mut().clear();
                    } else if e.id == off_id {
                        if let Some((bundle, _)) = app.typing_in.borrow().clone() {
                            let mut p = app.prefs.borrow_mut();
                            let now_off = off_here.is_checked();
                            p.off.retain(|b| *b != bundle);
                            p.on.retain(|b| *b != bundle);
                            if now_off && !QUIET.contains(&bundle.as_str()) {
                                p.off.push(bundle);
                            } else if !now_off && QUIET.contains(&bundle.as_str()) {
                                p.on.push(bundle);
                            }
                            p.save(&dirs());
                        }
                    } else if let Some((n, _)) = counts.iter().find(|(_, c)| *c.id() == e.id) {
                        app.prefs.borrow_mut().shown = *n;
                        app.prefs.borrow().save(&dirs());
                        for (m, c) in &counts {
                            c.set_checked(m == n);
                        }
                        app.sentence.borrow_mut().clear();
                    }
                }
                // "Off in Claude": the menu names the app being typed in (every second or so).
                menu_tick.set(menu_tick.get() + 1);
                if menu_tick.get() % 7 == 0 {
                    if let Some((bundle, name)) = app.typing_in.borrow().clone() {
                        off_here.set_text(format!("Off in {name}"));
                        off_here.set_checked(quiet(app, &bundle));
                    }
                }
                tick(mtm, app);
            })
        });
        unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(0.15, true, &tick_block) };
        nsapp.run();
    }

    fn launcher() -> Option<auto_launch::AutoLaunch> {
        let exe = std::env::current_exe().ok()?;
        let bundle = exe.ancestors().find(|p| p.extension().is_some_and(|e| e == "app"))?.to_string_lossy().into_owned();
        auto_launch::AutoLaunchBuilder::new()
            .set_app_name("ccez-keys")
            .set_app_path(&bundle)
            .set_macos_launch_mode(auto_launch::MacOSLaunchMode::SMAppService)
            .build()
            .ok()
    }

    fn login_on() -> bool {
        launcher().and_then(|l| l.is_enabled().ok()).unwrap_or(false)
    }

    fn set_login(on: bool) {
        if let Some(l) = launcher() {
            let r = if on { l.enable() } else { l.disable() };
            if let Err(e) = r {
                log("warn", &format!("login: {e}"));
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn main() {
    app::run();
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("ccez-keys-mac runs on macOS");
}
