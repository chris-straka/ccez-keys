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
mod keys;
#[cfg(target_os = "macos")]
mod learn;
#[cfg(target_os = "macos")]
mod prefs;
#[cfg(target_os = "macos")]
mod meaning;
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

    /// How long the preview stays after typing stops (⌥ brings it back): a
    /// minute (owner, 2026-11-09). It goes at once on leaving the field or
    /// app, sending (the field empties) or a new line.
    const LINGER: Duration = Duration::from_secs(60);
    /// The words of a line still being translated ("…" held in its place).
    const PENDING: &str = "\u{1}pending";
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
        engine: std::sync::Arc<ccez_keys_core::Engine>,
        /** The word whose card is open: language, dictionary form, as shown. */
        card: RefCell<Option<(String, String, String)>>,
        /** What the open card shows: its word, the dictionary's meaning, its status, and
         * the key its meaning-here comes back under. */
        card_view: RefCell<Option<(String, String, String, u8, String)>>,
        meanings: std::sync::Arc<crate::meaning::Meanings>,
        /** A translation put in place of the sentence (right-click): not translated
         * again while the sentence still starts with it. */
        swapped: RefCell<String>,
        prefs: RefCell<crate::prefs::Prefs>,
        /** The app being typed in (bundle id, name): the menu's "Off in …" is about it. */
        typing_in: RefCell<Option<(String, String)>>,
        sentence: RefCell<String>,
        caret: Cell<ax::Place>,
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
        if let Some(pid) = NSWorkspace::sharedWorkspace().frontmostApplication().map(|a| a.processIdentifier())
            && app.woken.borrow_mut().insert(pid) {
                ax::wake(pid);
            }
        let field = ax::focused();
        let Some(field) = field.filter(|f| !f.secure) else {
            app.view.hide();
            app.sentence.borrow_mut().clear();
            app.line.borrow_mut().clear();
            return;
        };
        // The sentence typed, still shown after its full stop and a space (as on
        // the phone); a new line or an emptied field starts blank (sentence_of).
        let sentence = sentence_of(field.before.clone());
        // Just swapped in for a translation (and typed on from there): nothing to show.
        {
            let mut sw = app.swapped.borrow_mut();
            if !sw.is_empty() {
                if sentence.starts_with(sw.as_str()) {
                    app.view.hide();
                    return;
                }
                sw.clear();
            }
        }
        // Nothing secret is ever shown or translated: the sentence, or what's near it.
        let tail: String = field.before.chars().rev().take(300).collect::<Vec<_>>().into_iter().rev().collect();
        if sentence.trim().is_empty() || looks_secret(tail) {
            app.view.hide();
            if !app.sentence.borrow().is_empty() {
                // An answer still on its way for the old sentence is dropped.
                app.seq.set(app.seq.get() + 1);
            }
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
                app.view.close_card();
            } else {
                // Typed on: lines still waiting were for the old words; the next answer brings them.
                app.line.borrow_mut().retain(|r| !r.toks.iter().any(|t| t.key == PENDING));
            }
            *app.sentence.borrow_mut() = sentence.clone();
            // Answers for what was there before don't belong to this one.
            app.seq.set(app.seq.get() + 1);
            app.caret.set(ax::Place { caret: field.caret, frame: field.frame });
            app.changed.set(Instant::now());
            app.asked.set(false);
        } else if field.caret.is_some() || field.frame.is_some() {
            app.caret.set(ax::Place { caret: field.caret, frame: field.frame });
        }
        // Translated once typing pauses a moment, not for every letter.
        if !app.asked.get() && app.changed.get().elapsed() >= PAUSE {
            app.asked.set(true);
            app.seq.set(app.seq.get() + 1);
            let shown = app.prefs.borrow().shown;
            let langs: Vec<String> = if shown == 0 { app.langs.clone() } else { app.langs.iter().take(shown + 2).cloned().collect() };
            app.worker.ask(Ask { seq: app.seq.get(), text: sentence, from: "en".into(), langs, max: if shown == 0 { usize::MAX } else { shown } });
        }
        // A card's meaning-here, arriving after it opened.
        if let Some((k, here)) = app.meanings.out.lock().unwrap().take()
            && app.view.card_open.get()
            && let Some((head, sub, dict, st, key)) = app.card_view.borrow().clone()
            && key == k
        {
            app.view.open_card(mtm, &head, &sub, &card_text(Some(&here), &dict), st);
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

    /// A card's face, as on the phone: the word with its article ("la gare";
    /// the form typed when it's a form of another), a small line with its
    /// reading and form ("past participle · unterhalten"), and up to three
    /// dictionary meanings (its word's, when the form has none of its own).
    fn card_face(engine: &ccez_keys_core::Engine, lang: &str, tok: &crate::view::Tok, card: Option<&ccez_keys_core::Card>) -> (String, String, String) {
        use ccez_keys_core::text::form_tag;
        let Some(c) = card else { return (tok.text.clone(), tok.reading.clone(), String::new()) };
        let real = |ms: &[String]| ms.iter().filter(|m| form_tag((*m).clone()).is_empty()).take(3).cloned().collect::<Vec<_>>();
        let mut meanings = real(&c.meanings);
        // Only a form of the card's own word gets the tag ("gegangen" of
        // "gehen"; "gare" is a word of its own, though also a form of "garer").
        let form = (!c.lemma.is_empty() && c.lemma != tok.text)
            .then(|| c.meanings.iter().map(|m| form_tag(m.clone())).find(|t| t.ends_with(&format!(" · {}", c.lemma))))
            .flatten();
        if meanings.is_empty() && !c.lemma.is_empty() && c.lemma != tok.text
            && let Some(l) = engine.word_card(lang.into(), c.lemma.clone(), String::new()) {
                meanings = real(&l.meanings);
            }
        let word = if form.is_some() || c.lemma.is_empty() { tok.text.clone() } else { c.lemma.clone() };
        let head = match c.article.as_str() {
            "" => word,
            a if a.ends_with('\'') => format!("{a}{word}"),
            a => format!("{a} {word}"),
        };
        let reading = if tok.reading.is_empty() { c.reading.clone() } else { tok.reading.clone() };
        let sub: Vec<String> = [Some(reading).filter(|r| !r.is_empty() && *r != tok.text), form].into_iter().flatten().collect();
        (head, sub.join("   "), meanings.join("; "))
    }

    #[cfg(test)]
    #[test]
    fn card_faces_from_the_dictionaries() {
        // The dictionaries this Mac downloaded, in a scratch folder of their own.
        let dict = dirs().join("dict");
        if !dict.exists() {
            return;
        }
        let tmp = std::env::temp_dir().join(format!("ccez-keys-card-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);
        let _ = std::os::unix::fs::symlink(&dict, tmp.join("dict"));
        let e = ccez_keys_core::Engine::new(tmp.to_string_lossy().into_owned());
        let face = |lang: &str, word: &str| {
            let tok = crate::view::Tok { text: word.into(), reading: String::new(), word: true, key: word.into(), status: 0 };
            let c = e.word_card(lang.into(), word.into(), String::new());
            let f = card_face(&e, lang, &tok, c.as_ref());
            eprintln!("{lang} {word}: {f:?}");
            f
        };
        let (head, sub, meaning) = face("fr", "gare");
        assert_eq!((head.as_str(), sub.as_str()), ("la gare", ""));
        assert!(!meaning.is_empty());
        let (head, sub, meaning) = face("de", "gegangen");
        assert_eq!(head, "gegangen");
        assert!(sub.contains("participle"), "{sub}");
        assert!(!meaning.is_empty());
        face("es", "estación");
        face("ja", "駅");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Right-click on a line: that translation in place of the sentence typed
    /// (spaces after it kept). Through Accessibility where the app takes it,
    /// otherwise selected and typed over, or deleted and typed; ⌘Z undoes it.
    fn swap(app: &App, row: usize) {
        let rows = app.line.borrow().clone();
        let Some(r) = rows.get(row).filter(|r| !r.toks.iter().any(|t| t.key == PENDING)) else { return };
        let text: String = r.toks.iter().map(|t| t.text.as_str()).collect::<String>().trim().to_string();
        let Some(f) = ax::focused().filter(|f| !f.secure) else { return };
        let sentence = sentence_of(f.before.clone());
        // Only the sentence this line translates, as it is now.
        if sentence.is_empty() || sentence != *app.sentence.borrow() || looks_secret(f.before.clone()) {
            return;
        }
        let trimmed = f.before.trim_end();
        if !trimmed.ends_with(sentence.as_str()) {
            return;
        }
        let trailing = &f.before[trimmed.len()..];
        let end = trimmed.encode_utf16().count();
        let len = sentence.encode_utf16().count();
        let (done, selected) = ax::replace_range(end - len, len, &text);
        if !done {
            if selected {
                crate::keys::type_text(&text);
            } else {
                crate::keys::delete(sentence.chars().count() + trailing.chars().count());
                crate::keys::type_text(&format!("{text}{trailing}"));
            }
        }
        log("info", &format!("swapped a sentence for {} ({})", r.lang, if done { "accessibility" } else if selected { "typed over" } else { "retyped" }));
        *app.swapped.borrow_mut() = text;
        app.line.borrow_mut().clear();
        app.view.hide();
    }

    /// The card's meaning: what it means here (on-device model) over the dictionary's.
    fn card_text(here: Option<&str>, dict: &str) -> String {
        match here {
            Some(h) if dict.is_empty() || h.eq_ignore_ascii_case(dict) => h.to_string(),
            Some(h) => format!("{h}\n{dict}"),
            None => dict.to_string(),
        }
    }

    /// One line per language, in your order: "🇫🇷 je te retrouve …"; Japanese
    /// and Chinese with their readings over the words.
    fn render(app: &App, shown: &worker::Shown) -> Vec<crate::view::Row> {
        let p = app.prefs.borrow();
        let max = if p.shown == 0 { usize::MAX } else { p.shown };
        let row = |lang: &str, toks: Vec<crate::view::Tok>| crate::view::Row {
            lang: lang.to_string(),
            flag: app.flags.get(lang).cloned().unwrap_or_else(|| lang.to_string()),
            toks,
        };
        let mut rows: Vec<crate::view::Row> = shown
            .lines
            .iter()
            .take(max)
            .map(|l| row(&l.lang, l.toks.iter().map(|t| crate::view::Tok { reading: if p.readings { t.reading.clone() } else { String::new() }, ..t.clone() }).collect()))
            .collect();
        // Languages still coming: a dim "…" in their place, as tall as they'll be.
        for lang in shown.pending.iter().take(max.saturating_sub(rows.len())) {
            let cjk = matches!(lang.as_str(), "ja" | "zh" | "zh-Hans" | "zh-Hant");
            let reading = if cjk && p.readings { " ".to_string() } else { String::new() };
            rows.push(row(lang, vec![crate::view::Tok { text: "…".into(), reading, word: false, key: PENDING.into(), status: 0 }]));
        }
        rows
    }    /// `--demo "line"`: the preview with that line at the screen's foot for a few
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
            struct P { x: f64, y: f64 }
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
            worker: Worker::start(engine.clone(), log),
            engine: engine.clone(),
            card: RefCell::default(),
            card_view: RefCell::default(),
            meanings: crate::meaning::Meanings::new(&dirs()),
            swapped: RefCell::default(),
            langs,
            flags: flags(),
            paused: Cell::new(false),
            prefs: RefCell::new(crate::prefs::Prefs::load(&dirs())),
            typing_in: RefCell::default(),
            sentence: RefCell::default(),
            caret: Cell::new(ax::Place::default()),
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
        // Translations run on this Mac, in the languages downloaded in System
        // Settings: this opens that page, and names any still missing.
        let languages = MenuItem::new("ⓘ Translation languages…", true, None);
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

        // A word clicked: its card; Learning / Known on the card set its status
        // everywhere (ccez-words), and the preview redraws with it.
        crate::view::CLICK.with(|c| {
            *c.borrow_mut() = Some(Box::new(move |tag: isize| {
                let Some(mtm) = MainThreadMarker::new() else { return };
                match tag {
                    -1 => {
                        app.view.close_card();
                        *app.card.borrow_mut() = None;
                    }
                    -2 | -3 => {
                        let Some((lang, key, text)) = app.card.borrow().clone() else { return };
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
                        app.sentence.borrow_mut().clear();
                    }
                    t if t <= -1_000_000 => swap(app, (-1_000_000 - t) as usize),
                    t if t <= -1000 => {
                        // A line's flag or blank space: the line read aloud.
                        let rows = app.line.borrow().clone();
                        if let Some(r) = rows.get((-1000 - t) as usize).filter(|r| !r.toks.iter().any(|t| t.key == PENDING)) {
                            let text: String = r.toks.iter().map(|t| t.text.as_str()).collect();
                            crate::speak::say(&r.lang, &text);
                            app.appeared.set(Instant::now());
                        }
                    }
                    t if t >= 0 => {
                        let rows = app.line.borrow().clone();
                        let (ri, ti) = ((t / 1000) as usize, (t % 1000) as usize);
                        let Some(tok) = rows.get(ri).and_then(|r| r.toks.get(ti).map(|tk| (r.lang.clone(), tk.clone()))) else { return };
                        let (lang, mut tok) = tok;
                        // Japanese and Chinese: a kanji or hanzi clicked is its whole word.
                        if (lang.starts_with("ja") || lang.starts_with("zh")) && tok.key != tok.text && !tok.key.is_empty() {
                            let word = tok.key.clone();
                            // Its reading from its own pieces (the run around the one clicked).
                            let toks = &rows[ri].toks;
                            let (mut a, mut b) = (ti, ti);
                            while a > 0 && toks[a - 1].key == word { a -= 1; }
                            while b + 1 < toks.len() && toks[b + 1].key == word { b += 1; }
                            let sep = if lang.starts_with("zh") { " " } else { "" };
                            let reading = toks[a..=b].iter().map(|t| if t.reading.is_empty() { t.text.clone() } else { t.reading.clone() }).collect::<Vec<_>>().join(sep);
                            tok = crate::view::Tok { text: word, reading, ..tok };
                        }
                        let card = app.engine.word_card(lang.clone(), tok.text.clone(), if tok.key != tok.text { tok.key.clone() } else { String::new() });
                        let (head, sub, meaning) = card_face(&app.engine, &lang, &tok, card.as_ref());
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
                        app.view.open_card(mtm, &head, &sub, &card_text(here.as_deref(), &meaning), st);
                        *app.card_view.borrow_mut() = Some((head, sub, meaning, st, mkey));
                        *app.card.borrow_mut() = Some((lang, key, tok.text.clone()));
                        // The card stays as long as the preview, counted from the click.
                        app.appeared.set(Instant::now());
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
