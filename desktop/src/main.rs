// Windows: no console window behind the app.
#![cfg_attr(windows, windows_subsystem = "windows")]

//! ccez-keys on the desktop: while you type English anywhere, a small panel
//! by the cursor shows the sentence in the languages you're learning
//! (docs/mac.md). On the Mac a menu bar app (this file's `app`); on Linux and
//! Windows the same preview drawn with egui (pc/). On at login.
//!
//! Main thread: AppKit, a poll of the focused field (Accessibility) every
//! 150 ms, the panel. A worker thread translates on the Mac (Apple's
//! on-device translator, through a small Swift helper).

#[cfg(target_os = "macos")]
mod apple;
#[cfg(target_os = "macos")]
mod ax;
mod cards;
#[cfg(target_os = "macos")]
mod chatswap;
#[cfg(not(target_os = "macos"))]
mod firefox;
#[cfg(target_os = "macos")]
mod float;
#[allow(dead_code)]
mod idle;
#[cfg(target_os = "macos")]
mod keys;
mod learn;
#[cfg(target_os = "macos")]
mod meaning;
mod model;
mod paths;
#[cfg(not(target_os = "macos"))]
mod pc;
mod prefs;
mod preview;
#[cfg(target_os = "macos")]
mod speak;
#[cfg(target_os = "macos")]
mod update;
#[cfg(target_os = "macos")]
mod view;
mod worker;

#[cfg(target_os = "macos")]
mod app {
    use std::cell::{Cell, RefCell};
    use std::io::Write;
    use std::ptr::NonNull;
    use std::time::Instant;

