//! The preview: a small dark panel under the text cursor (or at the foot of
//! the screen when the app doesn't say where its cursor is), never taking
//! focus or clicks from the app being typed in.

use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSBox, NSBoxType, NSButton, NSColor, NSFont, NSPanel, NSScreen, NSTextField, NSTitlePosition,
    NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

use crate::ax::{Place, Rect};

/// Widest the line may be, in points: longer, the start gives way to "…".
const MAX_W: f64 = 560.0;
const PAD_X: f64 = 12.0;
const PAD_Y: f64 = 7.0;

/// A word on the preview (or a space or mark between words).
#[derive(Clone, PartialEq, Debug)]
pub struct Tok {
    pub text: String,
    /// Furigana or pinyin to put over it ("" for none).
    pub reading: String,
    /// A word (clickable), not a space or a mark.
    pub word: bool,
    /// Its dictionary form, for its status and card.
    pub key: String,
    /// 0 new, 1 learning, 2 known.
    pub status: u8,
}

/// One language on the preview: its flag and its words.
#[derive(Clone, PartialEq, Debug)]
pub struct Row {
    pub lang: String,
    pub flag: String,
    pub toks: Vec<Tok>,
}

thread_local! {
    /// What a click on a word or a card button does (set by the app).
    pub static CLICK: std::cell::RefCell<Option<Box<dyn Fn(isize)>>> = const { std::cell::RefCell::new(None) };
}

objc2::define_class!(
    #[unsafe(super(objc2_foundation::NSObject))]
    #[thread_kind = objc2::MainThreadOnly]
    #[name = "CcezKeysClicker"]
    pub struct Clicker;

    impl Clicker {
        #[unsafe(method(clicked:))]
        fn clicked(&self, sender: &NSButton) {
            let tag = sender.tag();
            CLICK.with(|c| if let Some(f) = c.borrow().as_ref() { f(tag) });
        }
    }
);

pub struct View {
    panel: Retained<NSPanel>,
    bg: Retained<NSBox>,
    shown: std::cell::Cell<bool>,
    /// What's laid out now (rebuilt only when it changes).
    rows: std::cell::RefCell<Vec<Row>>,
    size: std::cell::Cell<(f64, f64)>,
    pub clicker: Retained<Clicker>,
    card: Retained<NSPanel>,
    card_bg: Retained<NSBox>,
    pub card_open: std::cell::Cell<bool>,
}

const GAP: f64 = 4.0;
const TEXT: f64 = 14.0;
/// Japanese and Chinese a size up, their readings clear (owner: hard to read).
const CJK: f64 = 16.0;
const READING: f64 = 10.5;

fn rect(x: f64, y: f64, w: f64, h: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
}

fn rgb(r: f64, g: f64, b: f64) -> Retained<NSColor> {
    NSColor::colorWithSRGBRed_green_blue_alpha(r, g, b, 1.0)
}

fn label(mtm: MainThreadMarker, text: &str, size: f64, grey: bool) -> Retained<NSTextField> {
    let l = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    l.setFont(Some(&NSFont::systemFontOfSize(size)));
    let c = if grey { 0.74 } else { 0.95 };
    l.setTextColor(Some(&rgb(c, c, c)));
    l.setUsesSingleLineMode(true);
    l.sizeToFit();
    l
}

