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
#[cfg(target_os = "macos")]
mod menu_ipc;
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
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::{Duration, Instant};

    use block2::RcBlock;
    use objc2::MainThreadMarker;
    use objc2::rc::autoreleasepool;
    use objc2_app_kit::{
        NSApplication, NSApplicationActivationPolicy, NSRunningApplication, NSWorkspace,
        NSWorkspaceApplicationKey, NSWorkspaceDidActivateApplicationNotification,
        NSWorkspaceDidLaunchApplicationNotification, NSWorkspaceDidTerminateApplicationNotification,
        NSWorkspaceScreensDidSleepNotification, NSWorkspaceScreensDidWakeNotification,
    };
    use objc2_foundation::{NSDistributedNotificationCenter, NSNotification, NSNotificationName, NSString, NSTimer};
    use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
    use tray_icon::{TrayIcon, TrayIconBuilder};

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

    struct FallbackTray {
        _tray: TrayIcon,
        pause_id: tray_icon::menu::MenuId,
        pause: MenuItem,
        off_id: tray_icon::menu::MenuId,
        off_here: CheckMenuItem,
        lang_id: tray_icon::menu::MenuId,
        languages: MenuItem,
        login_id: tray_icon::menu::MenuId,
        login: CheckMenuItem,
        quit_id: tray_icon::menu::MenuId,
        front: Cell<bool>,
    }

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
        /** When it shows and what it says (the same on every system). */
        preview: RefCell<crate::preview::Preview>,
        /** Learning words on screen (float.rs). */
        floaters: crate::float::Floaters,
        /** A ⌥⏎ swap's note for the toast, from its thread. */
        swapped: Arc<Mutex<Option<String>>>,
        awake: Arc<AtomicBool>,
        front_quiet: Arc<AtomicBool>,
        ax_paused: Arc<AtomicBool>,
        ask_in_flight: Arc<AtomicBool>,
        ax_field: Arc<Mutex<Option<crate::model::Field>>>,
        ax_signal: Arc<(Mutex<AxWake>, Condvar)>,
        ipc_queue: crate::menu_ipc::Queue,
        tray_events: Arc<Mutex<Vec<String>>>,
        ui_timer: Cell<Option<objc2::rc::Retained<NSTimer>>>,
        fallback_tray: RefCell<Option<FallbackTray>>,
        last_missing: RefCell<Vec<String>>,
        last_tray_front: RefCell<Option<(String, String, bool)>>,
    }

    #[derive(Default)]
    struct AxWake {
        wake_pids: Vec<i32>,
        dirty: bool,
    }

    static WAKE_PENDING: AtomicBool = AtomicBool::new(false);
    thread_local! {
        static APP_REF: Cell<Option<&'static App>> = const { Cell::new(None) };
    }

    extern "C" fn on_wake(_ctx: *mut std::ffi::c_void) {
        WAKE_PENDING.store(false, Ordering::Release);
        if let Some(mtm) = MainThreadMarker::new() {
            APP_REF.with(|c| {
                if let Some(app) = c.get() {
                    autoreleasepool(|_| drain_and_tick(mtm, app));
                }
            });
        }
    }

    fn wake_main() {
        if !WAKE_PENDING.swap(true, Ordering::AcqRel) {
            unsafe extern "C" {
                static _dispatch_main_q: u8;
                fn dispatch_async_f(queue: *const u8, context: *mut std::ffi::c_void, work: extern "C" fn(*mut std::ffi::c_void));
            }
            unsafe { dispatch_async_f(&_dispatch_main_q as *const u8, std::ptr::null_mut(), on_wake) };
        }
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
        unsafe extern "C" {
            fn CGEventSourceFlagsState(state: i32) -> u64;
        }
        (unsafe { CGEventSourceFlagsState(0) } & (1 << 19)) != 0
    }

    /// Off in this app: quiet by default (and not turned back on), or turned off from the menu.
    fn quiet(app: &App, bundle: &str) -> bool {
        let p = app.prefs.borrow();
        p.off.iter().any(|b| b == bundle) || (QUIET.contains(&bundle) && !p.on.iter().any(|b| b == bundle))
    }

    fn sync_front_app(app: &App) {
        let Some(a) = NSWorkspace::sharedWorkspace().frontmostApplication() else {
            app.front_quiet.store(false, Ordering::Relaxed);
            return;
        };
        let bundle = a.bundleIdentifier().map(|s| s.to_string()).unwrap_or_default();
        let pid = a.processIdentifier();
        if bundle != "uk.ccez.keys.mac" && bundle != "uk.ccez.clip" && !bundle.is_empty() {
            let name = a.localizedName().map(|s| s.to_string()).unwrap_or_else(|| bundle.clone());
            *app.typing_in.borrow_mut() = Some((bundle.clone(), name));
        }
        let q = bundle == "uk.ccez.keys.mac" || bundle == "uk.ccez.clip" || (!bundle.is_empty() && quiet(app, &bundle));
        app.front_quiet.store(q, Ordering::Relaxed);
        let (lock, cvar) = &*app.ax_signal;
        {
            let mut g = lock.lock().unwrap();
            if !q && pid > 0 && !app.paused.get() && !g.wake_pids.contains(&pid) {
                g.wake_pids.push(pid);
            }
            g.dirty = true;
        }
        cvar.notify_all();
    }

    fn start_ax_thread(
        awake: Arc<AtomicBool>,
        front_quiet: Arc<AtomicBool>,
        paused: Arc<AtomicBool>,
        ask_in_flight: Arc<AtomicBool>,
        out: Arc<Mutex<Option<crate::model::Field>>>,
        signal: Arc<(Mutex<AxWake>, Condvar)>,
    ) {
        std::thread::Builder::new()
            .name("ax-field".into())
            .spawn(move || {
                let mut reader = ax::FocusReader::default();
                let mut woken = std::collections::HashSet::new();
                let mut last: Option<crate::model::Field> = None;
                let mut last_opt = false;
                let (lock, cvar) = &*signal;
                loop {
                    let wake_pid = {
                        let mut g = lock.lock().unwrap();
                        g.dirty = false;
                        g.wake_pids.pop()
                    };
                    if let Some(pid) = wake_pid
                        && woken.insert(pid)
                    {
                        ax::wake(pid);
                    }
                    let stopped = !awake.load(Ordering::Relaxed) || paused.load(Ordering::Relaxed) || front_quiet.load(Ordering::Relaxed);
                    if stopped {
                        if last.is_some() || last_opt {
                            last = None;
                            last_opt = false;
                            *out.lock().unwrap() = None;
                            wake_main();
                        }
                        let mut g = lock.lock().unwrap();
                        while !g.dirty && g.wake_pids.is_empty() && (!awake.load(Ordering::Relaxed) || paused.load(Ordering::Relaxed) || front_quiet.load(Ordering::Relaxed)) {
                            g = cvar.wait(g).unwrap();
                        }
                        continue;
                    }
                    let field = autoreleasepool(|_| reader.read());
                    let opt = option_held();
                    if field != last || opt != last_opt {
                        last = field.clone();
                        last_opt = opt;
                        *out.lock().unwrap() = field;
                        wake_main();
                    }
                    let active = crate::idle::input_idle() < Duration::from_secs(2) || opt || ask_in_flight.load(Ordering::Relaxed);
                    let wait = if active { Duration::from_millis(150) } else { Duration::from_millis(1500) };
                    let g = lock.lock().unwrap();
                    if !g.dirty && g.wake_pids.is_empty() {
                        let _ = cvar.wait_timeout(g, wait).unwrap();
                    }
                }
            })
            .ok();
    }

    fn tick(mtm: MainThreadMarker, app: &App, mut field: Option<crate::model::Field>) {
        let off = !app.awake.load(Ordering::Relaxed) || app.paused.get() || app.front_quiet.load(Ordering::Relaxed);
        if off {
            field = None;
        }
        if !app.prefs.borrow().selection
            && let Some(f) = field.as_mut()
        {
            f.selected.clear();
        }
        let answer = app.worker.out.lock().unwrap().take();
        let langs = app.langs.clone();
        let shown = app.prefs.borrow().shown;
        let inputs = crate::preview::Inputs { now: Instant::now(), off, field, held: option_held(), shown, langs: &langs };
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
        let most = if app.paused.get() || !app.awake.load(Ordering::Relaxed) { 0 } else { app.prefs.borrow().words_on_screen };
        let (while_video, while_typing) = {
            let p = app.prefs.borrow();
            app.floaters.hide_in_shots.set(p.words_hide_shots);
            app.floaters.readings.set(p.readings);
            app.floaters.breather.set(std::time::Duration::from_secs(p.words_break));
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
            |lang: &str, word: &str| screen_meaning(&app.engine, lang, word),
        );
        let p = app.preview.borrow();
        if out.show {
            app.view.show(mtm, &p.line, p.place);
        } else {
            app.view.hide();
        }
        app.ask_in_flight.store(!p.complete, Ordering::Relaxed);
    }

    thread_local! {
        /// Per app, where its content keeps turning up (the spot thread's own).
        static BUSY: std::cell::RefCell<crate::idle::Busy> = std::cell::RefCell::new(crate::idle::Busy::default());
        /// The videos known in each app (the spot thread's own).
        static VIDEOS: std::cell::RefCell<crate::idle::Videos> = std::cell::RefCell::new(crate::idle::Videos::default());
        /// A video in the front window was playing at the last look (for the log).
        static PLAYING: Cell<bool> = const { Cell::new(false) };
        /// When each app's accessibility was last asked for again (spots).
        static WOKE: std::cell::RefCell<std::collections::HashMap<i32, Instant>> = std::cell::RefCell::new(std::collections::HashMap::new());
    }

    /// Where idle words can go: room in the front window its content leaves,
    /// and screen no other app's window covers. And whether a video in that
    /// window is playing (one that doesn't hold the display awake: Claude's).
    fn spots(screen: crate::model::Rect) -> (Vec<crate::model::Rect>, bool) {
        let pid = NSWorkspace::sharedWorkspace().frontmostApplication().map(|a| a.processIdentifier()).unwrap_or(0);
        let mut look = ax::content_frames(pid, 4000, std::time::Duration::from_millis(250));
        let app = NSWorkspace::sharedWorkspace().frontmostApplication().and_then(|a| a.bundleIdentifier()).map(|b| b.to_string()).unwrap_or_default();
        // A window whose text the walk didn't see (Chromium's accessibility
        // off, or switched itself off again a while after it was asked) is
        // taken whole: a word goes only where there is known to be no text
        // (owner, 2026-11-18: words over a Claude chat). Asked again, not
        // more than every 10 s, so the next look can see it.
        if let Some(w) = look.window
            && !ax::layout_known(look.texts, look.complete)
        {
            look.content.push(w);
            let again = WOKE.with_borrow_mut(|m| {
                let due = m.get(&pid).is_none_or(|t: &Instant| t.elapsed() > std::time::Duration::from_secs(10));
                if due {
                    m.insert(pid, Instant::now());
                }
                due
            });
            if again && !QUIET.contains(&app.as_str()) {
                ax::wake(pid);
            }
        }
        // Its videos: content however big, playing or not.
        let videos = VIDEOS.with_borrow_mut(|v| v.look(&app, &look.videos, Instant::now()));
        look.content.extend(videos.iter().map(|v| v.0));
        // ccez-llm in front (its app, or llm.ccez.uk in a browser): no words
        // on screen; it's reading with its own cards already (owner,
        // 2026-11-13). No spots: those up go at once.
        if crate::idle::words_stay_off(&app, look.url.as_deref()) {
            return (Vec::new(), false);
        }
        let free = crate::idle::free_spots(screen, look.window, &look.content, &crate::idle::windows(pid), 8, 6);
        // Spots this app keeps filling lately: skipped too.
        let all = crate::idle::free_cells(screen, &[], 8, 6);
        let playing = videos.iter().find(|v| v.1).map(|v| v.0);
        if PLAYING.replace(playing.is_some()) != playing.is_some() {
            match playing {
                Some(f) => log("info", &format!("video playing in {app} ({}x{})", f.w as i64, f.h as i64)),
                None => log("info", "video stopped"),
            }
        }
        let playing = playing.is_some();
        (BUSY.with_borrow_mut(|b| b.look(&app, &all, &free)), playing)
    }

    /// Whether a word at `r` (top-left coordinates) has anything under it:
    /// the app whose window is on top there, hit-tested at five points. The
    /// desktop (no window) is clear; None: that app didn't answer in time.
    fn under(r: crate::model::Rect) -> Option<bool> {
        let me = std::process::id() as i32;
        let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
        match crate::idle::owner_at(cx, cy, me) {
            Some((pid, win)) => ax::content_under(pid, &ax::sample_points(r), Some(win)),
            None => Some(false),
        }
    }

    /// Lets this app set the cursor while it isn't the front app (the
    /// window server's own switch, as Hammerspoon and others use it).
    fn cursor_in_background() {
        use std::ffi::c_void;
        #[link(name = "ApplicationServices", kind = "framework")]
        unsafe extern "C" {
            fn _CGSDefaultConnection() -> i32;
            fn CGSSetConnectionProperty(cid: i32, target: i32, key: *const c_void, value: *const c_void) -> i32;
            static kCFBooleanTrue: *const c_void;
        }
        let key = objc2_foundation::NSString::from_str("SetsCursorInBackground");
        let cid = unsafe { _CGSDefaultConnection() };
        let err = unsafe { CGSSetConnectionProperty(cid, cid, objc2::rc::Retained::as_ptr(&key) as *const c_void, kCFBooleanTrue) };
        if err != 0 {
            log("warn", &format!("cursor in background: {err}"));
        }
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

    /// A screen word as shown, its meaning and its tags (the core's
    /// `word_face`; no meaning: it isn't shown).
    fn screen_meaning(engine: &ccez_keys_core::Engine, lang: &str, word: &str) -> (String, String, Vec<String>) {
        engine.word_face(lang, word)
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
        // `--look <pid>`: that app's window as idle words see it (how much
        // content, what has a video's shape, the spots free with those
        // counted as videos), printed; reads only, never its text.
        if let Some(i) = args.iter().position(|a| a == "--look") {
            let pid = args.get(i + 1).and_then(|p| p.parse().ok()).unwrap_or(0);
            let look = ax::content_frames(pid, 4000, std::time::Duration::from_millis(250));
            println!("window={:?} content={} texts={} complete={} trusted={} videos={:?} url={:?}", look.window, look.content.len(), look.texts, look.complete, ax::layout_known(look.texts, look.complete), look.videos, look.url.as_deref().map(|u| u.split('?').next().unwrap_or("")));
            for c in &look.content {
                println!("  content {} {} {} {}", c.x as i64, c.y as i64, c.w as i64, c.h as i64);
            }
            if let Some(w) = look.window {
                let content: Vec<_> = look.content.iter().copied().chain(look.videos.iter().map(|v| v.frame)).collect();
                let free = crate::idle::free_spots(w, look.window, &content, &[], 8, 6);
                println!("spots={}: {:?}", free.len(), free.iter().map(|c| (c.x as i64, c.y as i64)).collect::<Vec<_>>());
            }
            return;
        }
        // `--guard-demo x y w h [noguard]`: one word up in that spot (top-left
        // coordinates) and the look under it the app makes every 0.3 s: it goes
        // the moment anything comes under it. With `noguard`, it stays (how it
        // was). Prints when it went; quits after 12 s. For checks.
        if let Some(i) = args.iter().position(|a| a == "--guard-demo") {
            let n = |k: usize| args.get(i + k).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
            let cell = crate::model::Rect { x: n(1), y: n(2), w: n(3), h: n(4) };
            let guard = !args.iter().any(|a| a == "noguard");
            let nsapp = NSApplication::sharedApplication(mtm);
            nsapp.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
            let primary_h = objc2_app_kit::NSScreen::screens(mtm).firstObject().map(|p| p.frame().size.height).unwrap_or(0.0);
            let panel = crate::float::demo_word(mtm, cell, primary_h);
            let start = Instant::now();
            let up = std::rc::Rc::new(Cell::new(true));
            let unsure = std::rc::Rc::new(std::cell::RefCell::new(std::collections::HashMap::new()));
            let look = RcBlock::new(move |_t: NonNull<NSTimer>| {
                if !guard || !up.get() {
                    return;
                }
                let f = panel.frame();
                let r = crate::model::Rect { x: f.origin.x, y: primary_h - (f.origin.y + f.size.height), w: f.size.width, h: f.size.height };
                if !crate::float::covered_words(&[(1, r)], under, &mut unsure.borrow_mut()).is_empty() {
                    up.set(false);
                    panel.orderOut(None);
                    println!("word off at {} ms: something under it at ({}, {}, {}, {})", start.elapsed().as_millis(), r.x as i64, r.y as i64, r.w as i64, r.h as i64);
                }
            });
            unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(0.3, true, &look) };
            let quit = RcBlock::new(move |_t: NonNull<NSTimer>| std::process::exit(0));
            unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(12.0, false, &quit) };
            println!("word up in ({}, {}, {}, {}){}", cell.x as i64, cell.y as i64, cell.w as i64, cell.h as i64, if guard { "" } else { ", no guard" });
            nsapp.run();
            return;
        }
        // `--spots`: the spots words may take right now, as the running app
        // finds them (the front app, the screen's visible frame, the busy map
        // over a few looks), left to right; reads only.
        if args.iter().any(|a| a == "--spots") {
            let Some(main) = objc2_app_kit::NSScreen::mainScreen(mtm) else { return };
            let vis = main.visibleFrame();
            let primary_h = objc2_app_kit::NSScreen::screens(mtm).firstObject().map(|p| p.frame().size.height).unwrap_or(vis.size.height);
            let screen = crate::model::Rect { x: vis.origin.x, y: primary_h - (vis.origin.y + vis.size.height), w: vis.size.width, h: vis.size.height };
            println!("screen {screen:?}");
            for _ in 0..3 {
                let (free, _) = spots(screen);
                let mut xs: Vec<(i64, i64)> = free.iter().map(|c| (c.x as i64, c.y as i64)).collect();
                xs.sort();
                println!("{} spots: {xs:?}", free.len());
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
        // `--preview-words out.png word:status …`: preview words drawn to a file.
        if let Some(i) = args.iter().position(|a| a == "--preview-words") {
            let out = args.get(i + 1).cloned().unwrap_or_default();
            let words: Vec<(String, u8)> = args[i + 2..].iter().filter_map(|w| w.rsplit_once(':').map(|(t, s)| (t.to_string(), s.parse().unwrap_or(0)))).collect();
            println!("drawn: {}", crate::view::words_picture(mtm, &words, std::path::Path::new(&out)));
            return;
        }
        // `--word-picture lang word out.png`: a screen word as it looks with
        // the pointer on it, drawn to a file; nothing comes on screen.
        if let Some(i) = args.iter().position(|a| a == "--word-picture") {
            NSApplication::sharedApplication(mtm).setActivationPolicy(NSApplicationActivationPolicy::Prohibited);
            let engine = ccez_keys_core::Engine::new(dirs().to_string_lossy().into_owned());
            let arg = |n: usize| args.get(i + n).cloned().unwrap_or_default();
            let open = args.iter().all(|a| a != "--closed");
            println!("drawn: {}", crate::float::picture(mtm, &engine, &arg(1), &arg(2), open, std::path::Path::new(&arg(3))));
            return;
        }
        // `--say-time lang word…`: how long each word takes to start being
        // said, at volume 0; with `--warm`, after a first word has loaded the voice.
        if let Some(i) = args.iter().position(|a| a == "--say-time") {
            let lang = args.get(i + 1).cloned().unwrap_or_default();
            if args.iter().any(|a| a == "--warm") {
                println!("loading the voice: {} ms", crate::speak::time_to_start(&lang, "a").as_millis());
            }
            for w in args.iter().skip(i + 2).filter(|a| !a.starts_with("--")) {
                println!("{w}: {} ms", crate::speak::time_to_start(&lang, w).as_millis());
            }
            return;
        }
        // `--say-made lang word…`: each word's sound made ahead, and where it's kept.
        if let Some(i) = args.iter().position(|a| a == "--say-made") {
            let lang = args.get(i + 1).cloned().unwrap_or_default();
            for w in args.iter().skip(i + 2) {
                let t = std::time::Instant::now();
                println!("{w}: {:?} in {} ms", crate::speak::made(&lang, w), t.elapsed().as_millis());
            }
            return;
        }
        // `--meaning lang word`: what a screen word shows (word, meaning, tags).
        if let Some(i) = args.iter().position(|a| a == "--meaning") {
            let engine = ccez_keys_core::Engine::new(dirs().to_string_lossy().into_owned());
            let (lang, word) = (args.get(i + 1).cloned().unwrap_or_default(), args.get(i + 2).cloned().unwrap_or_default());
            println!("{:?}", screen_meaning(&engine, &lang, &word));
            if let Some(c) = engine.word_card(lang.clone(), word.clone(), String::new()) {
                println!("  card: word={} lemma={} gender={} article={} pos={:?}", c.word, c.lemma, c.gender, c.article, c.pos);
            }
            for m in engine.word_card(lang.clone(), word.clone(), String::new()).map(|c| c.meanings).unwrap_or_default().iter().take(6) {
                println!("  sense: {m}");
            }
            return;
        }
        if let Some(i) = args.iter().position(|a| a == "--type") {
            return type_text(args.get(i + 1).map(String::as_str).unwrap_or(""));
        }
        if let Some(i) = args.iter().position(|a| a == "--demo") {
            return demo(mtm, args.get(i + 1).cloned().unwrap_or_default());
        }
        // A flag none of the checks above knows (a newer one run by an older
        // build) never starts a second app: two put every word up twice
        // (2026-11-13).
        if let Some(a) = args.iter().skip(1).find(|a| a.starts_with("--")) {
            eprintln!("unknown flag {a}; not starting the app");
            std::process::exit(2);
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
        let awake = Arc::new(AtomicBool::new(true));
        let front_quiet = Arc::new(AtomicBool::new(false));
        let ax_paused = Arc::new(AtomicBool::new(false));
        let ask_in_flight = Arc::new(AtomicBool::new(false));
        let ax_field: Arc<Mutex<Option<crate::model::Field>>> = Arc::default();
        let ax_signal: Arc<(Mutex<AxWake>, Condvar)> = Arc::default();
        let ipc_queue: crate::menu_ipc::Queue = Arc::default();
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(wake_main);

        let app: &'static App = Box::leak(Box::new(App {
            view: View::new(mtm),
            worker: Worker::start_with_wake(
                engine.clone(),
                Box::new(|| crate::apple::Helper::start().map(|h| Box::new(h) as Box<dyn worker::Translator>)),
                log,
                wake.clone(),
            ),
            engine: engine.clone(),
            card: RefCell::default(),
            card_view: RefCell::default(),
            meanings: crate::meaning::Meanings::new_with_wake(&dirs(), wake.clone()),
            langs,
            flags: flags(),
            paused: Cell::new(false),
            prefs: RefCell::new(crate::prefs::Prefs::load(&dirs())),
            typing_in: RefCell::default(),
            preview: RefCell::new(crate::preview::Preview::new(Instant::now())),
            floaters: crate::float::Floaters::new(engine.clone(), awake.clone(), wake.clone(), spots, crate::idle::video, under),
            swapped: Default::default(),
            awake,
            front_quiet,
            ax_paused,
            ask_in_flight,
            ax_field,
            ax_signal,
            ipc_queue: ipc_queue.clone(),
            tray_events: Arc::default(),
            ui_timer: Cell::new(None),
            fallback_tray: RefCell::new(None),
            last_missing: RefCell::new(Vec::new()),
            last_tray_front: RefCell::new(None),
        }));
        APP_REF.with(|c| c.set(Some(app)));
        crate::speak::set_on(app.prefs.borrow().speak);

        // Screenshot UI state without polling proc_listallpids.
        let cap_up = !NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str("com.apple.screencaptureui")).is_empty();
        crate::float::set_capturing_ui(cap_up);

        sync_front_app(app);
        sync_fallback_tray(mtm, app);
        start_observers(app);
        watch_running_apps(mtm);
        crate::menu_ipc::serve(dirs().join("menu.sock"), ipc_queue, wake.clone(), log);
        start_ax_thread(
            app.awake.clone(),
            app.front_quiet.clone(),
            app.ax_paused.clone(),
            app.ask_in_flight.clone(),
            app.ax_field.clone(),
            app.ax_signal.clone(),
        );

        // The pointing finger over a word or the preview even while another
        // app is in front (owner, 2026-11-11): macOS lets only the front app
        // set the cursor unless a background one asks to.
        cursor_in_background();

        // ⌥⏎ in ChatGPT and Claude: the message box into the first learning language.
        let started = crate::chatswap::start(
            move || app.prefs.borrow().chat_swap && !app.paused.get() && frontmost().is_some_and(|b| crate::chatswap::APPS.contains(&b.as_str())),
            {
                let swapper = app.langs.first().map(|to| crate::chatswap::Swapper::start_with_wake(to.clone(), app.swapped.clone(), log, wake.clone()));
                move || {
                    if let Some(s) = &swapper {
                        s.press();
                    }
                }
            },
            // A screenshot key: the words on screen get out of the picture.
            move || {
                app.floaters.screenshot();
                wake_main();
            },
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
                let Some(mtm) = MainThreadMarker::new() else { return };
                // Copied too, when "Copy a word when clicked" is on (owner,
                // 2026-11-11), as a word in the preview is.
                let shown = app.floaters.copy_text(id).unwrap_or_default();
                let spoken = app.floaters.spoken(id);
                let Some((answer, known)) = app.floaters.clicked(mtm, id, kind) else { return };
                // A left click says it too (owner, 2026-11-12), in its
                // language's voice, as a click on a preview word does; a
                // right click (Again) most of all (owner, 2026-11-13: "the
                // most important place to have TTS").
                if kind <= 1
                    && let Some((lang, text)) = spoken
                {
                    crate::speak::say(&lang, &text);
                }
                if app.prefs.borrow().copy_click && !shown.is_empty() {
                    copy(&shown);
                }
                // Every answer shows, in its colour, with Undo (owner, 2026-11-11).
                app.view.toast_answer(mtm, &answer, crate::float::UNDO_FOR.as_secs_f64());
                if let Some((lang, word)) = known {
                    // Known: off the learning list on every device (ccez-words).
                    app.engine.set_word_status(lang, word, ccez_keys_core::WordStatus::Known);
                    let e = app.engine.clone();
                    std::thread::spawn(move || {
                        let _ = e.sync_words("ccez-keys-mac".into());
                    });
                }
                ensure_ui_timer(app, needs_ui_timer(app));
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
                    crate::view::UNDO_TAG => {
                        if let Some((lang, word, known)) = app.floaters.undo(mtm) {
                            if known {
                                // Back on the learning list everywhere.
                                app.engine.set_word_status(lang, word.clone(), ccez_keys_core::WordStatus::Learning);
                                let e = app.engine.clone();
                                std::thread::spawn(move || {
                                    let _ = e.sync_words("ccez-keys-mac".into());
                                });
                            }
                            app.view.toast(mtm, &format!("Undone: {word}"));
                        }
                    }
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
                            let senses = crate::cards::card_senses(&app.engine, &lang, card.as_ref());
                            app.meanings.ask(lang.clone(), tok.text.clone(), sentence, senses);
                        }
                        app.view.open_card(mtm, &head, &sub, &crate::cards::card_text(here.as_deref(), &meaning), st);
                        *app.card_view.borrow_mut() = Some((head, sub, meaning, st, mkey));
                        *app.card.borrow_mut() = Some((lang, key, tok.text.clone()));
                        // The card stays as long as the preview, counted from the click.
                        app.preview.borrow_mut().touch(Instant::now());
                    }
                    _ => {}
                }
                ensure_ui_timer(app, needs_ui_timer(app));
            }));
        });

        {
            let q = app.tray_events.clone();
            MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
                q.lock().unwrap().push(e.id.0);
                wake_main();
            }));
        }

        drain_and_tick(mtm, app);
        nsapp.run();
    }

    fn menu_bar_image() -> objc2::rc::Retained<objc2_app_kit::NSImage> {
        use objc2::AllocAnyThread;
        use objc2_app_kit::{NSBitmapImageRep, NSImage};
        use objc2_foundation::NSData;
        let size = objc2_foundation::NSSize::new(16.0, 16.0);
        let img = NSImage::initWithSize(NSImage::alloc(), size);
        for png in [&include_bytes!("../assets/menubar-feather-16.png")[..], &include_bytes!("../assets/menubar-feather-32.png")[..]] {
            if let Some(rep) = NSBitmapImageRep::imageRepWithData(&NSData::with_bytes(png)) {
                rep.setSize(size);
                img.addRepresentation(&rep);
            }
        }
        img.setTemplate(true);
        img.setAccessibilityDescription(Some(&NSString::from_str("ccez-keys")));
        img
    }

    fn missing_flags(app: &App) -> Vec<String> {
        let missing = app.worker.missing.lock().unwrap().clone();
        missing.iter().map(|l| app.flags.get(l).cloned().unwrap_or_else(|| l.clone())).collect()
    }

    fn menu_state(app: &App, front_override: Option<(String, String)>) -> crate::menu_ipc::MenuState {
        let p = app.prefs.borrow();
        let comeback = app.engine.setting("learning.comeback".into()).ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(2);
        let front = front_override.or_else(|| app.typing_in.borrow().clone()).map(|(b, n)| {
            let q = quiet(app, &b);
            (b, n, q)
        });
        crate::menu_ipc::MenuState {
            paused: app.paused.get(),
            shown: p.shown,
            readings: p.readings,
            selection: p.selection,
            copy_click: p.copy_click,
            speak: p.speak,
            words_on_screen: p.words_on_screen,
            words_comeback: comeback,
            words_break: p.words_break,
            words_langs: p.words_langs,
            words_typing: p.words_typing,
            words_video: p.words_video,
            words_hide_shots: p.words_hide_shots,
            lang_count: app.langs.len(),
            chat_swap: p.chat_swap,
            front,
            missing_flags: missing_flags(app),
            login_on: login_on(),
        }
    }

    /// Returns Ok(true) when `quit` was requested so IPC can flush its reply first.
    fn apply_action(mtm: MainThreadMarker, app: &App, id: &str, arg: Option<&str>) -> Result<bool, String> {
        match id {
            "pause" => {
                let next = !app.paused.get();
                app.paused.set(next);
                app.ax_paused.store(next, Ordering::Relaxed);
                let (lock, cvar) = &*app.ax_signal;
                lock.lock().unwrap().dirty = true;
                cvar.notify_all();
            }
            "readings" => {
                let mut p = app.prefs.borrow_mut();
                p.readings = !p.readings;
                p.save(&dirs());
                app.preview.borrow_mut().redo();
            }
            "selection" => {
                let mut p = app.prefs.borrow_mut();
                p.selection = !p.selection;
                p.save(&dirs());
                app.preview.borrow_mut().redo();
            }
            "copy_click" => {
                let mut p = app.prefs.borrow_mut();
                p.copy_click = !p.copy_click;
                p.save(&dirs());
            }
            "speak" => {
                let mut p = app.prefs.borrow_mut();
                p.speak = !p.speak;
                crate::speak::set_on(p.speak);
                p.save(&dirs());
            }
            "words.typing" => {
                let mut p = app.prefs.borrow_mut();
                p.words_typing = !p.words_typing;
                p.save(&dirs());
            }
            "words.video" => {
                let mut p = app.prefs.borrow_mut();
                p.words_video = !p.words_video;
                p.save(&dirs());
            }
            "words.shots" => {
                let mut p = app.prefs.borrow_mut();
                p.words_hide_shots = !p.words_hide_shots;
                p.save(&dirs());
            }
            "chat_swap" => {
                let mut p = app.prefs.borrow_mut();
                p.chat_swap = !p.chat_swap;
                p.save(&dirs());
            }
            "off_in" => {
                let bundle = arg.map(str::to_string).or_else(|| app.typing_in.borrow().as_ref().map(|(b, _)| b.clone()));
                if let Some(bundle) = bundle {
                    let was_quiet = quiet(app, &bundle);
                    let mut p = app.prefs.borrow_mut();
                    p.off.retain(|b| *b != bundle);
                    p.on.retain(|b| *b != bundle);
                    let now_off = !was_quiet;
                    if now_off && !QUIET.contains(&bundle.as_str()) {
                        p.off.push(bundle);
                    } else if !now_off && QUIET.contains(&bundle.as_str()) {
                        p.on.push(bundle);
                    }
                    p.save(&dirs());
                    drop(p);
                    sync_front_app(app);
                }
            }
            "languages" => {
                let _ = std::process::Command::new("open").arg("x-apple.systempreferences:com.apple.Localization-Settings.extension").spawn();
            }
            "login" => set_login(!login_on()),
            "quit" => return Ok(true),
            _ if id.starts_with("shown=") => {
                let n: usize = id.trim_start_matches("shown=").parse().map_err(|_| format!("bad {id}"))?;
                app.prefs.borrow_mut().shown = n;
                app.prefs.borrow().save(&dirs());
                app.preview.borrow_mut().redo();
            }
            _ if id.starts_with("words.count=") => {
                let n: usize = id.trim_start_matches("words.count=").parse().map_err(|_| format!("bad {id}"))?;
                app.prefs.borrow_mut().words_on_screen = n;
                app.prefs.borrow().save(&dirs());
            }
            _ if id.starts_with("words.comeback=") => {
                let n: u64 = id.trim_start_matches("words.comeback=").parse().map_err(|_| format!("bad {id}"))?;
                let _ = app.engine.set_setting("learning.comeback".to_string(), n.to_string());
            }
            _ if id.starts_with("words.break=") => {
                let n: u64 = id.trim_start_matches("words.break=").parse().map_err(|_| format!("bad {id}"))?;
                app.prefs.borrow_mut().words_break = n;
                app.prefs.borrow().save(&dirs());
            }
            _ if id.starts_with("words.langs=") => {
                let n: usize = id.trim_start_matches("words.langs=").parse().map_err(|_| format!("bad {id}"))?;
                app.prefs.borrow_mut().words_langs = n;
                app.prefs.borrow().save(&dirs());
            }
            _ => return Err(format!("unknown action {id}")),
        }
        if app.fallback_tray.borrow().is_some() {
            rebuild_fallback_tray(mtm, app);
        }
        Ok(false)
    }

    fn append_nodes(menu: &Menu, nodes: &[crate::menu_ipc::Node], refs: &mut FallbackRefs) {
        for n in nodes {
            match n {
                crate::menu_ipc::Node::Action { id, title, .. } => {
                    let item = MenuItem::with_id(id.as_str(), title, true, None);
                    if id == "pause" {
                        refs.pause = Some((item.id().clone(), item.clone()));
                    } else if id == "languages" {
                        refs.languages = Some((item.id().clone(), item.clone()));
                    } else if id == "quit" {
                        refs.quit = Some(item.id().clone());
                    }
                    let _ = menu.append(&item);
                }
                crate::menu_ipc::Node::Check { id, title, checked, .. } | crate::menu_ipc::Node::Radio { id, title, checked, .. } => {
                    let item = CheckMenuItem::with_id(id.as_str(), title, true, *checked, None);
                    if id == "off_in" {
                        refs.off = Some((item.id().clone(), item.clone()));
                    } else if id == "login" {
                        refs.login = Some((item.id().clone(), item.clone()));
                    }
                    let _ = menu.append(&item);
                }
                crate::menu_ipc::Node::Submenu { title, items } => {
                    let sub = Submenu::new(title, true);
                    append_nodes_sub(&sub, items, refs);
                    let _ = menu.append(&sub);
                }
                crate::menu_ipc::Node::Sep => {
                    let _ = menu.append(&PredefinedMenuItem::separator());
                }
                crate::menu_ipc::Node::Info { title } => {
                    let _ = menu.append(&MenuItem::new(title, false, None));
                }
            }
        }
    }

    fn append_nodes_sub(sub: &Submenu, nodes: &[crate::menu_ipc::Node], refs: &mut FallbackRefs) {
        for n in nodes {
            match n {
                crate::menu_ipc::Node::Action { id, title, .. } => {
                    let _ = sub.append(&MenuItem::with_id(id.as_str(), title, true, None));
                }
                crate::menu_ipc::Node::Check { id, title, checked, .. } | crate::menu_ipc::Node::Radio { id, title, checked, .. } => {
                    let _ = sub.append(&CheckMenuItem::with_id(id.as_str(), title, true, *checked, None));
                }
                crate::menu_ipc::Node::Submenu { title, items } => {
                    let child = Submenu::new(title, true);
                    append_nodes_sub(&child, items, refs);
                    let _ = sub.append(&child);
                }
                crate::menu_ipc::Node::Sep => {
                    let _ = sub.append(&PredefinedMenuItem::separator());
                }
                crate::menu_ipc::Node::Info { title } => {
                    let _ = sub.append(&MenuItem::new(title, false, None));
                }
            }
        }
    }

    #[derive(Default)]
    struct FallbackRefs {
        pause: Option<(tray_icon::menu::MenuId, MenuItem)>,
        off: Option<(tray_icon::menu::MenuId, CheckMenuItem)>,
        languages: Option<(tray_icon::menu::MenuId, MenuItem)>,
        login: Option<(tray_icon::menu::MenuId, CheckMenuItem)>,
        quit: Option<tray_icon::menu::MenuId>,
    }

    fn build_fallback_tray(mtm: MainThreadMarker, app: &App) -> Option<FallbackTray> {
        let state = menu_state(app, None);
        let has_front = state.front.is_some();
        let nodes = crate::menu_ipc::build_menu(&state);
        let menu = Menu::new();
        let mut refs = FallbackRefs::default();
        append_nodes(&menu, &nodes, &mut refs);
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("ccez-keys")
            .build()
            .ok()?;
        if let Some(item) = tray.ns_status_item()
            && let Some(button) = item.button(mtm)
        {
            button.setImage(Some(&menu_bar_image()));
        }
        let _ = tray.set_tooltip(Some("ccez-keys"));
        let (pause_id, pause) = refs.pause?;
        let (lang_id, languages) = refs.languages?;
        let (login_id, login) = refs.login?;
        let quit_id = refs.quit?;
        let (off_id, off_here) = refs.off.unwrap_or_else(|| {
            let dummy = CheckMenuItem::with_id("off_in", "Off in this app", false, false, None);
            (dummy.id().clone(), dummy)
        });
        Some(FallbackTray {
            _tray: tray,
            pause_id,
            pause,
            off_id,
            off_here,
            lang_id,
            languages,
            login_id,
            login,
            quit_id,
            front: Cell::new(has_front),
        })
    }

    fn rebuild_fallback_tray(mtm: MainThreadMarker, app: &App) {
        *app.fallback_tray.borrow_mut() = None;
        *app.fallback_tray.borrow_mut() = build_fallback_tray(mtm, app);
    }

    fn sync_fallback_tray(mtm: MainThreadMarker, app: &App) {
        let clip_running = !NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str("uk.ccez.clip")).is_empty();
        if clip_running {
            if app.fallback_tray.borrow().is_some() {
                *app.fallback_tray.borrow_mut() = None;
                log("info", "fallback tray icon hidden: ccez-clip is running");
            }
        } else if app.fallback_tray.borrow().is_none() {
            *app.fallback_tray.borrow_mut() = build_fallback_tray(mtm, app);
            log("info", "fallback tray icon shown: ccez-clip is not running");
        }
    }

    objc2::define_class!(
        /// Key-value observer on NSWorkspace.runningApplications: unlike the
        /// launch/terminate notifications it also fires for accessory
        /// (LSUIElement) apps such as ccez-clip.
        #[unsafe(super(objc2_foundation::NSObject))]
        #[thread_kind = objc2::MainThreadOnly]
        #[name = "CcezKeysRunningWatcher"]
        struct RunningWatcher;

        impl RunningWatcher {
            #[unsafe(method(observeValueForKeyPath:ofObject:change:context:))]
            fn observe(
                &self,
                _key_path: Option<&NSString>,
                _object: Option<&objc2::runtime::AnyObject>,
                _change: Option<&objc2::runtime::AnyObject>,
                _context: *mut std::ffi::c_void,
            ) {
                // KVO may fire off the main thread: hop to it.
                unsafe extern "C" {
                    static _dispatch_main_q: u8;
                    fn dispatch_async_f(queue: *const u8, context: *mut std::ffi::c_void, work: extern "C" fn(*mut std::ffi::c_void));
                }
                extern "C" fn on_main(_: *mut std::ffi::c_void) {
                    if let Some(mtm) = MainThreadMarker::new()
                        && let Some(app) = APP_REF.with(|c| c.get())
                    {
                        autoreleasepool(|_| sync_fallback_tray(mtm, app));
                    }
                }
                unsafe { dispatch_async_f(&_dispatch_main_q as *const u8, std::ptr::null_mut(), on_main) };
            }
        }
    );

    /// Re-check the fallback icon whenever any app starts or quits.
    fn watch_running_apps(mtm: MainThreadMarker) {
        use objc2::rc::Retained;
        use objc2::{msg_send, runtime::AnyObject};
        let watcher: Retained<RunningWatcher> = unsafe { msg_send![mtm.alloc::<RunningWatcher>(), init] };
        let ws = NSWorkspace::sharedWorkspace();
        let key = NSString::from_str("runningApplications");
        // Options 0: the callback re-reads the list itself.
        unsafe {
            let _: () = msg_send![&*ws, addObserver: &*watcher as &AnyObject, forKeyPath: &*key, options: 0usize, context: std::ptr::null_mut::<std::ffi::c_void>()];
        }
        std::mem::forget(watcher);
    }

    fn needs_ui_timer(app: &App) -> bool {
        (app.awake.load(Ordering::Relaxed) && !app.paused.get() && app.prefs.borrow().words_on_screen > 0)
            || app.floaters.has_activity()
            || app.view.is_shown()
            || app.view.card_open.get()
            || app.preview.borrow().is_waiting()
    }

    fn ensure_ui_timer(app: &'static App, need: bool) {
        let cur = app.ui_timer.take();
        if need {
            if let Some(t) = cur {
                app.ui_timer.set(Some(t));
                return;
            }
            let cb = RcBlock::new(move |_t: NonNull<NSTimer>| {
                if let Some(mtm) = MainThreadMarker::new() {
                    autoreleasepool(|_| drain_and_tick(mtm, app));
                }
            });
            let timer = unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(0.15, true, &cb) };
            app.ui_timer.set(Some(timer));
        } else if let Some(t) = cur {
            t.invalidate();
        }
    }

    fn drain_and_tick(mtm: MainThreadMarker, app: &'static App) {
        // 1. IPC menu/action requests from ccez-clip.
        let jobs: Vec<crate::menu_ipc::Job> = std::mem::take(&mut *app.ipc_queue.lock().unwrap());
        for job in jobs {
            let reply = match job.cmd.as_str() {
                "menu" => {
                    let front = job
                        .front
                        .filter(|b| !b.is_empty() && b != "uk.ccez.clip" && b != "uk.ccez.keys.mac")
                        .map(|b| (b.clone(), job.front_name.unwrap_or(b)));
                    let state = menu_state(app, front);
                    let items: Vec<serde_json::Value> = crate::menu_ipc::build_menu(&state).iter().map(crate::menu_ipc::Node::to_json).collect();
                    serde_json::json!({ "v": crate::menu_ipc::PROTOCOL_VERSION, "ok": true, "items": items })
                }
                "act" => match apply_action(mtm, app, &job.id, job.arg.as_deref()) {
                    Ok(quit) => {
                        let ok = serde_json::json!({ "v": crate::menu_ipc::PROTOCOL_VERSION, "ok": true });
                        let _ = job.reply.send(ok);
                        if quit {
                            let cb = RcBlock::new(|_t: NonNull<NSTimer>| std::process::exit(0));
                            unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(0.05, false, &cb) };
                        }
                        continue;
                    }
                    Err(e) => serde_json::json!({ "v": crate::menu_ipc::PROTOCOL_VERSION, "ok": false, "error": e }),
                },
                other => serde_json::json!({ "v": crate::menu_ipc::PROTOCOL_VERSION, "ok": false, "error": format!("unknown cmd {other:?}") }),
            };
            let _ = job.reply.send(reply);
        }

        // 2. Fallback tray menu events (only when ccez-clip isn't running).
        if app.fallback_tray.borrow().is_some() {
            let mut acted = false;
            let events: Vec<String> = std::mem::take(&mut *app.tray_events.lock().unwrap());
            for id in events {
                match apply_action(mtm, app, &id, None) {
                    Ok(true) => std::process::exit(0),
                    Ok(false) => acted = true,
                    Err(_) => {}
                }
            }
            if !acted {
                let front = app.typing_in.borrow().as_ref().map(|(b, n)| (b.clone(), n.clone(), quiet(app, b)));
                let missing = missing_flags(app);
                let front_changed = *app.last_tray_front.borrow() != front;
                let missing_changed = *app.last_missing.borrow() != missing;
                if front_changed || missing_changed {
                    let has_front = app.fallback_tray.borrow().as_ref().is_some_and(|t| t.front.get());
                    if front_changed && front.is_some() != has_front {
                        rebuild_fallback_tray(mtm, app);
                    } else if let Some(t) = app.fallback_tray.borrow().as_ref() {
                        if let Some((ref _b, ref name, q)) = front {
                            t.off_here.set_text(format!("Off in {name}"));
                            t.off_here.set_checked(q);
                        }
                        let title = if missing.is_empty() {
                            "ⓘ Translation languages…".to_string()
                        } else {
                            format!("ⓘ Translation languages… ({} missing)", missing.join(" "))
                        };
                        t.languages.set_text(title);
                        let _ = (&t.pause_id, &t.pause, &t.off_id, &t.lang_id, &t.login_id, &t.login, &t.quit_id);
                    }
                    *app.last_tray_front.borrow_mut() = front;
                    *app.last_missing.borrow_mut() = missing;
                }
            }
        }

        // 3. Preview + floating words tick.
        let field = app.ax_field.lock().unwrap().clone();
        tick(mtm, app, field);
        ensure_ui_timer(app, needs_ui_timer(app));
    }

    fn start_observers(app: &'static App) {
        let nc = NSWorkspace::sharedWorkspace().notificationCenter();
        let observe_ws = |name: &NSNotificationName, f: Box<dyn Fn(MainThreadMarker, &NSNotification)>| {
            let block = RcBlock::new(move |n: NonNull<NSNotification>| {
                if let Some(mtm) = MainThreadMarker::new() {
                    autoreleasepool(|_| f(mtm, unsafe { n.as_ref() }));
                }
            });
            let tok = unsafe { nc.addObserverForName_object_queue_usingBlock(Some(name), None, None, &block) };
            std::mem::forget((tok, block));
        };

        observe_ws(
            unsafe { NSWorkspaceDidActivateApplicationNotification },
            Box::new(move |mtm, _n| {
                sync_front_app(app);
                drain_and_tick(mtm, app);
            }),
        );

        let bundle_of = |n: &NSNotification| -> Option<String> {
            n.userInfo()?
                .objectForKey(unsafe { NSWorkspaceApplicationKey })?
                .downcast::<NSRunningApplication>()
                .ok()?
                .bundleIdentifier()
                .map(|b| b.to_string())
        };

        observe_ws(
            unsafe { NSWorkspaceDidLaunchApplicationNotification },
            Box::new(move |mtm, n| {
                let Some(b) = bundle_of(n) else { return };
                if b == "com.apple.screencaptureui" {
                    crate::float::set_capturing_ui(true);
                    drain_and_tick(mtm, app);
                } else if b == "uk.ccez.clip" {
                    sync_fallback_tray(mtm, app);
                }
            }),
        );

        observe_ws(
            unsafe { NSWorkspaceDidTerminateApplicationNotification },
            Box::new(move |mtm, n| {
                let Some(b) = bundle_of(n) else { return };
                if b == "com.apple.screencaptureui" {
                    crate::float::set_capturing_ui(false);
                    drain_and_tick(mtm, app);
                } else if b == "uk.ccez.clip" {
                    sync_fallback_tray(mtm, app);
                }
            }),
        );

        observe_ws(
            unsafe { NSWorkspaceScreensDidSleepNotification },
            Box::new(move |mtm, _n| {
                app.awake.store(false, Ordering::Relaxed);
                let (lock, cvar) = &*app.ax_signal;
                lock.lock().unwrap().dirty = true;
                cvar.notify_all();
                drain_and_tick(mtm, app);
            }),
        );

        observe_ws(
            unsafe { NSWorkspaceScreensDidWakeNotification },
            Box::new(move |mtm, _n| {
                app.awake.store(true, Ordering::Relaxed);
                let (lock, cvar) = &*app.ax_signal;
                lock.lock().unwrap().dirty = true;
                cvar.notify_all();
                drain_and_tick(mtm, app);
            }),
        );

        let dnc = NSDistributedNotificationCenter::defaultCenter();
        for (name, woke) in [("com.apple.screenIsLocked", false), ("com.apple.screenIsUnlocked", true)] {
            let n = NSString::from_str(name);
            let block = RcBlock::new(move |_n: NonNull<NSNotification>| {
                app.awake.store(woke, Ordering::Relaxed);
                let (lock, cvar) = &*app.ax_signal;
                lock.lock().unwrap().dirty = true;
                cvar.notify_all();
                if let Some(mtm) = MainThreadMarker::new() {
                    autoreleasepool(|_| drain_and_tick(mtm, app));
                }
            });
            let tok = unsafe { dnc.addObserverForName_object_queue_usingBlock(Some(&n), None, None, &block) };
            std::mem::forget((tok, block));
        }
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
