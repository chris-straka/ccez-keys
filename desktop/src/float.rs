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
use crate::review::{Grade, Review};
use crate::view::{fade, label, new_panel, rect, rgb};

/// How long a word stays when left alone.
const STAYS: Duration = Duration::from_secs(90);
/// Spots looked for this often (and a new word may come).
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
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _e: &NSEvent) {
            fire(self.tag(), 0);
        }
        #[unsafe(method(rightMouseDown:))]
        fn right_mouse_down(&self, _e: &NSEvent) {
            fire(self.tag(), 1);
        }
        #[unsafe(method(otherMouseDown:))]
        fn other_mouse_down(&self, _e: &NSEvent) {
            fire(self.tag(), 2);
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
    meaning: String,
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
    hold_until: Option<Instant>,
}

pub struct Floaters {
    words: RefCell<Vec<Word>>,
    leaving: RefCell<Vec<(Retained<NSPanel>, Instant)>>,
    review: RefCell<Review>,
    next_id: Cell<isize>,
    seed: Cell<usize>,
    last: Cell<Instant>,
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
                loop {
                    std::thread::sleep(EVERY);
                    if !looking.load(Ordering::Relaxed) {
                        continue;
                    }
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
    /// word's short meaning.
    pub fn tick(&self, mtm: MainThreadMarker, most: usize, words: impl FnOnce() -> Vec<(String, String)>, meaning: impl Fn(&str, &str) -> String) {
        let enabled = most > 0;
        self.looking.store(enabled, Ordering::Relaxed);
        // Words that faded out: off the screen.
        self.leaving.borrow_mut().retain(|(p, at)| {
            if at.elapsed() > Duration::from_millis(400) {
                p.orderOut(None);
                false
            } else {
                true
            }
        });
        if !enabled || self.video.load(Ordering::Relaxed) {
            self.clear();
            return;
        }
        let Some(main) = NSScreen::mainScreen(mtm) else { return };
        let vis = main.visibleFrame();
        let primary_h = NSScreen::screens(mtm).firstObject().map(|p| p.frame().size.height).unwrap_or(vis.size.height);
        *self.screen.lock().unwrap() = Rect { x: vis.origin.x, y: primary_h - (vis.origin.y + vis.size.height), w: vis.size.width, h: vis.size.height };
        let fresh = self.spots.lock().unwrap().take();
        let mouse = NSEvent::mouseLocation();
        // Hovered: its meaning shows; left: it closes again. Old or crowded out: it goes.
        let mut gone = Vec::new();
        for (i, w) in self.words.borrow_mut().iter_mut().enumerate() {
            let f = w.panel.frame();
            let over = mouse.x >= f.origin.x && mouse.x <= f.origin.x + f.size.width && mouse.y >= f.origin.y && mouse.y <= f.origin.y + f.size.height;
            let held = w.hold_until.is_some_and(|t| Instant::now() < t);
            let open = over || held;
            if open != w.open {
                w.open = open;
                lay_out(mtm, w, primary_h);
            }
            if w.hold_until.is_some_and(|t| Instant::now() >= t) {
                gone.push(i);
                continue;
            }
            let crowded = fresh.as_ref().is_some_and(|s| !s.contains(&w.cell));
            if (w.came.elapsed() > STAYS && !over) || crowded {
                gone.push(i);
            }
        }
        // Fewer asked for in the menu: the newest go.
        let over = self.words.borrow().len().saturating_sub(most);
        let n = self.words.borrow().len();
        gone.extend(n - over..n);
        gone.sort_unstable();
        gone.dedup();
        for i in gone.into_iter().rev() {
            let w = self.words.borrow_mut().remove(i);
            self.leave(w);
        }
        // A new word now and then, in a spot nobody holds.
        let Some(free) = fresh else { return };
        if self.last.get().elapsed() < EVERY || self.words.borrow().len() >= most {
            return;
        }
        self.last.set(Instant::now());
        let taken: Vec<Rect> = self.words.borrow().iter().map(|w| w.cell).collect();
        let Some(cell) = free.into_iter().find(|c| !taken.contains(c)) else { return };
        let up: Vec<(String, String)> = self.words.borrow().iter().map(|w| (w.lang.clone(), w.word.clone())).collect();
        let list = crate::review::mixed(words());
        let Some((lang, word)) = self.review.borrow().next(&list, &up, 1, Self::now(), self.seed.get()).into_iter().next() else { return };
        self.seed.set(self.seed.get() + 1);
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        let m = meaning(&lang, &word);
        self.words.borrow_mut().push(show(mtm, id, lang, word, m, cell, primary_h));
    }

    fn leave(&self, w: Word) {
        w.on.set(false);
        fade(&w.panel, false, w.on.clone());
        self.leaving.borrow_mut().push((w.panel, Instant::now()));
    }

    /// All of them away (switched off, paused, a video playing).
    pub fn clear(&self) {
        let all: Vec<Word> = self.words.borrow_mut().drain(..).collect();
        for w in all {
            self.leave(w);
        }
    }

    /// A click on word `id`: 0 Good, 1 Again, 2 Easy.
    pub fn clicked(&self, mtm: MainThreadMarker, id: isize, kind: u8) -> Option<()> {
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
                Some(())
            }
            k => {
                self.review.borrow_mut().grade(&lang, &word, if k == 2 { Grade::Easy } else { Grade::Good }, Self::now());
                let w = self.words.borrow_mut().remove(i);
                self.leave(w);
                Some(())
            }
        }
    }
}