/// A word as a borderless button: its colour says its status (learning tinted
/// as on the phone, known dimmed); a click opens its card.
fn word_button(mtm: MainThreadMarker, clicker: &Clicker, t: &Tok, size: f64, tag: isize) -> Retained<NSButton> {
    use objc2_foundation::{NSAttributedString, NSDictionary};
    let b = unsafe { NSButton::buttonWithTitle_target_action(&NSString::from_str(&t.text), Some(clicker), Some(objc2::sel!(clicked:)), mtm) };
    b.setBordered(false);
    b.setTag(tag);
    let color = match t.status {
        1 => rgb(0.98, 0.55, 0.45),
        2 => rgb(0.55, 0.55, 0.58),
        _ => rgb(0.95, 0.95, 0.95),
    };
    let font = NSFont::systemFontOfSize(size);
    let attrs = unsafe {
        NSDictionary::<objc2_foundation::NSString, objc2::runtime::AnyObject>::from_slices(
            &[objc2_app_kit::NSFontAttributeName, objc2_app_kit::NSForegroundColorAttributeName],
            &[&*font as &objc2::runtime::AnyObject, &*color as &objc2::runtime::AnyObject],
        )
    };
    let title = unsafe { NSAttributedString::new_with_attributes(&NSString::from_str(&t.text), &attrs) };
    b.setAttributedTitle(&title);
    // As wide as the word itself: a button's own padding spread the words apart.
    let w = objc2_app_kit::NSAttributedStringNSStringDrawing::size(&*title).width.ceil() + 1.0;
    b.sizeToFit();
    let h = b.frame().size.height;
    b.setFrame(rect(0.0, 0.0, w, h));
    b
}

fn new_panel(mtm: MainThreadMarker) -> (Retained<NSPanel>, Retained<NSBox>) {
    let style = NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel;
    let panel =
        NSPanel::initWithContentRect_styleMask_backing_defer(NSPanel::alloc(mtm), rect(0.0, 0.0, 200.0, 30.0), style, NSBackingStoreType::Buffered, false);
    unsafe { panel.setReleasedWhenClosed(false) };
    panel.setOpaque(false);
    panel.setBackgroundColor(Some(&NSColor::clearColor()));
    panel.setHasShadow(true);
    panel.setLevel(101);
    panel.setHidesOnDeactivate(false);
    // Clicks reach the words without taking focus from the app being typed in.
    panel.setBecomesKeyOnlyIfNeeded(true);
    panel.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces | NSWindowCollectionBehavior::FullScreenAuxiliary | NSWindowCollectionBehavior::Transient,
    );
    let bg = NSBox::new(mtm);
    bg.setBoxType(NSBoxType::Custom);
    bg.setTitlePosition(NSTitlePosition::NoTitle);
    bg.setBorderWidth(0.0);
    bg.setCornerRadius(9.0);
    bg.setFillColor(&NSColor::colorWithSRGBRed_green_blue_alpha(0.12, 0.12, 0.13, 0.97));
    bg.setContentViewMargins(NSSize::new(0.0, 0.0));
    panel.setContentView(Some(&bg));
    (panel, bg)
}

/// Where the preview goes, in Cocoa's coordinates (from the foot of the main
/// screen): `place` is in Accessibility's (from the top of the primary
/// screen, `primary_h` tall), `vis` is the screen's visible area in Cocoa's.
pub fn spot((w, h): (f64, f64), place: Place, primary_h: f64, vis: Rect) -> (f64, f64) {
    let (floor, ceil) = (vis.y + 4.0, vis.y + vis.h - 4.0);
    let (left, right) = (vis.x + 4.0, vis.x + vis.w - 4.0);
    // A box small enough to step around (a message or search box); a
    // document is too big, so its cursor's line is what's kept clear.
    let anchor = match (place.frame, place.caret) {
        (Some(f), _) if f.h <= 220.0 => Some(f),
        (_, Some(c)) => Some(c),
        (f, None) => f,
    };
    let Some(a) = anchor else {
        return (vis.x + (vis.w - w) / 2.0, vis.y + 24.0);
    };
    let (top, bottom) = (primary_h - a.y, primary_h - (a.y + a.h));
    let below = bottom - h - 6.0;
    let above = top + 6.0;
    let y = if below >= floor {
        below
    } else if above + h <= ceil {
        above
    } else {
        // A box filling the screen top to foot: inside it, at its foot.
        floor
    };
    let x0 = place.caret.map(|c| c.x - 6.0).unwrap_or(a.x);
    (x0.min(right - w).max(left), y)
}

