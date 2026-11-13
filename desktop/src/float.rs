//! Learning words on screen (owner, 2026-11-11: "all the time", "where the
//! screen isn't already occupied", "like Anki"): a few at a time, each in a
//! spot the front window has no content in and no other window covers,
//! faint until the pointer is over it, when its meaning shows. Anki's three
//! on the mouse: left click Good (back later, later each time), right click
//! Again (back in two minutes), middle click Easy (back
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
use crate::view::{fade_over, label, new_panel, rect};
use ccez_keys_core::review::{Grade, Snapshot};

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
/// A spot whose word was just clicked takes no new word this long (owner,
/// 2026-11-12: the next one came up right where the click was).
const RESTS: Duration = Duration::from_secs(120);

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

objc2::define_class!(
    /// Over a whole word's pill, on top and click-through: the pointing
    /// finger the moment the pointer is on it and on every move (the app
    /// underneath puts its own cursor back as the pointer moves), the arrow
    /// when it leaves. The one place a word sets the cursor, so two never
    /// take turns.
    #[unsafe(super(objc2_app_kit::NSView, objc2_app_kit::NSResponder, objc2_foundation::NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "CcezKeysFloatPad"]
    struct FloatPad;

    impl FloatPad {
        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _e: &NSEvent) {
            objc2_app_kit::NSCursor::pointingHandCursor().set();
        }
        #[unsafe(method(mouseMoved:))]
        fn mouse_moved(&self, _e: &NSEvent) {
            objc2_app_kit::NSCursor::pointingHandCursor().set();
        }
        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _e: &NSEvent) {
            objc2_app_kit::NSCursor::arrowCursor().set();
        }
        #[unsafe(method(hitTest:))]
        fn hit_test(&self, _p: objc2_foundation::NSPoint) -> *mut objc2_app_kit::NSView {
            std::ptr::null_mut()
        }
    }
);

fn pad(mtm: MainThreadMarker, frame: NSRect) -> Retained<objc2_app_kit::NSView> {
    use objc2::AllocAnyThread;
    use objc2_app_kit::{NSTrackingArea, NSTrackingAreaOptions};
    let p: Retained<FloatPad> = unsafe { objc2::msg_send![FloatPad::alloc(mtm), initWithFrame: frame] };
    let opts = NSTrackingAreaOptions::MouseEnteredAndExited | NSTrackingAreaOptions::MouseMoved | NSTrackingAreaOptions::ActiveAlways | NSTrackingAreaOptions::InVisibleRect;
    let area = unsafe { NSTrackingArea::initWithRect_options_owner_userInfo(NSTrackingArea::alloc(), p.bounds(), opts, Some(&p), None) };
    p.addTrackingArea(&area);
    Retained::into_super(p)
}

