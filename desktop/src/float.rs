//! Learning words on screen (owner, 2026-11-11: "all the time", "where the
//! screen isn't already occupied", "like Anki"): a few at a time, each in a
//! spot the front window has no content in and no other window covers,
//! faint until the pointer is over it, when its meaning shows. Anki's three
//! on the mouse: left click Good (back later, later each time), right click
//! Again (its meaning a moment, back in two minutes), middle click Easy (back
//! much later). Each leaves after a while and another comes; one whose spot
//! fills up goes at once. Looking for spots reads the front window's layout,
//! so it runs on its own thread every few seconds.

use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSBox, NSButton, NSColor, NSEvent, NSFont, NSPanel, NSScreen, NSTextField};
use objc2_foundation::{NSRect, NSString};

use crate::model::Rect;
use crate::view::{fade_over, label, new_panel, rect, rgb};
use ccez_keys_core::review::{Grade, Review};

/// How long a word stays when left alone.
const STAYS: Duration = Duration::from_secs(90);
/// Kept off the screen this long after a screenshot key, and while the
/// screenshot tools run (owner, 2026-11-11: not in screenshots).
const SHOT_QUIET: Duration = Duration::from_secs(8);
/// Fading in this slowly, they don't pull the eye (owner, 2026-11-11).
const FADE_IN: f64 = 2.0;
const FADE_OUT: f64 = 0.8;
/// The front window's layout read again this often when nothing moved.
const WALK_EVERY: Duration = Duration::from_secs(2);
/// A new word may come this often.
const EVERY: Duration = Duration::from_secs(4);

thread_local! {
    /// What a click on a word does: (its index, 0 left / 1 right / 2 middle).
    pub static CLICK: RefCell<Option<Box<dyn Fn(isize, u8)>>> = const { RefCell::new(None) };
}

fn fire(tag: isize, kind: u8) {
    CLICK.with(|c| {
        if let Some(f) = c.borrow().as_ref() {
            f(tag, kind)
        }
    });
}

objc2::define_class!(
    /// A word on screen: takes the first click without the app coming
    /// forward, and tells the three buttons apart.
    #[unsafe(super(NSButton, objc2_app_kit::NSControl, objc2_app_kit::NSView, objc2_app_kit::NSResponder, objc2_foundation::NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "CcezKeysFloatWord"]
    pub struct FloatWord;

    impl FloatWord {
        // The pointing finger over a word (it takes clicks).
        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _e: &NSEvent) {
            objc2_app_kit::NSCursor::pointingHandCursor().set();
        }
        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _e: &NSEvent) {
            objc2_app_kit::NSCursor::arrowCursor().set();
        }
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _e: &NSEvent) {
            fire(self.tag(), 0);
        }
        #[unsafe(method(rightMouseDown:))]
        fn right_mouse_down(&self, _e: &NSEvent) {
            fire(self.tag(), 1);
        }
        #[unsafe(method(otherMouseDown:))]
        fn other_mouse_down(&self, e: &NSEvent) {
            // Shift + middle click: Known (owner, 2026-11-11); middle alone: Easy.
            let known = e.modifierFlags().contains(objc2_app_kit::NSEventModifierFlags::Shift);
            fire(self.tag(), if known { 3 } else { 2 });
        }
        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _e: Option<&NSEvent>) -> bool {
            true
        }
    }
);

struct Word {
    id: isize,
    lang: String,
    word: String,
    /// The word as it shows ("parvenir à" for parvenir).
    shown: String,
    meaning: String,
    /// Small tags beside the meaning ("tr" for a transitive sense).
    tags: Vec<String>,
    /// Its spot (top-left coordinates), as the spot search gives them.
    cell: Rect,
    panel: Retained<NSPanel>,
    bg: Retained<NSBox>,
    button: Retained<NSButton>,
    meaning_label: Option<Retained<NSTextField>>,
    on: std::rc::Rc<Cell<bool>>,
    came: Instant,
    /// The meaning shows (hovered, or after Again until this time).
    open: bool,
    /// Its top edge (Cocoa's coordinates), fixed once it's up: the meaning
    /// opens below the word without moving it (owner, 2026-11-11).
    top: Option<f64>,
    /// The pointer was on it last tick.
    pointed: bool,
    hold_until: Option<Instant>,
}