impl View {
    pub fn new(mtm: MainThreadMarker) -> View {
        let (panel, bg) = new_panel(mtm);
        let (card, card_bg) = new_panel(mtm);
        let clicker: Retained<Clicker> = unsafe { objc2::msg_send![Clicker::alloc(mtm), init] };
        View {
            panel,
            bg,
            shown: std::cell::Cell::new(false),
            rows: Default::default(),
            size: std::cell::Cell::new((0.0, 0.0)),
            clicker,
            card,
            card_bg,
            card_open: std::cell::Cell::new(false),
        }
    }

    /// Lays the rows out afresh: flag, then the words (readings over them).
    /// Each word's button tag is row * 1000 + its index.
    fn build(&self, mtm: MainThreadMarker, rows: &[Row]) -> (f64, f64) {
        let Some(content) = self.bg.contentView() else { return (0.0, 0.0) };
        for v in content.subviews().iter() {
            v.removeFromSuperview();
        }
        let mut built: Vec<(Vec<(Retained<objc2_app_kit::NSView>, f64, f64)>, f64)> = Vec::new();
        let mut widest: f64 = 0.0;
        for (ri, r) in rows.iter().enumerate() {
            let mut parts: Vec<(Retained<objc2_app_kit::NSView>, f64, f64)> = Vec::new();
            let cjk = r.toks.iter().any(|t| !t.reading.is_empty()) || matches!(r.lang.as_str(), "ja" | "zh");
            let size = if cjk { CJK } else { TEXT };
            let flag = label(mtm, &r.flag, TEXT - 2.0, false);
            let fw = flag.frame().size.width;
            let has_ruby = r.toks.iter().any(|t| !t.reading.is_empty());
            let reading_h = if has_ruby { label(mtm, "あ", READING, true).frame().size.height - 2.0 } else { 0.0 };
            let text_h = label(mtm, "Ag", size, false).frame().size.height;
            parts.push((objc2::rc::Retained::into_super(objc2::rc::Retained::into_super(flag)), 0.0, 0.0));
            let x0 = fw + 6.0;
            // Words from the end back while they fit: the end is what's being typed.
            let mut items: Vec<(Retained<objc2_app_kit::NSView>, Option<Retained<NSTextField>>, f64, f64)> = Vec::new();
            let mut used = 0.0;
            for (ti, t) in r.toks.iter().enumerate().rev() {
                // A space is a space's width, not a label's (labels pad themselves).
                if !t.word && t.text.trim().is_empty() {
                    let gap = if cjk { 2.0 } else { size * 0.28 };
                    used += gap;
                    items.push((objc2_app_kit::NSView::new(mtm), None, gap, 0.0));
                    continue;
                }
                let v: Retained<objc2_app_kit::NSView> = if t.word {
                    let b = word_button(mtm, &self.clicker, t, size, (ri * 1000 + ti) as isize);
                    objc2::rc::Retained::into_super(objc2::rc::Retained::into_super(b))
                } else {
                    objc2::rc::Retained::into_super(objc2::rc::Retained::into_super(label(mtm, &t.text, size, t.text == "…")))
                };
                let vw = v.frame().size.width;
                let rl = (!t.reading.is_empty()).then(|| label(mtm, &t.reading, READING, true));
                let w = vw.max(rl.as_ref().map(|l| l.frame().size.width).unwrap_or(0.0)) + if cjk && t.word { 2.0 } else { 0.0 };
                if used + w > MAX_W - 2.0 * PAD_X - x0 - 14.0 && !items.is_empty() {
                    let dots = label(mtm, "…", size, false);
                    let dw = dots.frame().size.width;
                    items.push((objc2::rc::Retained::into_super(objc2::rc::Retained::into_super(dots)), None, dw, dw));
                    break;
                }
                used += w;
                items.push((v, rl, w, vw));
            }
            let mut x = x0;
            for (v, rl, w, vw) in items.into_iter().rev() {
                parts.push((v, x + (w - vw) / 2.0, 0.0));
                if let Some(rl) = rl {
                    let rw = rl.frame().size.width;
                    parts.push((objc2::rc::Retained::into_super(objc2::rc::Retained::into_super(rl)), x + (w - rw) / 2.0, text_h - 3.0));
                }
                x += w;
            }
            widest = widest.max(x);
            built.push((parts, text_h + reading_h));
        }
        let w = (widest + 2.0 * PAD_X).min(MAX_W);
        let n = built.len() as f64;
        let h: f64 = built.iter().map(|(_, rh)| rh).sum::<f64>() + (n - 1.0).max(0.0) * GAP + 2.0 * PAD_Y;
        let mut top = h - PAD_Y;
        for (parts, rh) in built {
            let base = top - rh;
            for (v, x, dy) in parts {
                let f = v.frame().size;
                v.setFrame(rect(PAD_X + x, base + dy, f.width, f.height));
                content.addSubview(&v);
            }
            top = base - GAP;
        }
        (w, h)
    }