    use block2::RcBlock;
    use objc2::MainThreadMarker;
    use objc2::rc::autoreleasepool;
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSWorkspace};
    use objc2_foundation::NSTimer;
    use tray_icon::TrayIconBuilder;
    use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};

    use ccez_keys_core::text::{looks_secret, sentence_of};

    use crate::view::View;
    use crate::worker::Worker;
    use crate::{ax, worker};

    use crate::preview::PENDING;

    /// Apps where nothing shows (docs/mac.md, decision 4): terminals, code
    /// editors, password managers, Claude.
    const QUIET: &[&str] = &[
        "com.apple.Terminal",
        "com.googlecode.iterm2",
        "com.mitchellh.ghostty",
        "net.kovidgoyal.kitty",
        "dev.warp.Warp-Stable",
        "io.alacritty",
        "com.microsoft.VSCode",
        "com.todesktop.230313mzl4w4u92",
        "dev.zed.Zed",
        "com.apple.dt.Xcode",
        "com.sublimetext.4",
        "com.1password.1password",
        "com.bitwarden.desktop",
        "com.apple.keychainaccess",
        "com.apple.Passwords",
    ];

    struct App {
        view: View,
        worker: Worker,
        langs: Vec<String>,
        flags: std::collections::HashMap<String, String>,
        paused: Cell<bool>,
        engine: std::sync::Arc<ccez_keys_core::Engine>,
        /** The word whose card is open: language, dictionary form, as shown. */
        card: RefCell<Option<(String, String, String)>>,
        /** What the open card shows: its word, the dictionary's meaning, its status, and
         * the key its meaning-here comes back under. */
        card_view: RefCell<Option<(String, String, String, u8, String)>>,
        meanings: std::sync::Arc<crate::meaning::Meanings>,
        prefs: RefCell<crate::prefs::Prefs>,
        /** The app being typed in (bundle id, name): the menu's "Off in …" is about it. */
        typing_in: RefCell<Option<(String, String)>>,
        woken: RefCell<std::collections::HashSet<i32>>,
        /** When it shows and what it says (the same on every system). */
        preview: RefCell<crate::preview::Preview>,
        /** Idle words: where the list carries on from, and when to look again
         * when there was nowhere (or nothing) to show them. */
        /** Learning words on screen (float.rs). */
        floaters: crate::float::Floaters,
        /** A ⌥⏎ swap's note for the toast, from its thread. */
        swapped: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    }

    pub fn log(level: &str, what: &str) {
        let dir = dirs();
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("mac.log")) {
            let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            let _ = writeln!(f, "{t} {level} {what}");
        }
    }

    fn dirs() -> std::path::PathBuf {
        crate::paths::data()
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
        let off = app.paused.get() || frontmost().is_some_and(|b| quiet(app, &b));
        // Electron and Chrome apps: asked once to share their fields.
        if !off
            && let Some(pid) = NSWorkspace::sharedWorkspace().frontmostApplication().map(|a| a.processIdentifier())
            && app.woken.borrow_mut().insert(pid)
        {
            ax::wake(pid);
        }
        let mut field = if off { None } else { ax::focused() };
        if !app.prefs.borrow().selection
            && let Some(f) = field.as_mut()
        {
            f.selected.clear();
        }
        let answer = app.worker.out.lock().unwrap().take();
        let langs = app.langs.clone();
        let shown = app.prefs.borrow().shown;
        let inputs = crate::preview::Inputs { now: Instant::now(), off, field, held: option_held(), under_mouse: app.view.under_mouse(mtm), shown, langs: &langs };
        let out = app.preview.borrow_mut().tick(inputs, answer, &|s| render(app, s));
        if out.close_card {
            app.view.close_card();
        }
        if let Some(ask) = out.ask {
            app.worker.ask(ask);
        }
        // A card's meaning-here, arriving after it opened.
        if let Some((k, here)) = app.meanings.out.lock().unwrap().take()
            && app.view.card_open.get()
            && let Some((head, sub, dict, st, key)) = app.card_view.borrow().clone()
            && key == k
        {
            app.view.open_card(mtm, &head, &sub, &crate::cards::card_text(Some(&here), &dict), st);
        }
        if let Some(note) = app.swapped.lock().unwrap().take() {
            app.view.toast(mtm, &note);
        }
        app.view.tick_toast();
        let most = if app.paused.get() { 0 } else { app.prefs.borrow().words_on_screen };
        let (while_video, while_typing) = {
            let p = app.prefs.borrow();
            app.floaters.hide_in_shots.set(p.words_hide_shots);
            (p.words_video, p.words_typing)
        };
        app.floaters.tick(
            mtm,
            most,
            while_video,
            while_typing,
            || {
                // From the first few learning languages only, when the menu says so.
                let n = app.prefs.borrow().words_langs;
                let keep: Vec<&String> = if n == 0 { app.langs.iter().collect() } else { app.langs.iter().take(n).collect() };
                app.engine.learning_words().into_iter().filter(|w| keep.iter().any(|l| **l == w.lang)).map(|w| (w.lang, w.word)).collect()
            },
            |lang: &str, word: &str| {
                let meanings = |w: &str| app.engine.word_card(lang.into(), w.into(), String::new()).map(|c| c.meanings).unwrap_or_default();
                ccez_keys_core::gloss::short_gloss(word, &meanings(word), meanings)
            },
        );
        let p = app.preview.borrow();
        if out.show {
            app.view.show(mtm, &p.line, p.place);
        } else {
            app.view.hide();
        }
    }

    thread_local! {
        /// Per app, where its content keeps turning up (the spot thread's own).
        static BUSY: std::cell::RefCell<crate::idle::Busy> = std::cell::RefCell::new(crate::idle::Busy::default());
    }

    /// Where idle words can go: room in the front window its content leaves,
    /// and screen no other app's window covers.
    fn spots(screen: crate::model::Rect) -> Vec<crate::model::Rect> {
        let pid = NSWorkspace::sharedWorkspace().frontmostApplication().map(|a| a.processIdentifier()).unwrap_or(0);
        let (front, content) = ax::content_frames(pid, 4000, std::time::Duration::from_millis(250));
        let free = crate::idle::free_spots(screen, front, &content, &crate::idle::windows(pid), 8, 6);
        // Spots this app keeps filling lately: skipped too.
        let all = crate::idle::free_cells(screen, &[], 8, 6);
        let app = NSWorkspace::sharedWorkspace().frontmostApplication().and_then(|a| a.bundleIdentifier()).map(|b| b.to_string()).unwrap_or_default();
        BUSY.with_borrow_mut(|b| b.look(&app, &all, &free))
    }

    /// Puts `text` on the clipboard, as plain text.
    fn copy(text: &str) {
        put(&objc2_app_kit::NSPasteboard::generalPasteboard(), text);
    }

    fn put(pb: &objc2_app_kit::NSPasteboard, text: &str) {
        pb.clearContents();
        pb.setString_forType(&objc2_foundation::NSString::from_str(text), unsafe { objc2_app_kit::NSPasteboardTypeString });
    }

    #[cfg(test)]
    #[test]
    fn copies_any_script() {
        // A pasteboard of its own: the owner's clipboard is left alone.
        let pb = objc2_app_kit::NSPasteboard::pasteboardWithUniqueName();
        for text in ["Je vais à la gare", "間に合わせられますか？", "我们要去车站"] {
            put(&pb, text);
            let got = pb.stringForType(unsafe { objc2_app_kit::NSPasteboardTypeString }).map(|s| s.to_string());
            assert_eq!(got.as_deref(), Some(text));
        }
    }

    /// Right-click on a line: that translation in place of the sentence typed
    /// (spaces after it kept). Through Accessibility where the app takes it,
    /// else typed over the selected sentence, else deleted and typed, only
    /// while the same field still holds the same text; ⌘Z undoes it.
    fn swap(app: &App, row: usize) {
        use unicode_segmentation::UnicodeSegmentation;
        let rows = app.preview.borrow().line.clone();
        let Some(r) = rows.get(row).filter(|r| !r.toks.iter().any(|t| t.key == PENDING)) else {
            return;
        };
        let text: String = r.toks.iter().map(|t| t.text.as_str()).collect::<String>().trim().to_string();
        let Some(f) = ax::focused().filter(|f| !f.secure) else {
            return;
        };
        let sentence = sentence_of(f.before.clone());
        // Only a whole translation of the sentence as it is now (not one kept
        // from fewer words while typing on), never anything secret-looking.
        let fits = {
            let p = app.preview.borrow();
            p.sentence == sentence && p.line_for == sentence && p.complete
        };
        if sentence.is_empty() || !fits {
            return;
        }
        if looks_secret(f.before.clone()) || looks_secret(sentence.clone()) {
            return;
        }
        let trimmed = f.before.trim_end();
        if !trimmed.ends_with(sentence.as_str()) {
            return;
        }
        let front = || NSWorkspace::sharedWorkspace().frontmostApplication().map(|a| a.processIdentifier());
        let app_then = front();
        let trailing = f.before[trimmed.len()..].to_string();
        let end = trimmed.encode_utf16().count();
        let len = sentence.encode_utf16().count();
        let how = match ax::replace_range(end - len, len, &text) {
            ax::Put::Done => "accessibility",
            put => {
                // Typing goes to whatever has the keyboard: only if that's still this field, as it was.
                let now = ax::focused();
                let same = front() == app_then && now.as_ref().is_some_and(|n| !n.secure);
                if !same {
                    log("info", "swap skipped: the field changed");
                    return;
                }
                if put == ax::Put::Selected && ax::selection() == Some((end - len, len)) {
                    crate::keys::type_text(&text);
                    "typed over"
                } else if now.as_ref().is_some_and(|n| n.before == f.before) && ax::selection() == Some((f.before.encode_utf16().count(), 0)) {
                    // Delete takes a whole visible character (an emoji of several chars too).
                    let n = sentence.graphemes(true).count() + trailing.graphemes(true).count();
                    crate::keys::delete(n);
                    crate::keys::type_text(&format!("{text}{trailing}"));
                    "retyped"
                } else {
                    log("info", "swap skipped: the field moved on");
                    return;
                }
            }
        };
        log("info", &format!("swapped a sentence for {} ({how})", r.lang));
        app.preview.borrow_mut().swapped_in(text);
        app.view.hide();
    }

    fn render(app: &App, shown: &worker::Shown) -> Vec<crate::view::Row> {
        crate::cards::rows(shown, &app.prefs.borrow(), &app.flags)
    }

    /// `--demo "line"`: the preview with that line at the screen's foot for a few
    /// seconds, for screenshots (no permission, no translation needed).
    fn demo(mtm: MainThreadMarker, line: String) {
        let nsapp = NSApplication::sharedApplication(mtm);
        nsapp.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        let view: &'static View = Box::leak(Box::new(View::new(mtm)));
        let lines: Vec<crate::view::Row> = line
            .split('|')
            .map(|t| crate::view::Row {
                lang: "fr".into(),
                flag: String::new(),
                toks: vec![crate::view::Tok { text: t.to_string(), reading: String::new(), word: true, key: t.to_string(), status: 0 }],
            })
            .collect();
        view.show(mtm, &lines, ax::Place { caret: Some(ax::Rect { x: 300.0, y: 300.0, w: 2.0, h: 18.0 }), frame: None });
        // Fades in, stays a moment, fades out, quits.
        let out = RcBlock::new(move |_t: NonNull<NSTimer>| view.hide());
        unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(1.5, false, &out) };
        let quit = RcBlock::new(move |_t: NonNull<NSTimer>| std::process::exit(0));
        unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(2.3, false, &quit) };
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
        // `--click x y` (screen points, top-left): a click there, for checks.
        // `--probe`: what the focused field reports (frame, cursor, length), to
        // the log; reads only, never its text.
        if args.iter().any(|a| a == "--probe") {
            let f = ax::focused();
            let line = match f {
                Some(f) => format!("probe front={:?} secure={} chars={} caret={:?} frame={:?}", frontmost(), f.secure, f.before.chars().count(), f.caret, f.frame),
                None => format!("probe front={:?} no text field: {}", frontmost(), ax::describe()),
            };
            if let Ok(mut out) = std::fs::OpenOptions::new().create(true).append(true).open(dirs().join("mac.log")) {
                use std::io::Write;
                let _ = writeln!(out, "{line}");
            }
            return;
        }
        // `--update-now`: one look for a newer release, replacing this app if
        // there is one (not started again), for checks.
        if args.iter().any(|a| a == "--update-now") {
            let replaced = crate::update::check(&|l, w| println!("{l} {w}"), false);
            println!("replaced: {replaced}");
            return;
        }
        // `--rclick x y`: the same with the right button.
        if let Some(i) = args.iter().position(|a| a == "--click" || a == "--rclick") {
            let right = args[i] == "--rclick";
            use std::ffi::c_void;
            #[repr(C)]
            struct P {
                x: f64,
                y: f64,
            }
            #[link(name = "CoreGraphics", kind = "framework")]
            unsafe extern "C" {
                fn CGEventCreateMouseEvent(source: *const c_void, kind: u32, at: P, button: u32) -> *mut c_void;
                fn CGEventPost(tap: u32, event: *mut c_void);
            }
            let x: f64 = args.get(i + 1).and_then(|v| v.parse().ok()).unwrap_or(0.0);
            let y: f64 = args.get(i + 2).and_then(|v| v.parse().ok()).unwrap_or(0.0);
            let (kinds, button) = if right { ([3u32, 4u32], 1u32) } else { ([1u32, 2u32], 0u32) };
            for kind in kinds {
                unsafe { CGEventPost(0, CGEventCreateMouseEvent(std::ptr::null(), kind, P { x, y }, button)) };
                std::thread::sleep(std::time::Duration::from_millis(60));
            }
            return;
        }
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
        crate::learn::start(engine.clone(), dirs(), log);
        crate::update::start(log);
        // On at login from the first start, as decided; the menu changes it after.
        {
            let mut p = crate::prefs::Prefs::load(&dirs());
            if !p.login_set {
                set_login(true);
                p.login_set = true;
                p.save(&dirs());
            }
        }
        let app: &'static App = Box::leak(Box::new(App {
            view: View::new(mtm),
            worker: Worker::start(engine.clone(), Box::new(|| crate::apple::Helper::start().map(|h| Box::new(h) as Box<dyn worker::Translator>)), log),
            engine: engine.clone(),
            card: RefCell::default(),
            card_view: RefCell::default(),
            meanings: crate::meaning::Meanings::new(&dirs()),
            langs,
            flags: flags(),
            paused: Cell::new(false),
            prefs: RefCell::new(crate::prefs::Prefs::load(&dirs())),
            typing_in: RefCell::default(),
            woken: RefCell::default(),
            preview: RefCell::new(crate::preview::Preview::new(Instant::now())),
            floaters: crate::float::Floaters::new(&dirs(), spots, crate::idle::video),
            swapped: Default::default(),
        }));

        // The menu: pause, how many languages, readings, off in this app, the
        // Mac's translation languages, open at login, quit.
        let menu = Menu::new();
        let pause = MenuItem::new("Pause", true, None);
        let shown_menu = Submenu::new("Languages shown", true);
        let counts: Vec<(usize, CheckMenuItem)> =
            [1usize, 2, 4, 6, 8, 0].iter().map(|&n| (n, CheckMenuItem::new(if n == 0 { "All".to_string() } else { n.to_string() }, true, app.prefs.borrow().shown == n, None))).collect();
        for (_, c) in &counts {
            let _ = shown_menu.append(c);
        }
        let readings = CheckMenuItem::new("Readings over Japanese and Chinese", true, app.prefs.borrow().readings, None);
        let off_here = CheckMenuItem::new("Off in this app", true, false, None);
        let selection = CheckMenuItem::new("Preview selected text", true, app.prefs.borrow().selection, None);
        let copy_click = CheckMenuItem::new("Copy a word when clicked", true, app.prefs.borrow().copy_click, None);
        // How many learning words float in the screen's empty spots.
        let words_menu = Submenu::new("Words on screen", true);
        let word_counts: Vec<(usize, CheckMenuItem)> =
            (0usize..=12).map(|n| (n, CheckMenuItem::new(if n == 0 { "Off".to_string() } else { n.to_string() }, true, app.prefs.borrow().words_on_screen == n, None))).collect();
        for (_, c) in &word_counts {
            let _ = words_menu.append(c);
        }
        // How many languages they come from (the first few, in the owner's order).
        let langs_menu = Submenu::new("Languages on screen", true);
        let lang_counts: Vec<(usize, CheckMenuItem)> = std::iter::once(0usize)
            .chain(1..=app.langs.len())
            .map(|n| (n, CheckMenuItem::new(if n == 0 { "All".to_string() } else { n.to_string() }, true, app.prefs.borrow().words_langs == n, None)))
            .collect();
        for (_, c) in &lang_counts {
            let _ = langs_menu.append(c);
        }
        let words_typing = CheckMenuItem::new("While typing", true, app.prefs.borrow().words_typing, None);
        let words_video = CheckMenuItem::new("While a video plays", true, app.prefs.borrow().words_video, None);
        let words_shots = CheckMenuItem::new("Hide in screenshots", true, app.prefs.borrow().words_hide_shots, None);
        let _ = words_menu.append_items(&[&PredefinedMenuItem::separator(), &langs_menu, &PredefinedMenuItem::separator(), &words_typing, &words_video, &words_shots]);
        let chat_swap = CheckMenuItem::new("⌥⏎ in ChatGPT and Claude: into my language", true, app.prefs.borrow().chat_swap, None);
        // Translations run on this Mac, in the languages downloaded in System
        // Settings: this opens that page, and names any still missing.
        let languages = MenuItem::new("ⓘ Translation languages…", true, None);
        let login = CheckMenuItem::new("Open at login", true, login_on(), None);
        let quit = MenuItem::new("Quit", true, None);
        let sep = PredefinedMenuItem::separator();
        let sep2 = PredefinedMenuItem::separator();
        let sep3 = PredefinedMenuItem::separator();
        let _ = menu.append_items(&[&pause, &sep, &shown_menu, &readings, &selection, &copy_click, &words_menu, &chat_swap, &off_here, &sep2, &languages, &login, &sep3, &quit]);
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
        let (selection_id, copy_id, swap_id) = (selection.id().clone(), copy_click.id().clone(), chat_swap.id().clone());
        let (typing_id, video_id, shots_id) = (words_typing.id().clone(), words_video.id().clone(), words_shots.id().clone());
        let menu_tick = Cell::new(0u32);

        // ⌥⏎ in ChatGPT and Claude: the message box into the first learning language.
        let started = crate::chatswap::start(
            move || app.prefs.borrow().chat_swap && !app.paused.get() && frontmost().is_some_and(|b| crate::chatswap::APPS.contains(&b.as_str())),
            {
                let swapper = app.langs.first().map(|to| crate::chatswap::Swapper::start(to.clone(), app.swapped.clone(), log));
                move || {
                    if let Some(s) = &swapper {
                        s.press();
                    }
                }
            },
            // A screenshot key: the words on screen get out of the picture.
            move || app.floaters.screenshot(),
        );
        log("info", &format!("chat swap key {}", if started { "on" } else { "unavailable" }));

        // Shift + scroll anywhere while the preview is up (owner, 2026-11-11):
        // back through a long sentence and forward again, a word per notch;
        // the app under the pointer doesn't scroll sideways meanwhile.
        {
            let acc = Cell::new(0.0f64);
            let on = crate::chatswap::scroll_tap(
                move || app.view.is_shown(),
                move |d: f64, precise: bool| {
                    let Some(mtm) = MainThreadMarker::new() else { return };
                    acc.set(acc.get() + d / if precise { 12.0 } else { 1.0 });
                    let n = acc.get().trunc();
                    if n != 0.0 {
                        acc.set(acc.get() - n);
                        app.view.scroll(mtm, n as isize);
                        app.preview.borrow_mut().touch(Instant::now());
                    }
                },
            );
            log("info", &format!("preview scroll {}", if on { "on" } else { "unavailable" }));
        }

        // A word on screen clicked: Good, Again or Easy (float.rs).
        crate::float::CLICK.with(|c| {
            *c.borrow_mut() = Some(Box::new(move |id: isize, kind: u8| {
                if let Some(mtm) = MainThreadMarker::new() {
                    app.floaters.clicked(mtm, id, kind);
                }
            }));
        });

        // A word clicked: its card; Learning / Known on the card set its status
        // everywhere (ccez-words), and the preview redraws with it.
        crate::view::CLICK.with(|c| {
            *c.borrow_mut() = Some(Box::new(move |tag: isize| {
                let Some(mtm) = MainThreadMarker::new() else {
                    return;
                };
                match tag {
                    -4 => {
                        if let Some((_, _, text)) = app.card.borrow().clone() {
                            copy(&text);
                        }
                    }
                    -1 => {
                        app.view.close_card();
                        *app.card.borrow_mut() = None;
                    }
                    -2 | -3 => {
                        let Some((lang, key, text)) = app.card.borrow().clone() else {
                            return;
                        };
                        let want = if tag == -2 { ccez_keys_core::WordStatus::Learning } else { ccez_keys_core::WordStatus::Known };
                        let now = app.engine.word_status(lang.clone(), key.clone());
                        let next = if now == want { ccez_keys_core::WordStatus::New } else { want };
                        app.engine.set_word_status(lang.clone(), key.clone(), next);
                        let e = app.engine.clone();
                        std::thread::spawn(move || {
                            let _ = e.sync_words("ccez-keys-mac".into());
                        });
                        log("info", &format!("status {lang} set"));
                        let _ = text;
                        app.view.close_card();
                        *app.card.borrow_mut() = None;
                        // Asked again: the cached translation comes back with the new status.
                        app.preview.borrow_mut().redo();
                    }
                    t if t <= -1_000_000 => swap(app, (-1_000_000 - t) as usize),
                    t if t <= -1000 => {
                        // A line's flag, marks or blank space: the line read aloud and copied.
                        let rows = app.preview.borrow().line.clone();
                        if let Some(r) = rows.get((-1000 - t) as usize).filter(|r| !r.toks.iter().any(|t| t.key == PENDING)) {
                            let text: String = r.toks.iter().map(|t| t.text.as_str()).collect::<String>().trim().to_string();
                            crate::speak::say(&r.lang, &text);
                            copy(&text);
                            app.preview.borrow_mut().touch(Instant::now());
                        }
                    }
                    t if t >= 0 => {
                        let rows = app.preview.borrow().line.clone();
                        let (ri, ti) = ((t / crate::view::WORDS) as usize, (t % crate::view::WORDS) as usize);
                        let Some(tok) = rows.get(ri).and_then(|r| r.toks.get(ti).map(|tk| (r.lang.clone(), tk.clone()))) else {
                            return;
                        };
                        let (lang, mut tok) = tok;
                        // Japanese and Chinese: a kanji or hanzi clicked is its whole word.
                        if (lang.starts_with("ja") || lang.starts_with("zh")) && tok.key != tok.text && !tok.key.is_empty() {
                            let word = tok.key.clone();
                            // Its reading from its own pieces (the run around the one clicked).
                            let toks = &rows[ri].toks;
                            let (mut a, mut b) = (ti, ti);
                            while a > 0 && toks[a - 1].key == word {
                                a -= 1;
                            }
                            while b + 1 < toks.len() && toks[b + 1].key == word {
                                b += 1;
                            }
                            let sep = if lang.starts_with("zh") { " " } else { "" };
                            let reading = toks[a..=b].iter().map(|t| if t.reading.is_empty() { t.text.clone() } else { t.reading.clone() }).collect::<Vec<_>>().join(sep);
                            tok = crate::view::Tok { text: word, reading, ..tok };
                        }
                        // Copied at once, and said so before the card loads (owner, 2026-11-11).
                        if app.prefs.borrow().copy_click {
                            copy(&tok.text);
                            app.view.toast(mtm, &format!("Copied “{}”", tok.text));
                        }
                        let card = app.engine.word_card(lang.clone(), tok.text.clone(), if tok.key != tok.text { tok.key.clone() } else { String::new() });
                        let (head, sub, meaning) = crate::cards::card_face(&app.engine, &lang, &tok, card.as_ref());
                        let key = card.as_ref().map(|c| if c.lemma.is_empty() { tok.text.clone() } else { c.lemma.clone() }).unwrap_or_else(|| tok.key.clone());
                        let st = match app.engine.word_status(lang.clone(), key.clone()) {
                            ccez_keys_core::WordStatus::Learning => 1,
                            ccez_keys_core::WordStatus::Known => 2,
                            _ => 0,
                        };
                        crate::speak::say(&lang, &tok.text);
                        // What it means in this sentence: kept from before, or asked of the on-device model.
                        let sentence: String = rows[ri].toks.iter().map(|t| t.text.as_str()).collect();
                        let mkey = crate::meaning::key(&lang, &tok.text, &sentence);
                        let here = app.meanings.known(&mkey);
                        if here.is_none() {
                            app.meanings.ask(lang.clone(), tok.text.clone(), sentence);
                        }
                        app.view.open_card(mtm, &head, &sub, &crate::cards::card_text(here.as_deref(), &meaning), st);
                        *app.card_view.borrow_mut() = Some((head, sub, meaning, st, mkey));
                        *app.card.borrow_mut() = Some((lang, key, tok.text.clone()));
                        // The card stays as long as the preview, counted from the click.
                        app.preview.borrow_mut().touch(Instant::now());
                    }
                    _ => {}
                }
            }));
        });

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
                        app.preview.borrow_mut().redo();
                    } else if e.id == selection_id || e.id == copy_id || e.id == swap_id || e.id == typing_id || e.id == video_id || e.id == shots_id {
                        {
                            let mut p = app.prefs.borrow_mut();
                            p.selection = selection.is_checked();
                            p.copy_click = copy_click.is_checked();
                            p.chat_swap = chat_swap.is_checked();
                            p.words_typing = words_typing.is_checked();
                            p.words_video = words_video.is_checked();
                            p.words_hide_shots = words_shots.is_checked();
                            p.save(&dirs());
                        }
                        app.preview.borrow_mut().redo();
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
                    } else if let Some((n, _)) = lang_counts.iter().find(|(_, c)| *c.id() == e.id) {
                        app.prefs.borrow_mut().words_langs = *n;
                        app.prefs.borrow().save(&dirs());
                        for (m, c) in &lang_counts {
                            c.set_checked(m == n);
                        }
                    } else if let Some((n, _)) = word_counts.iter().find(|(_, c)| *c.id() == e.id) {
                        app.prefs.borrow_mut().words_on_screen = *n;
                        app.prefs.borrow().save(&dirs());
                        for (m, c) in &word_counts {
                            c.set_checked(m == n);
                        }
                    } else if let Some((n, _)) = counts.iter().find(|(_, c)| *c.id() == e.id) {
                        app.prefs.borrow_mut().shown = *n;
                        app.prefs.borrow().save(&dirs());
                        for (m, c) in &counts {
                            c.set_checked(m == n);
                        }
                        app.preview.borrow_mut().redo();
                    }
                }
                // "Off in Claude": the menu names the app being typed in (every second or so).
                menu_tick.set(menu_tick.get() + 1);
                if menu_tick.get().is_multiple_of(7) {
                    if let Some((bundle, name)) = app.typing_in.borrow().clone() {
                        off_here.set_text(format!("Off in {name}"));
                        off_here.set_checked(quiet(app, &bundle));
                    }
                    let missing = app.worker.missing.lock().unwrap().clone();
                    let flags: Vec<String> = missing.iter().map(|l| app.flags.get(l).cloned().unwrap_or_else(|| l.clone())).collect();
                    languages.set_text(if flags.is_empty() { "ⓘ Translation languages…".to_string() } else { format!("ⓘ Translation languages… ({} missing)", flags.join(" ")) });
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
        auto_launch::AutoLaunchBuilder::new().set_app_name("ccez-keys").set_app_path(&bundle).set_macos_launch_mode(auto_launch::MacOSLaunchMode::SMAppService).build().ok()
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
    pc::run();
}