pub struct Floaters {
    words: RefCell<Vec<Word>>,
    /// Words fading out: since when, and whether the pointer landing on one
    /// brings it back (not after a click: that fade is the answer).
    leaving: RefCell<Vec<(Word, Instant, bool)>>,
    review: RefCell<Review>,
    next_id: Cell<isize>,
    seed: Cell<usize>,
    last: Cell<Instant>,
    /// Off the screen until then (a screenshot key was pressed).
    quiet_until: Cell<Option<Instant>>,
    /// The menu's "Hide in screenshots" (owner, 2026-11-11: off, to show them).
    pub hide_in_shots: Cell<bool>,
    /// When the screenshot tools were last looked for, and whether they were up.
    capture_seen: Cell<(Instant, bool)>,
    /// From the spot thread: the free spots, and whether a video holds the display.
    spots: Arc<Mutex<Option<Vec<Rect>>>>,
    video: Arc<AtomicBool>,
    /// For the spot thread: the screen (top-left coordinates) and whether to look.
    screen: Arc<Mutex<Rect>>,
    looking: Arc<AtomicBool>,
}

impl Floaters {
    pub fn new(dir: &std::path::Path, find: impl Fn(Rect) -> Vec<Rect> + Send + 'static, video: impl Fn() -> bool + Send + 'static) -> Floaters {
        let spots: Arc<Mutex<Option<Vec<Rect>>>> = Arc::default();
        let playing: Arc<AtomicBool> = Arc::default();
        let screen: Arc<Mutex<Rect>> = Arc::default();
        let looking: Arc<AtomicBool> = Arc::default();
        {
            let (spots, playing, screen, looking) = (spots.clone(), playing.clone(), screen.clone(), looking.clone());
            std::thread::spawn(move || {
                // Windows looked at every 0.3 s (cheap): the moment one moves,
                // opens or closes, or another app comes forward, the front
                // window's layout is read again; else every 2 s (its content
                // scrolls without the windows moving).
                let mut seen: Option<String> = None;
                let mut walked = std::time::Instant::now() - WALK_EVERY;
                loop {
                    std::thread::sleep(Duration::from_millis(300));
                    if !looking.load(Ordering::Relaxed) {
                        seen = None;
                        continue;
                    }
                    let front = objc2_app_kit::NSWorkspace::sharedWorkspace().frontmostApplication().map(|a| a.processIdentifier()).unwrap_or(0);
                    let now = format!("{front}:{:?}", crate::idle::windows(-1));
                    if seen.as_deref() == Some(now.as_str()) && walked.elapsed() < WALK_EVERY {
                        continue;
                    }
                    seen = Some(now);
                    walked = std::time::Instant::now();
                    playing.store(video(), Ordering::Relaxed);
                    let s = *screen.lock().unwrap();
                    if s.w > 0.0 {
                        *spots.lock().unwrap() = Some(find(s));
                    }
                }
            });
        }
        Floaters {
            words: RefCell::default(),
            leaving: RefCell::default(),
            review: RefCell::new(Review::load(dir)),
            next_id: Cell::new(1),
            seed: Cell::new((std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) % 997) as usize),
            last: Cell::new(Instant::now()),
            quiet_until: Cell::new(None),
            hide_in_shots: Cell::new(true),
            capture_seen: Cell::new((Instant::now(), false)),
            spots,
            video: playing,
            screen,
            looking,
        }
    }

    fn now() -> u64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
    }

    /// Every tick: words come, stay, open under the pointer, and go.
    /// `most` is how many may be up (the menu's count; 0: none, also when
    /// paused), `words` gives the learning words (lang, word) and `meaning` a
    /// word as shown and its short meaning.
    /// With `while_video` off they go while a video holds the display
    /// awake; with `while_typing` off none comes while keys are being typed.
    #[allow(clippy::too_many_arguments)]
    pub fn tick(
        &self,
        mtm: MainThreadMarker,
        most: usize,
        while_video: bool,
        while_typing: bool,
        words: impl FnOnce() -> Vec<(String, String)>,
        meaning: impl Fn(&str, &str) -> (String, String, Vec<String>),
    ) {
        let enabled = most > 0;
        self.looking.store(enabled, Ordering::Relaxed);
        // A word fading out with the pointer on it: back, lit (owner,
        // 2026-11-11: never fade under the pointer). The rest: off the screen.
        let mouse = NSEvent::mouseLocation();
        let under = |p: &NSPanel| {
            let f = p.frame();
            mouse.x >= f.origin.x && mouse.x <= f.origin.x + f.size.width && mouse.y >= f.origin.y && mouse.y <= f.origin.y + f.size.height
        };
        let mut back = Vec::new();
        self.leaving.borrow_mut().retain_mut(|(w, at, revive)| {
            if *revive && under(&w.panel) && w.panel.isVisible() {
                back.push(w.id);
                return true;
            }
            if at.elapsed() > Duration::from_secs_f64(FADE_OUT + 0.1) {
                w.panel.orderOut(None);
                return false;
            }
            true
        });
        for id in back {
            let i = self.leaving.borrow().iter().position(|(w, _, _)| w.id == id);
            if let Some(i) = i {
                let (mut w, _, _) = self.leaving.borrow_mut().remove(i);
                w.on.set(true);
                w.came = Instant::now();
                fade_over(&w.panel, true, w.on.clone(), 0.25);
                self.words.borrow_mut().push(w);
            }
        }
        let (at, up) = self.capture_seen.get();
        let up = if !self.hide_in_shots.get() {
            false
        } else if at.elapsed() > Duration::from_millis(600) {
            let now = capturing();
            self.capture_seen.set((Instant::now(), now));
            now
        } else {
            up
        };
        let quiet = self.hide_in_shots.get() && (self.quiet_until.get().is_some_and(|t| Instant::now() < t) || up);
        if quiet {
            self.hide_now();
            return;
        }
        if !enabled || (!while_video && self.video.load(Ordering::Relaxed)) {
            self.clear();
            return;
        }
        let Some(main) = NSScreen::mainScreen(mtm) else { return };
        let vis = main.visibleFrame();
        let primary_h = NSScreen::screens(mtm).firstObject().map(|p| p.frame().size.height).unwrap_or(vis.size.height);
        *self.screen.lock().unwrap() = Rect { x: vis.origin.x, y: primary_h - (vis.origin.y + vis.size.height), w: vis.size.width, h: vis.size.height };
        let fresh = self.spots.lock().unwrap().take();
        // Its spot taken (a window moved over it, the page changed): gone at
        // once, no fade (owner, 2026-11-11).
        if let Some(free) = fresh.as_ref() {
            let crowded: Vec<usize> = self.words.borrow().iter().enumerate().filter(|(_, w)| !free.contains(&w.cell)).map(|(i, _)| i).collect();
            for i in crowded.into_iter().rev() {
                let w = self.words.borrow_mut().remove(i);
                w.on.set(false);
                w.panel.orderOut(None);
            }
        }
        // Hovered: its meaning shows; left: it closes again. Old: it goes.
        let mut gone = Vec::new();
        for (i, w) in self.words.borrow_mut().iter_mut().enumerate() {
            let f = w.panel.frame();
            let over = mouse.x >= f.origin.x && mouse.x <= f.origin.x + f.size.width && mouse.y >= f.origin.y && mouse.y <= f.origin.y + f.size.height;
            let held = w.hold_until.is_some_and(|t| Instant::now() < t);
            // The pointing finger while over it, the arrow once it's left.
            if over {
                objc2_app_kit::NSCursor::pointingHandCursor().set();
            } else if w.pointed {
                objc2_app_kit::NSCursor::arrowCursor().set();
            }
            w.pointed = over;
            let open = over || held;
            if open != w.open {
                w.open = open;
                lay_out(mtm, w, primary_h);
            }
            // Answered Again: its meaning shown a moment, then it goes, pointer
            // or not (the answer wins; owner, 2026-11-11).
            if w.hold_until.is_some_and(|t| Instant::now() >= t) {
                gone.push((i, false));
                continue;
            }
            // Otherwise, under the pointer nothing makes it go.
            if over {
                continue;
            }
            if w.came.elapsed() > STAYS {
                gone.push((i, true));
            }
        }
        // Fewer asked for in the menu: the newest go.
        let over = self.words.borrow().len().saturating_sub(most);
        let n = self.words.borrow().len();
        gone.extend((n - over..n).map(|i| (i, false)));
        gone.sort_unstable_by_key(|g| g.0);
        gone.dedup_by_key(|g| g.0);
        for (i, revive) in gone.into_iter().rev() {
            let w = self.words.borrow_mut().remove(i);
            self.leave(w, revive);
        }
        // A new word now and then, in a spot nobody holds.
        let Some(free) = fresh else { return };
        if self.last.get().elapsed() < EVERY || self.words.borrow().len() >= most || (!while_typing && typing()) {
            return;
        }
        self.last.set(Instant::now());
        let taken: Vec<Rect> = self.words.borrow().iter().map(|w| w.cell).collect();
        let Some(cell) = free.into_iter().find(|c| !taken.contains(c)) else { return };
        let up: Vec<(String, String)> = self.words.borrow().iter().map(|w| (w.lang.clone(), w.word.clone())).collect();
        let list = ccez_keys_core::review::mixed(words());
        let Some((lang, word)) = self.review.borrow().next(&list, &up, 1, Self::now(), self.seed.get()).into_iter().next() else { return };
        self.seed.set(self.seed.get() + 1);
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        let (shown, m, tags) = meaning(&lang, &word);
        let w = show(mtm, id, lang, word, shown, m, tags, cell, primary_h);
        if !self.hide_in_shots.get() {
            w.panel.setSharingType(objc2_app_kit::NSWindowSharingType::ReadOnly);
        }
        self.words.borrow_mut().push(w);
    }

    fn leave(&self, w: Word, revive: bool) {
        w.on.set(false);
        fade_over(&w.panel, false, w.on.clone(), FADE_OUT);
        self.leaving.borrow_mut().push((w, Instant::now(), revive));
    }

    /// A screenshot key: every word off the screen at once, no fade, for a while.
    pub fn screenshot(&self) {
        if !self.hide_in_shots.get() {
            return;
        }
        self.quiet_until.set(Some(Instant::now() + SHOT_QUIET));
        self.hide_now();
    }

    fn hide_now(&self) {
        for w in self.words.borrow_mut().drain(..) {
            w.on.set(false);
            w.panel.orderOut(None);
        }
        for (w, _, _) in self.leaving.borrow_mut().drain(..) {
            w.panel.orderOut(None);
        }
    }

    /// All of them away (switched off, paused, a video playing).
    pub fn clear(&self) {
        let all: Vec<Word> = self.words.borrow_mut().drain(..).collect();
        for w in all {
            self.leave(w, false);
        }
    }

    /// What a click did, for its toast ("Good · back in 1 h"), from the
    /// word's new time.
    fn said(&self, what: &str, lang: &str, word: &str) -> String {
        let back = self.review.borrow().due_at(lang, word).map(|d| d.saturating_sub(Self::now()));
        match back {
            Some(s) => format!("{what} · back in {}", Review::wait_words(s)),
            None => what.to_string(),
        }
    }

    /// A click on word `id`: 0 Good, 1 Again, 2 Easy, 3 Known. What it did
    /// (for a toast), and the word (lang, word) when Known, which the caller
    /// marks known everywhere.
    pub fn clicked(&self, mtm: MainThreadMarker, id: isize, kind: u8) -> Option<(String, Option<(String, String)>)> {
        let primary_h = NSScreen::screens(mtm).firstObject().map(|p| p.frame().size.height).unwrap_or(900.0);
        let i = self.words.borrow().iter().position(|w| w.id == id)?;
        let (lang, word) = {
            let ws = self.words.borrow();
            (ws[i].lang.clone(), ws[i].word.clone())
        };
        match kind {
            1 => {
                self.review.borrow_mut().grade(&lang, &word, Grade::Again, Self::now());
                // Its meaning a moment, then away.
                let mut ws = self.words.borrow_mut();
                ws[i].hold_until = Some(Instant::now() + Duration::from_millis(2500));
                ws[i].open = true;
                lay_out(mtm, &mut ws[i], primary_h);
                drop(ws);
                Some((self.said("Again", &lang, &word), None))
            }
            3 => {
                let w = self.words.borrow_mut().remove(i);
                self.leave(w, false);
                Some((format!("Known: {word}"), Some((lang, word))))
            }
            k => {
                self.review.borrow_mut().grade(&lang, &word, if k == 2 { Grade::Easy } else { Grade::Good }, Self::now());
                let w = self.words.borrow_mut().remove(i);
                self.leave(w, false);
                Some((self.said(if k == 2 { "Easy" } else { "Good" }, &lang, &word), None))
            }
        }
    }
}