    /// Puts `rows` up (one language each, the first on top) where they cover
    /// neither the text being typed nor the box it's in: under the box (a
    /// message box, a search field), or over it when there's no room below; in
    /// a big document, under the cursor's line (over it near the screen's
    /// foot). With nothing to go by, at the foot of the screen.
    pub fn show(&self, mtm: MainThreadMarker, rows: &[Row], place: Place) {
        if rows.is_empty() {
            return self.hide();
        }
        if *self.rows.borrow() != rows {
            let size = self.build(mtm, rows);
            self.size.set(size);
            *self.rows.borrow_mut() = rows.to_vec();
        }
        let (w, h) = self.size.get();
        let screens = NSScreen::screens(mtm);
        let primary_h = screens.firstObject().map(|p| p.frame().size.height).unwrap_or(900.0);
        let vis = NSScreen::mainScreen(mtm).map(|m| m.visibleFrame()).unwrap_or(rect(0.0, 0.0, 1200.0, 800.0));
        let (x, y) = spot(
            (w, h),
            place,
            primary_h,
            Rect { x: vis.origin.x, y: vis.origin.y, w: vis.size.width, h: vis.size.height },
        );
        self.panel.setFrame_display(rect(x, y, w, h), true);
        self.bg.setFrame(rect(0.0, 0.0, w, h));
        if !self.shown.get() {
            self.panel.orderFrontRegardless();
            self.shown.set(true);
        }
    }

    pub fn hide(&self) {
        if self.shown.get() && !self.card_open.get() {
            self.panel.orderOut(None);
            self.shown.set(false);
        }
    }

    /// A word's card beside the preview: the word, what it means, Learning and
    /// Known (lit when it is), and a way to close it. Button tags: -1 close,
    /// -2 learning, -3 known.
    pub fn open_card(&self, mtm: MainThreadMarker, head: &str, meaning: &str, status: u8) {
        let Some(content) = self.card_bg.contentView() else { return };
        for v in content.subviews().iter() {
            v.removeFromSuperview();
        }
        let w = 300.0;
        let word = label(mtm, head, 20.0, false);
        word.setFont(Some(&NSFont::boldSystemFontOfSize(20.0)));
        word.sizeToFit();
        let mean = NSTextField::wrappingLabelWithString(&NSString::from_str(meaning), mtm);
        mean.setFont(Some(&NSFont::systemFontOfSize(13.0)));
        mean.setTextColor(Some(&rgb(0.85, 0.85, 0.87)));
        mean.setPreferredMaxLayoutWidth(w - 24.0);
        let mh = mean.sizeThatFits(NSSize::new(w - 24.0, 200.0)).height.min(70.0);
        let btn = |title: &str, tag: isize, on: bool| {
            let t = Tok { text: title.to_string(), reading: String::new(), word: true, key: String::new(), status: if on { 1 } else { 0 } };
            word_button(mtm, &self.clicker, &t, 13.0, tag)
        };
        let learning = btn("Learning", -2, status == 1);
        let known = btn("Known", -3, status == 2);
        let close = btn("✕", -1, false);
        let h = 12.0 + word.frame().size.height + 4.0 + mh + 10.0 + 22.0 + 10.0;
        let mut y = h - 12.0 - word.frame().size.height;
        word.setFrame(rect(12.0, y, w - 50.0, word.frame().size.height));
        close.setFrame(rect(w - 30.0, h - 30.0, 20.0, 20.0));
        y -= 4.0 + mh;
        mean.setFrame(rect(12.0, y, w - 24.0, mh));
        learning.setFrame(rect(12.0, 10.0, 90.0, 22.0));
        known.setFrame(rect(110.0, 10.0, 90.0, 22.0));
        for v in [&*word as &objc2_app_kit::NSView, &*mean, &*learning, &*known, &*close] {
            content.addSubview(v);
        }
        let f = self.panel.frame();
        self.card.setFrame_display(rect(f.origin.x, f.origin.y - h - 6.0, w, h), true);
        self.card_bg.setFrame(rect(0.0, 0.0, w, h));
        self.card.orderFrontRegardless();
        self.card_open.set(true);
    }