struct Word {
    id: isize,
    lang: String,
    word: String,
    /// The word as it shows ("parvenir à" for parvenir).
    shown: String,
    meaning: String,
    /// Small tags beside the meaning ("tr" for a transitive sense).
    tags: Vec<String>,
    /// Japanese and Chinese: its pieces with the furigana or pinyin over
    /// each ("" over kana), shown while the pointer is on it (owner,
    /// 2026-11-12). Empty for other languages.
    reading: Vec<(String, String)>,
    /// Its language's flag, small beside it (owner, 2026-11-13: Chinese
    /// and Japanese words looked alike).
    flag: String,
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

/// The last answer, kept a few seconds for Undo.
struct Answer {
    lang: String,
    word: String,
    shown: String,
    meaning: String,
    tags: Vec<String>,
    cell: Rect,
    before: Snapshot,
    known: bool,
    at: Instant,
}

/// How long Undo stays offered.
pub const UNDO_FOR: Duration = Duration::from_secs(4);

pub struct Floaters {
    words: RefCell<Vec<Word>>,
    /// Words fading out: since when, and whether the pointer landing on one
    /// brings it back (not after a click: that fade is the answer).
    leaving: RefCell<Vec<(Word, Instant, bool)>>,
    /// The engine: its schedule is the one the sync uses too.
    engine: std::sync::Arc<ccez_keys_core::Engine>,
    last_answer: RefCell<Option<Answer>>,
    next_id: Cell<isize>,
    seed: Cell<usize>,
    last: Cell<Instant>,
    /// Off the screen until then (a screenshot key was pressed).
    quiet_until: Cell<Option<Instant>>,
    /// The menu's "Hide in screenshots" (owner, 2026-11-11: off, to show them).
    pub hide_in_shots: Cell<bool>,
    /// The menu's "Readings over Japanese and Chinese".
    pub readings: Cell<bool>,
    /// Each language's flag (data/languages.json), set by the app.
    pub flags: RefCell<std::collections::HashMap<String, String>>,
    /// Spots whose word was clicked, and when: resting (RESTS).
    rested: RefCell<Vec<(Rect, Instant)>>,
    /// When the screenshot tools were last looked for, and whether they were up.
    capture_seen: Cell<(Instant, bool)>,
    /// From the spot thread: the free spots, and whether a video is playing
    /// (holding the display awake, or in the front window).
    spots: Arc<Mutex<Option<Vec<Rect>>>>,
    video: Arc<AtomicBool>,
    /// For the spot thread: the screen (top-left coordinates) and whether to look.
    screen: Arc<Mutex<Rect>>,
    looking: Arc<AtomicBool>,
}

impl Floaters {
    /// `find` gives the free spots on a screen and whether a video in the
    /// front window is playing; `video`, whether one holds the display awake.
    pub fn new(engine: std::sync::Arc<ccez_keys_core::Engine>, find: impl Fn(Rect) -> (Vec<Rect>, bool) + Send + 'static, video: impl Fn() -> bool + Send + 'static) -> Floaters {
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
                    let mut on = video();
                    let s = *screen.lock().unwrap();
                    if s.w > 0.0 {
                        let (free, in_window) = find(s);
                        on |= in_window;
                        *spots.lock().unwrap() = Some(free);
                    }
                    playing.store(on, Ordering::Relaxed);
                }
            });
        }
        Floaters {
            words: RefCell::default(),
            leaving: RefCell::default(),
            engine,
            last_answer: RefCell::default(),
            next_id: Cell::new(1),
            seed: Cell::new((std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) % 997) as usize),
            last: Cell::new(Instant::now()),
            quiet_until: Cell::new(None),
            hide_in_shots: Cell::new(true),
            readings: Cell::new(true),
            flags: RefCell::default(),
            rested: RefCell::default(),
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
    /// With `while_video` off they go while a video plays; with
    /// `while_typing` off none comes while keys are being typed.
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
            // Answered Again: its meaning shown a moment (longer while the
            // pointer stays), then it closes and the word stays on screen like
            // the others (owner, 2026-11-11: vanishing after a right click
            // was weird).
            if w.hold_until.is_some_and(|t| Instant::now() >= t) {
                w.hold_until = None;
                if !over {
                    w.open = false;
                    lay_out(mtm, w, primary_h);
                }
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
        self.rested.borrow_mut().retain(|(_, at)| at.elapsed() < RESTS);
        let rested: Vec<Rect> = self.rested.borrow().iter().map(|r| r.0).collect();
        // The pointer, in the spots' coordinates (top-left).
        let Some(cell) = next_spot(&free, &taken, &rested, (mouse.x, primary_h - mouse.y)) else { return };
        let up: Vec<(String, String)> = self.words.borrow().iter().map(|w| (w.lang.clone(), w.word.clone())).collect();
        let list = ccez_keys_core::review::mixed(words());
        // The first of a few that the dictionary can give a meaning to.
        let picks = self.engine.with_review(|r| r.next(&list, &up, 4, Self::now(), self.seed.get()));
        self.seed.set(self.seed.get() + 1);
        let Some((lang, word, (shown, m, tags))) = picks
            .into_iter()
            .map(|(l, w)| {
                let m = meaning(&l, &w);
                (l, w, m)
            })
            .find(|p| !p.2.1.is_empty())
        else {
            return;
        };
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        let reading = self.reading(&lang, &shown);
        let flag = self.flags.borrow().get(&lang).cloned().unwrap_or_default();
        let w = show(mtm, id, lang, word, shown, m, tags, reading, flag, cell, primary_h);
        if !self.hide_in_shots.get() {
            w.panel.setSharingType(objc2_app_kit::NSWindowSharingType::ReadOnly);
        }
        self.words.borrow_mut().push(w);
    }

    /// A Japanese or Chinese word's pieces with their readings (none for
    /// other languages, or with readings off in the menu).
    fn reading(&self, lang: &str, shown: &str) -> Vec<(String, String)> {
        if !self.readings.get() || !matches!(lang.split(['-', '_']).next(), Some("ja" | "zh")) {
            return Vec::new();
        }
        pieces(self.engine.strip_words(lang.to_string(), shown.to_string()).into_iter().flat_map(|w| w.ruby).map(|r| (r.text, r.rt)).collect(), shown)
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

    /// Undo of the last answer, within [UNDO_FOR]: its schedule as it was
    /// and the word back on screen. The word (lang, word) when the answer
    /// was Known, which the caller puts back to Learning.
    pub fn undo(&self, mtm: MainThreadMarker) -> Option<(String, String, bool)> {
        let a = self.last_answer.borrow_mut().take().filter(|a| a.at.elapsed() < UNDO_FOR + Duration::from_secs(1))?;
        self.engine.with_review(|r| r.restore(&a.lang, &a.word, a.before, Self::now()));
        let primary_h = NSScreen::screens(mtm).firstObject().map(|p| p.frame().size.height).unwrap_or(900.0);
        // Still up (Again shows its meaning a moment): it just stays.
        let up = self.words.borrow().iter().position(|w| w.lang == a.lang && w.word == a.word);
        if let Some(i) = up {
            let mut ws = self.words.borrow_mut();
            ws[i].hold_until = None;
            ws[i].came = Instant::now();
            ws[i].open = false;
            lay_out(mtm, &mut ws[i], primary_h);
        } else {
            let id = self.next_id.get();
            self.next_id.set(id + 1);
            let reading = self.reading(&a.lang, &a.shown);
            let flag = self.flags.borrow().get(&a.lang).cloned().unwrap_or_default();
            let w = show(mtm, id, a.lang.clone(), a.word.clone(), a.shown, a.meaning, a.tags, reading, flag, a.cell, primary_h);
            if !self.hide_in_shots.get() {
                w.panel.setSharingType(objc2_app_kit::NSWindowSharingType::ReadOnly);
            }
            self.words.borrow_mut().push(w);
        }
        Some((a.lang, a.word, a.known))
    }

    /// The word `id` as it shows, without its article ("das Urteil" →
    /// "Urteil"; "parvenir à" stays whole): what a click copies.
    pub fn copy_text(&self, id: isize) -> Option<String> {
        let ws = self.words.borrow();
        let w = ws.iter().find(|w| w.id == id)?;
        Some(without_article(&w.lang, &w.shown))
    }

    /// The word `id`'s language and the word as it shows, article and all
    /// ("das Urteil"): what a left click says aloud.
    pub fn spoken(&self, id: isize) -> Option<(String, String)> {
        let ws = self.words.borrow();
        let w = ws.iter().find(|w| w.id == id)?;
        Some((w.lang.clone(), w.shown.clone()))
    }

    /// A click on word `id`: 0 Good, 1 Again, 2 Easy, 3 Known. The answer
    /// (for a toast), and the word (lang, word) when Known, which the caller
    /// marks known everywhere.
    pub fn clicked(&self, mtm: MainThreadMarker, id: isize, kind: u8) -> Option<(String, Option<(String, String)>)> {
        let _ = mtm;
        let i = self.words.borrow().iter().position(|w| w.id == id)?;
        let (lang, word) = {
            let ws = self.words.borrow();
            (ws[i].lang.clone(), ws[i].word.clone())
        };
        self.rested.borrow_mut().push((self.words.borrow()[i].cell, Instant::now()));
        // Kept for Undo (owner, 2026-11-11: "just in case I misclicked").
        {
            let ws = self.words.borrow();
            let w = &ws[i];
            *self.last_answer.borrow_mut() = Some(Answer {
                lang: lang.clone(),
                word: word.clone(),
                shown: w.shown.clone(),
                meaning: w.meaning.clone(),
                tags: w.tags.clone(),
                cell: w.cell,
                before: self.engine.with_review(|r| r.snapshot(&lang, &word)),
                known: kind == 3,
                at: Instant::now(),
            });
        }
        match kind {
            1 => {
                // Fades out at once like the others, back in two minutes
                // (owner, 2026-11-11: "I expect it to fade out when I right
                // click it"; staying, then leaving after 2.5 s, both felt off).
                self.engine.with_review(|r| r.grade(&lang, &word, Grade::Again, Self::now()));
                let w = self.words.borrow_mut().remove(i);
                self.leave(w, false);
                Some(("Again".to_string(), None))
            }
            3 => {
                let w = self.words.borrow_mut().remove(i);
                self.leave(w, false);
                Some(("Known".to_string(), Some((lang, word))))
            }
            k => {
                self.engine.with_review(|r| r.grade(&lang, &word, if k == 2 { Grade::Easy } else { Grade::Good }, Self::now()));
                let w = self.words.borrow_mut().remove(i);
                self.leave(w, false);
                Some(((if k == 2 { "Easy" } else { "Good" }).to_string(), None))
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
#[allow(clippy::too_many_arguments)]
fn show(mtm: MainThreadMarker, id: isize, lang: String, word: String, shown: String, meaning: String, tags: Vec<String>, reading: Vec<(String, String)>, flag: String, cell: Rect, primary_h: f64) -> Word {
    let w = build(mtm, id, lang, word, shown, meaning, tags, reading, flag, cell, primary_h);
    w.on.set(true);
    w.panel.setAlphaValue(0.0);
    w.panel.orderFrontRegardless();
    fade_over(&w.panel, true, w.on.clone(), FADE_IN);
    w
}

/// A word made and laid out, not yet on screen.
#[allow(clippy::too_many_arguments)]
fn build(mtm: MainThreadMarker, id: isize, lang: String, word: String, shown: String, meaning: String, tags: Vec<String>, reading: Vec<(String, String)>, flag: String, cell: Rect, primary_h: f64) -> Word {
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
        reading,
        flag,
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
    w
}

/// For checks (`--word-picture`): a word as it looks with the pointer on
/// it, drawn into a PNG without ever being put on screen.
pub fn picture(mtm: MainThreadMarker, engine: &ccez_keys_core::Engine, lang: &str, word: &str, flag: &str, open: bool, out: &std::path::Path) -> bool {
    let (shown, meaning, tags) = engine.word_face(lang, word);
    let reading = pieces(engine.strip_words(lang.to_string(), shown.clone()).into_iter().flat_map(|w| w.ruby).map(|r| (r.text, r.rt)).collect(), &shown);
    let cell = Rect { x: 0.0, y: 0.0, w: 240.0, h: 175.0 };
    let mut w = build(mtm, 1, lang.to_string(), word.to_string(), shown, meaning, tags, reading, flag.to_string(), cell, 1080.0);
    w.open = open;
    lay_out(mtm, &mut w, 1080.0);
    let bounds = w.bg.bounds();
    let Some(rep) = w.bg.bitmapImageRepForCachingDisplayInRect(bounds) else { return false };
    w.bg.cacheDisplayInRect_toBitmapImageRep(bounds, &rep);
    let png = unsafe { rep.representationUsingType_properties(objc2_app_kit::NSBitmapImageFileType::PNG, &objc2_foundation::NSDictionary::new()) };
    png.is_some_and(|d| std::fs::write(out, d.to_vec()).is_ok())
}

/// The spot for a new word: the first free one that no word holds, that
/// isn't resting after a click, and that the pointer isn't in (a word never
/// comes up under the pointer). None: the word waits.
fn next_spot(free: &[Rect], taken: &[Rect], rested: &[Rect], pointer: (f64, f64)) -> Option<Rect> {
    let under = |c: &Rect| pointer.0 >= c.x && pointer.0 <= c.x + c.w && pointer.1 >= c.y && pointer.1 <= c.y + c.h;
    free.iter().find(|c| !taken.contains(c) && !rested.contains(c) && !under(c)).copied()
}

/// Each language's colour for its words on screen (sRGB, readable on the
/// dark pill): the owner's languages far apart (Chinese red, Japanese
/// violet), the rest spread round the wheel by their code.
pub fn lang_color(lang: &str) -> (f64, f64, f64) {
    match lang.split(['-', '_']).next().unwrap_or(lang) {
        "fr" => (0.50, 0.66, 1.00),
        "de" => (0.96, 0.77, 0.32),
        "es" => (1.00, 0.58, 0.36),
        "zh" => (1.00, 0.42, 0.42),
        "ja" => (0.80, 0.55, 1.00),
        "ko" => (0.31, 0.84, 0.78),
        "pt" => (0.48, 0.85, 0.56),
        "it" => (0.98, 0.55, 0.75),
        other => {
            // A hue from the code, at the same lightness as the rest.
            let h = other.bytes().fold(0u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32)) % 360;
            let (h, s, l) = (h as f64 / 60.0, 0.75, 0.68);
            let c = (1.0 - (2.0 * l - 1.0f64).abs()) * s;
            let x = c * (1.0 - (h % 2.0 - 1.0).abs());
            let (r, g, b) = match h as u32 {
                0 => (c, x, 0.0),
                1 => (x, c, 0.0),
                2 => (0.0, c, x),
                3 => (0.0, x, c),
                4 => (x, 0.0, c),
                _ => (c, 0.0, x),
            };
            let m = l - c / 2.0;
            (r + m, g + m, b + m)
        }
    }
}

#[test]
fn chinese_and_japanese_words_are_far_apart_in_colour() {
    let d = |a: &str, b: &str| {
        let (x, y) = (lang_color(a), lang_color(b));
        (x.0 - y.0).abs() + (x.1 - y.1).abs() + (x.2 - y.2).abs()
    };
    assert!(d("zh", "ja") > 0.4);
    assert!(d("fr", "de") > 0.4);
    // Any language gets a colour in range.
    let (r, g, b) = lang_color("hy");
    assert!([r, g, b].iter().all(|v| (0.0..=1.0).contains(v)));
}

/// A word's pieces as the dictionary cut them, when they have a reading and
/// spell the word as shown (else none: nothing goes over the wrong letters).
fn pieces(ruby: Vec<(String, String)>, shown: &str) -> Vec<(String, String)> {
    let whole: String = ruby.iter().map(|r| r.0.as_str()).collect();
    if whole != shown || ruby.iter().all(|r| r.1.is_empty()) { Vec::new() } else { ruby }
}

/// Readings wider than their pieces go over the whole word as one line:
/// Japanese run together, kana standing for itself ("たべる" over 食べる);
/// pinyin a syllable apart ("zhuāng shì" over 装饰).
fn reading_line(lang: &str, reading: &[(String, String)]) -> String {
    let sep = if lang.starts_with("zh") { " " } else { "" };
    reading.iter().map(|(t, r)| if r.is_empty() { t.as_str() } else { r.as_str() }).collect::<Vec<_>>().join(sep)
}

/// The size of furigana and pinyin over a screen word.
const READING: f64 = 11.0;

/// The word (and its meaning when open) centred in its spot.
fn lay_out(mtm: MainThreadMarker, w: &mut Word, primary_h: f64) {
    use objc2_foundation::{NSAttributedString, NSDictionary};
    let Some(content) = w.bg.contentView() else { return };
    for v in content.subviews().iter() {
        v.removeFromSuperview();
    }
    let font = NSFont::boldSystemFontOfSize(20.0);
    // The word in its language's colour (owner, 2026-11-13: Chinese and
    // Japanese looked alike; colour, not flags).
    let (r, g, b) = lang_color(&w.lang);
    let color = NSColor::colorWithSRGBRed_green_blue_alpha(r, g, b, if w.open { 1.0 } else { 0.75 });
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
    // The meaning wraps within the word's spot (owner, 2026-11-12: a long
    // one was cut off at the screen's edge), centred under the word.
    let m = (w.open && !w.meaning.is_empty()).then(|| wrapped(mtm, &w.meaning, 13.0, (w.cell.w - 24.0).max(120.0)));
    // Its tags as small pills beside the word (owner, 2026-11-11), the word
    // kept in the middle so it doesn't move when they come.
    let pills: Vec<(Retained<NSBox>, f64, f64)> = if w.open { w.tags.iter().map(|t| pill(mtm, t)).collect() } else { Vec::new() };
    let pills_w: f64 = pills.iter().map(|(_, pw, _)| pw + 4.0).sum();
    let (mw, mh) = m.as_ref().map(|l| (l.frame().size.width.min(w.cell.w - 24.0), l.frame().size.height)).unwrap_or((0.0, 0.0));
    // Japanese and Chinese, open: furigana or pinyin over each piece where
    // every one fits over its own, else one line over the whole word.
    let mut over: Vec<(Retained<NSTextField>, f64)> = Vec::new();
    if w.open && !w.reading.is_empty() {
        let width_of = |t: &str| objc2_app_kit::NSAttributedStringNSStringDrawing::size(&*unsafe { NSAttributedString::new_with_attributes(&NSString::from_str(t), &attrs) }).width;
        let mut before = String::new();
        let mut fits = true;
        for (t, r) in &w.reading {
            let (x0, tw) = (width_of(&before), width_of(t));
            before.push_str(t);
            if r.is_empty() {
                continue;
            }
            let l = label(mtm, r, READING, true);
            let lw = l.frame().size.width;
            fits &= lw <= tw + 4.0;
            // Its middle, from the word's left edge.
            over.push((l, x0 + tw / 2.0 + 1.0 - lw / 2.0));
        }
        if !fits {
            let l = label(mtm, &reading_line(&w.lang, &w.reading), READING, true);
            let lw = l.frame().size.width;
            over = vec![(l, ww / 2.0 - lw / 2.0)];
        }
    }
    let rh = over.first().map(|(l, _)| l.frame().size.height - 3.0).unwrap_or(0.0);
    let rw = over.iter().map(|(l, x)| 2.0 * (ww / 2.0 - x).abs().max((x + l.frame().size.width - ww / 2.0).abs())).fold(0.0, f64::max);
    let width = (ww + 2.0 * pills_w).max(mw).max(rw) + 20.0;
    let height = wh + mh + rh + 10.0;
    let wx = (width - ww) / 2.0;
    w.button.setFrame(rect(wx, mh + 5.0, ww, wh));
    content.addSubview(&w.button);
    for (l, x) in &over {
        let f = l.frame().size;
        l.setFrame(rect(wx + x, mh + 5.0 + wh - 3.0, f.width, f.height));
        content.addSubview(l);
    }
    let mut x = wx + ww + 4.0;
    for (p, pw, ph) in &pills {
        p.setFrame(rect(x, mh + 5.0 + (wh - ph) / 2.0, *pw, *ph));
        content.addSubview(p);
        x += pw + 4.0;
    }
    if let Some(m) = &m {
        m.setFrame(rect((width - mw) / 2.0, 4.0, mw, mh));
        content.addSubview(m);
    }
    w.meaning_label = m;
    // Its spot's middle, in Cocoa's coordinates; the top stays where it first
    // was, and a reading opens above it: the word never moves.
    let cx = w.cell.x + w.cell.w / 2.0;
    let top = *w.top.get_or_insert(primary_h - (w.cell.y + w.cell.h / 2.0) + (height - rh) / 2.0) + rh;
    let frame: NSRect = rect(cx - width / 2.0, top - height, width, height);
    w.panel.setFrame_display(frame, true);
    w.bg.setFrame(rect(0.0, 0.0, width, height));
    content.addSubview(&pad(mtm, content.bounds()));
    w.bg.setCornerRadius(height.min(28.0) / 2.0);
}

/// A word as shown without its article: "das Urteil" → "Urteil", "l'ardoise"
/// → "ardoise"; "parvenir à" and a word that is itself an article ("la")
/// stay whole.
fn without_article(lang: &str, shown: &str) -> String {
    let base = lang.split(['-', '_']).next().unwrap_or(lang);
    let articles: &[&str] = match base {
        "fr" => &["le ", "la ", "les ", "l'", "l’", "un ", "une "],
        "de" => &["der ", "die ", "das "],
        "es" => &["el ", "la ", "los ", "las ", "un ", "una "],
        "it" => &["il ", "lo ", "la ", "i ", "gli ", "le ", "l'", "l’", "un ", "una ", "uno ", "un'"],
        "pt" => &["o ", "a ", "os ", "as ", "um ", "uma "],
        _ => &[],
    };
    let bare = articles.iter().find_map(|a| shown.strip_prefix(a)).filter(|r| !r.trim().is_empty());
    bare.unwrap_or(shown).trim().to_string()
}

#[cfg(test)]
#[test]
fn a_new_word_never_comes_where_one_was_just_clicked_or_under_the_pointer() {
    let cell = |i: usize| Rect { x: 240.0 * i as f64, y: 0.0, w: 240.0, h: 175.0 };
    let free = [cell(0), cell(1), cell(2), cell(3)];
    let away = (5000.0, 5000.0);
    assert_eq!(next_spot(&free, &[], &[], away), Some(cell(0)));
    // The first spot's word was clicked (the pointer is still there): the next free one.
    assert_eq!(next_spot(&free, &[], &[cell(0)], (100.0, 80.0)), Some(cell(1)));
    // The pointer moved on to the second: neither.
    assert_eq!(next_spot(&free, &[], &[cell(0)], (300.0, 80.0)), Some(cell(2)));
    // Nowhere else: the word waits.
    assert_eq!(next_spot(&free[..1], &[], &[cell(0)], away), None);
    assert_eq!(next_spot(&free[..2], &[cell(1)], &[cell(0)], away), None);
}

#[test]
fn readings_go_over_the_word_shown() {
    let r = |v: &[(&str, &str)]| v.iter().map(|(t, r)| (t.to_string(), r.to_string())).collect::<Vec<_>>();
    // Furigana over the kanji, kana standing for itself in the one line.
    let taberu = r(&[("食", "た"), ("べる", "")]);
    assert_eq!(pieces(taberu.clone(), "食べる"), taberu);
    assert_eq!(reading_line("ja", &taberu), "たべる");
    assert_eq!(reading_line("zh", &r(&[("装", "zhuāng"), ("饰", "shì")])), "zhuāng shì");
    // Kana alone has nothing to show; pieces that spell another word: none.
    assert!(pieces(r(&[("こんにちは", "")]), "こんにちは").is_empty());
    assert!(pieces(r(&[("出馬", "しゅつば")]), "出馬する").is_empty());
}

#[test]
fn a_click_copies_the_word_without_its_article() {
    assert_eq!(without_article("de", "das Urteil"), "Urteil");
    assert_eq!(without_article("fr", "une ardoise"), "ardoise");
    assert_eq!(without_article("fr", "parvenir à"), "parvenir à");
    assert_eq!(without_article("fr", "la"), "la");
    assert_eq!(without_article("ja", "議席"), "議席");
}

/// A grey label that wraps at `max` points wide, sized to its text.
fn wrapped(mtm: MainThreadMarker, text: &str, size: f64, max: f64) -> Retained<NSTextField> {
    let l = NSTextField::wrappingLabelWithString(&NSString::from_str(text), mtm);
    l.setFont(Some(&NSFont::systemFontOfSize(size)));
    l.setTextColor(Some(&crate::view::rgb(0.74, 0.74, 0.74)));
    l.setAlignment(objc2_app_kit::NSTextAlignment::Center);
    l.setPreferredMaxLayoutWidth(max);
    let fit = l.fittingSize();
    l.setFrameSize(objc2_foundation::NSSize::new(fit.width.min(max).ceil(), fit.height.ceil()));
    l
}