/// The screenshot tools are up: ⇧⌘5's panel, or `screencapture` picking an
/// area or a window (⇧⌘4).
fn capturing() -> bool {
    use objc2_app_kit::NSRunningApplication;
    if !NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str("com.apple.screencaptureui")).is_empty() {
        return true;
    }
    process_running("screencapture")
}

fn process_running(name: &str) -> bool {
    #[link(name = "proc", kind = "dylib")]
    unsafe extern "C" {
        fn proc_listallpids(buffer: *mut i32, size: i32) -> i32;
        fn proc_name(pid: i32, buffer: *mut u8, size: u32) -> i32;
    }
    let mut pids = vec![0i32; 4096];
    let n = unsafe { proc_listallpids(pids.as_mut_ptr(), (pids.len() * 4) as i32) };
    let mut buf = [0u8; 64];
    pids.iter().take(n.max(0) as usize).any(|&pid| {
        let len = unsafe { proc_name(pid, buf.as_mut_ptr(), buf.len() as u32) };
        len > 0 && &buf[..len as usize] == name.as_bytes()
    })
}

/// A tag as a small grey pill: (it, width, height).
fn pill(mtm: MainThreadMarker, tag: &str) -> (Retained<NSBox>, f64, f64) {
    let l = label(mtm, tag, 10.5, true);
    let (lw, lh) = (l.frame().size.width, l.frame().size.height);
    let (w, h) = (lw + 8.0, lh + 2.0);
    let b = NSBox::new(mtm);
    b.setBoxType(objc2_app_kit::NSBoxType::Custom);
    b.setTitlePosition(objc2_app_kit::NSTitlePosition::NoTitle);
    b.setBorderWidth(0.0);
    b.setCornerRadius(h / 2.0);
    b.setFillColor(&NSColor::colorWithSRGBRed_green_blue_alpha(1.0, 1.0, 1.0, 0.12));
    b.setContentViewMargins(objc2_foundation::NSSize::new(0.0, 0.0));
    if let Some(c) = b.contentView() {
        l.setFrame(rect(4.0, 1.0, lw, lh));
        c.addSubview(&l);
    }
    (b, w, h)
}