    pub fn close_card(&self) {
        self.card.orderOut(None);
        self.card_open.set(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 1920×1080 screen, menu bar 30 pt, no dock.
    const H: f64 = 1080.0;
    const VIS: Rect = Rect { x: 0.0, y: 0.0, w: 1920.0, h: 1050.0 };
    const SIZE: (f64, f64) = (400.0, 150.0);

    /// The panel's top and bottom in Accessibility's coordinates.
    fn span(y: f64) -> (f64, f64) {
        (H - (y + SIZE.1), H - y)
    }

    #[test]
    fn a_message_box_at_the_foot_is_kept_clear() {
        // Claude's box, no cursor position given: over the box, not on it.
        let frame = Rect { x: 672.0, y: 979.0, w: 921.0, h: 53.0 };
        let (x, y) = spot(SIZE, Place { caret: None, frame: Some(frame) }, H, VIS);
        let (_, bottom) = span(y);
        assert!(bottom <= frame.y, "covers the box: {bottom} > {}", frame.y);
        assert_eq!(x, frame.x);
        // With a cursor in it: still clear of the whole box, at the cursor's x.
        let caret = Rect { x: 900.0, y: 995.0, w: 1.0, h: 18.0 };
        let (x, y) = spot(SIZE, Place { caret: Some(caret), frame: Some(frame) }, H, VIS);
        assert!(span(y).1 <= frame.y);
        assert_eq!(x, 894.0);
    }

    #[test]
    fn a_search_box_at_the_top_has_it_under() {
        let frame = Rect { x: 100.0, y: 60.0, w: 300.0, h: 28.0 };
        let caret = Rect { x: 150.0, y: 65.0, w: 1.0, h: 18.0 };
        let (_, y) = spot(SIZE, Place { caret: Some(caret), frame: Some(frame) }, H, VIS);
        assert!(span(y).0 >= frame.y + frame.h);
    }

    #[test]
    fn a_document_keeps_the_cursor_line_clear() {
        // TextEdit's page is far too big to step around: under the line typed on.
        let frame = Rect { x: 200.0, y: 80.0, w: 900.0, h: 2400.0 };
        let caret = Rect { x: 500.0, y: 300.0, w: 1.0, h: 16.0 };
        let (_, y) = spot(SIZE, Place { caret: Some(caret), frame: Some(frame) }, H, VIS);
        assert!(span(y).0 >= caret.y + caret.h);
        // Near the foot of the screen: over the line instead.
        let caret = Rect { x: 500.0, y: 1000.0, w: 1.0, h: 16.0 };
        let (_, y) = spot(SIZE, Place { caret: Some(caret), frame: Some(frame) }, H, VIS);
        assert!(span(y).1 <= caret.y);
    }

    #[test]
    fn nothing_to_go_by_is_the_foot_of_the_screen() {
        let (x, y) = spot(SIZE, Place::default(), H, VIS);
        assert_eq!((x, y), (760.0, 24.0));
    }

    #[test]
    fn never_off_the_right_edge() {
        let caret = Rect { x: 1900.0, y: 300.0, w: 1.0, h: 16.0 };
        let (x, _) = spot(SIZE, Place { caret: Some(caret), frame: None }, H, VIS);
        assert!(x + SIZE.0 <= VIS.w);
    }
}