/// A word put up in `cell`, faint, its meaning hidden.
fn show(mtm: MainThreadMarker, id: isize, lang: String, word: String, meaning: String, cell: Rect, primary_h: f64) -> Word {
    let (panel, bg) = new_panel(mtm);
    panel.setLevel(20);
    panel.setHasShadow(false);
    bg.setFillColor(&NSColor::colorWithSRGBRed_green_blue_alpha(0.1, 0.1, 0.11, 0.72));
    let button: Retained<NSButton> = {
        let b: Retained<FloatWord> = unsafe {
            objc2::msg_send![<FloatWord as objc2::ClassType>::class(), buttonWithTitle: &*NSString::from_str(&word), target: Option::<&objc2::runtime::AnyObject>::None, action: Option::<objc2::runtime::Sel>::None]
        };
        Retained::into_super(b)
    };
    button.setBordered(false);
    button.setTag(id);
    let mut w = Word { id, lang, word, meaning, cell, panel, bg, button, meaning_label: None, on: Default::default(), came: Instant::now(), open: false, hold_until: None, top: None };
    lay_out(mtm, &mut w, primary_h);
    w.on.set(true);
    w.panel.setAlphaValue(0.0);
    w.panel.orderFrontRegardless();
    fade(&w.panel, true, w.on.clone());
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
    let title = unsafe { NSAttributedString::new_with_attributes(&NSString::from_str(&w.word), &attrs) };
    w.button.setAttributedTitle(&title);
    let ww = objc2_app_kit::NSAttributedStringNSStringDrawing::size(&*title).width.ceil() + 2.0;
    w.button.sizeToFit();
    let wh = w.button.frame().size.height;
    let m = (w.open && !w.meaning.is_empty()).then(|| label(mtm, &w.meaning, 13.0, true));
    let (mw, mh) = m.as_ref().map(|l| (l.frame().size.width.min(w.cell.w - 24.0), l.frame().size.height)).unwrap_or((0.0, 0.0));
    let width = ww.max(mw) + 20.0;
    let height = wh + mh + 10.0;
    w.button.setFrame(rect((width - ww) / 2.0, mh + 5.0, ww, wh));
    content.addSubview(&w.button);
    if let Some(m) = &m {
        m.setFrame(rect((width - mw) / 2.0, 4.0, mw, mh));
        content.addSubview(m);
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