/// A key pressed in the last two seconds.
fn typing() -> bool {
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventSourceSecondsSinceLastEventType(state: i32, kind: u32) -> f64;
    }
    // Combined session state, key down.
    let since = unsafe { CGEventSourceSecondsSinceLastEventType(0, 10) };
    since < 2.0
}

/// A word put up in `cell`, faint, its meaning hidden.
#[allow(clippy::too_many_arguments)]
fn show(mtm: MainThreadMarker, id: isize, lang: String, word: String, shown: String, meaning: String, tags: Vec<String>, cell: Rect, primary_h: f64) -> Word {
    let (panel, bg) = new_panel(mtm);
    panel.setLevel(20);
    panel.setHasShadow(false);
    bg.setFillColor(&NSColor::colorWithSRGBRed_green_blue_alpha(0.1, 0.1, 0.11, 0.72));
    let button: Retained<NSButton> = {
        let b: Retained<FloatWord> = unsafe {
            objc2::msg_send![<FloatWord as objc2::ClassType>::class(), buttonWithTitle: &*NSString::from_str(&shown), target: Option::<&objc2::runtime::AnyObject>::None, action: Option::<objc2::runtime::Sel>::None]
        };
        Retained::into_super(b)
    };
    button.setBordered(false);
    button.setTag(id);
    // Never in a screenshot or a screen share (where macOS still honours it).
    panel.setSharingType(objc2_app_kit::NSWindowSharingType::None);
    let mut w = Word {
        id,
        lang,
        word,
        shown,
        meaning,
        tags,
        cell,
        panel,
        bg,
        button,
        meaning_label: None,
        on: Default::default(),
        came: Instant::now(),
        open: false,
        hold_until: None,
        top: None,
        pointed: false,
    };
    lay_out(mtm, &mut w, primary_h);
    w.on.set(true);
    w.panel.setAlphaValue(0.0);
    w.panel.orderFrontRegardless();
    fade_over(&w.panel, true, w.on.clone(), FADE_IN);
    w
}

