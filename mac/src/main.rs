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
    use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::TrayIconBuilder;

    use ccez_keys_core::text::{looks_secret, sentence_of};

    use crate::view::View;
    use crate::worker::{Ask, Worker};
    use crate::{ax, worker};

    /// How long the preview stays after typing stops (⌥ brings it back).
    const LINGER: Duration = Duration::from_secs(3);
    /** The pause in typing that asks for a translation. */
    const PAUSE: Duration = Duration::from_millis(200);

    /// Apps where nothing shows (docs/mac.md, decision 4): terminals, code
    /// editors, password managers, Claude.
    const QUIET: &[&str] = &[
        "com.apple.Terminal", "com.googlecode.iterm2", "com.mitchellh.ghostty", "net.kovidgoyal.kitty", "dev.warp.Warp-Stable",
        "io.alacritty", "com.microsoft.VSCode", "com.todesktop.230313mzl4w4u92", "dev.zed.Zed", "com.apple.dt.Xcode",
        "com.sublimetext.4", "com.anthropic.claudefordesktop", "com.1password.1password", "com.bitwarden.desktop",
        "com.apple.keychainaccess", "com.apple.Passwords",
    ];

    struct App {
        view: View,
        worker: Worker,
        langs: Vec<String>,
        flags: std::collections::HashMap<String, String>,
        paused: Cell<bool>,
        sentence: RefCell<String>,
        caret: Cell<Option<ax::Rect>>,
        changed: Cell<Instant>,
        seq: Cell<u64>,
        asked: Cell<bool>,
        line: RefCell<Vec<String>>,
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

    fn tick(mtm: MainThreadMarker, app: &App) {
        if app.paused.get() || frontmost().is_some_and(|b| QUIET.iter().any(|q| *q == b)) {
            app.view.hide();
            return;
        }
        let field = ax::focused();
        let Some(field) = field.filter(|f| !f.secure) else {
            app.view.hide();
            app.sentence.borrow_mut().clear();
            return;
        };
        let sentence = sentence_of(field.before.clone());
        // Nothing secret is ever shown or translated: the sentence, or what's near it.
        let tail: String = field.before.chars().rev().take(300).collect::<Vec<_>>().into_iter().rev().collect();
        if sentence.trim().is_empty() || looks_secret(tail) {
            app.view.hide();
            app.sentence.borrow_mut().clear();
            return;
        }
        if *app.sentence.borrow() != sentence {
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
            app.worker.ask(Ask { seq: app.seq.get(), text: sentence, from: "en".into(), langs: app.langs.clone() });
        }
        if let Some(shown) = app.worker.out.lock().unwrap().take() {
            if shown.seq == app.seq.get() {
                let line = render(app, &shown);
                *app.line.borrow_mut() = line;
            } else {
                // An older sentence's: put back for nothing, wait for the newest.
            }
        }
        let fresh = app.changed.get().elapsed() < LINGER || option_held();
        let line = app.line.borrow();
        if fresh && !line.is_empty() {
            app.view.show(mtm, &line, app.caret.get());
        } else {
            app.view.hide();
        }
    }

    /// One line per language, in your order: "🇫🇷 je te retrouve …".
    fn render(app: &App, shown: &worker::Shown) -> Vec<String> {
        shown
            .lines
            .iter()
            .take(crate::view::LINES)
            .map(|(lang, text)| format!("{} {}", app.flags.get(lang).map(String::as_str).unwrap_or(lang), text.trim()))
            .collect()
    }
    /// `--demo "line"`: the preview with that line at the screen's foot for a few
    /// seconds, for screenshots (no permission, no translation needed).
    fn demo(mtm: MainThreadMarker, line: String) {
        let nsapp = NSApplication::sharedApplication(mtm);
        nsapp.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        let view: &'static View = Box::leak(Box::new(View::new(mtm)));
        let lines: Vec<String> = line.split('|').map(str::to_string).collect();
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
            sentence: RefCell::default(),
            caret: Cell::new(None),
            changed: Cell::new(Instant::now()),
            seq: Cell::new(0),
            asked: Cell::new(true),
            line: RefCell::default(),
        }));

        // The menu bar: on/off, the Mac's translation languages, open at login, quit.
        let menu = Menu::new();
        let pause = CheckMenuItem::new("Paused", true, false, None);
        let languages = MenuItem::new("Translation languages…", true, None);
        let login = CheckMenuItem::new("Open at login", true, login_on(), None);
        let quit = MenuItem::new("Quit", true, None);
        let _ = menu.append_items(&[&pause, &languages, &login, &PredefinedMenuItem::separator(), &quit]);
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

        let tick_block = RcBlock::new(move |_t: NonNull<NSTimer>| {
            autoreleasepool(|_| {
                while let Ok(e) = MenuEvent::receiver().try_recv() {
                    if e.id == pause_id {
                        app.paused.set(pause.is_checked());
                    } else if e.id == lang_id {
                        let _ = std::process::Command::new("open").arg("x-apple.systempreferences:com.apple.Localization-Settings.extension").spawn();
                    } else if e.id == login_id {
                        set_login(login.is_checked());
                    } else if e.id == quit_id {
                        std::process::exit(0);
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