/// The word (and its meaning when open) centred in its spot.
fn lay_out(mtm: MainThreadMarker, w: &mut Word, primary_h: f64) {
    use objc2_foundation::{NSAttributedString, NSDictionary};
    let Some(content) = w.bg.contentView() else { return };
    for v in content.subviews().iter() {
        v.removeFromSuperview();
    }
    let font = NSFont::boldSystemFontOfSize(20.0);
    let color = if w.open { rgb(0.98, 0.55, 0.45) } else { NSColor::colorWithSRGBRed_green_blue_alpha(0.98, 0.55, 0.45, 0.75) };
    let attrs = unsafe {
        NSDictionary::<NSString, objc2::runtime::AnyObject>::from_slices(
            &[objc2_app_kit::NSFontAttributeName, objc2_app_kit::NSForegroundColorAttributeName],
            &[&*font as &objc2::runtime::AnyObject, &*color as &objc2::runtime::AnyObject],
        )
    };
    let title = unsafe { NSAttributedString::new_with_attributes(&NSString::from_str(&w.shown), &attrs) };
    w.button.setAttributedTitle(&title);
    let ww = objc2_app_kit::NSAttributedStringNSStringDrawing::size(&*title).width.ceil() + 2.0;
    w.button.sizeToFit();
    let wh = w.button.frame().size.height;
    let m = (w.open && !w.meaning.is_empty()).then(|| label(mtm, &w.meaning, 13.0, true));
    // Its tags as small pills after the meaning.
    let pills: Vec<(Retained<NSBox>, f64, f64)> = if w.open { w.tags.iter().map(|t| pill(mtm, t)).collect() } else { Vec::new() };
    let pills_w: f64 = pills.iter().map(|(_, pw, _)| pw + 4.0).sum();
    let (mw, mh) = m.as_ref().map(|l| (l.frame().size.width.min(w.cell.w - 24.0 - pills_w), l.frame().size.height)).unwrap_or((0.0, 0.0));
    let mw_all = mw + pills_w;
    let width = ww.max(mw_all) + 20.0;
    let height = wh + mh + 10.0;
    w.button.setFrame(rect((width - ww) / 2.0, mh + 5.0, ww, wh));
    content.addSubview(&w.button);
    for t in w.button.trackingAreas().iter() {
        w.button.removeTrackingArea(&t);
    }
    crate::view::pointer(&w.button);
    let mut x = (width - mw_all) / 2.0;
    if let Some(m) = &m {
        m.setFrame(rect(x, 4.0, mw, mh));
        content.addSubview(m);
        x += mw + 4.0;
    }
    for (p, pw, ph) in &pills {
        p.setFrame(rect(x, 4.0 + (mh - ph) / 2.0, *pw, *ph));
        content.addSubview(p);
        x += pw + 4.0;
    }
    w.meaning_label = m;
    // Its spot's middle, in Cocoa's coordinates; the top stays where it first was.
    let cx = w.cell.x + w.cell.w / 2.0;
    let top = *w.top.get_or_insert(primary_h - (w.cell.y + w.cell.h / 2.0) + height / 2.0);
    let frame: NSRect = rect(cx - width / 2.0, top - height, width, height);
    w.panel.setFrame_display(frame, true);
    w.bg.setFrame(rect(0.0, 0.0, width, height));
    w.bg.setCornerRadius(height.min(28.0) / 2.0);
}
