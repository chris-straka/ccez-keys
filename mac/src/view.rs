//! The preview: a small dark panel under the text cursor (or at the foot of
//! the screen when the app doesn't say where its cursor is), never taking
//! focus or clicks from the app being typed in.

use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSBox, NSBoxType, NSColor, NSFont, NSLineBreakMode, NSPanel, NSScreen, NSTextField,
    NSTitlePosition, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

use crate::ax::Rect;

/// Widest the line may be, in points: longer, the start gives way to "…".
const MAX_W: f64 = 560.0;
const PAD_X: f64 = 12.0;
const PAD_Y: f64 = 7.0;

/// One language on the preview: its flag, the translation, and for Japanese
/// and Chinese each word with its reading (furigana, pinyin) to put over it.
#[derive(Clone, PartialEq, Debug)]
pub struct Row {
    pub flag: String,
    pub text: String,
    pub ruby: Vec<(String, String)>,
}

pub struct View {
    panel: Retained<NSPanel>,
    bg: Retained<NSBox>,
    shown: std::cell::Cell<bool>,
    /// What's laid out now (rebuilt only when it changes).
    rows: std::cell::RefCell<Vec<Row>>,
    size: std::cell::Cell<(f64, f64)>,
}

/// Most languages shown at once.
pub const LINES: usize = 4;
const GAP: f64 = 4.0;
const TEXT: f64 = 14.0;
/// Japanese and Chinese a size up, their readings clear (owner: hard to read).
const CJK: f64 = 16.0;
const READING: f64 = 10.5;

fn rect(x: f64, y: f64, w: f64, h: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
}

fn label(mtm: MainThreadMarker, text: &str, size: f64, grey: bool) -> Retained<NSTextField> {
    let l = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    l.setFont(Some(&NSFont::systemFontOfSize(size)));
    let c = if grey { 0.74 } else { 0.95 };
    l.setTextColor(Some(&NSColor::colorWithSRGBRed_green_blue_alpha(c, c, c, 1.0)));
    l.setUsesSingleLineMode(true);
    l.sizeToFit();
    l
}

impl View {
    pub fn new(mtm: MainThreadMarker) -> View {
        let style = NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel;
        let panel =
            NSPanel::initWithContentRect_styleMask_backing_defer(NSPanel::alloc(mtm), rect(0.0, 0.0, 200.0, 30.0), style, NSBackingStoreType::Buffered, false);
        unsafe { panel.setReleasedWhenClosed(false) };
        panel.setOpaque(false);
        panel.setBackgroundColor(Some(&NSColor::clearColor()));
        panel.setHasShadow(true);
        panel.setLevel(101);
        panel.setHidesOnDeactivate(false);
        panel.setIgnoresMouseEvents(true);
        panel.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces | NSWindowCollectionBehavior::FullScreenAuxiliary | NSWindowCollectionBehavior::Transient,
        );
        let bg = NSBox::new(mtm);
        bg.setBoxType(NSBoxType::Custom);
        bg.setTitlePosition(NSTitlePosition::NoTitle);
        bg.setBorderWidth(0.0);
        bg.setCornerRadius(9.0);
        bg.setFillColor(&NSColor::colorWithSRGBRed_green_blue_alpha(0.12, 0.12, 0.13, 0.96));
        bg.setContentViewMargins(NSSize::new(0.0, 0.0));
        panel.setContentView(Some(&bg));
        View { panel, bg, shown: std::cell::Cell::new(false), rows: Default::default(), size: std::cell::Cell::new((0.0, 0.0)) }
    }

    /// Lays the rows out afresh: flag, then the words (readings over them).
    fn build(&self, mtm: MainThreadMarker, rows: &[Row]) -> (f64, f64) {
        let content = self.bg.contentView();
        if let Some(c) = &content {
            for v in c.subviews().iter() {
                v.removeFromSuperview();
            }
        }
        let Some(content) = content else { return (0.0, 0.0) };
        // Each row: its labels, x positions from 0, its height.
        let mut built: Vec<(Vec<(Retained<NSTextField>, f64, f64)>, f64)> = Vec::new();
        let mut widest: f64 = 0.0;
        for r in rows.iter().take(LINES) {
            let mut parts: Vec<(Retained<NSTextField>, f64, f64)> = Vec::new();
            let flag = label(mtm, &r.flag, TEXT - 2.0, false);
            let fw = flag.frame().size.width;
            let has_ruby = r.ruby.iter().any(|(_, rd)| !rd.is_empty());
            let reading_h = if has_ruby { label(mtm, "あ", READING, true).frame().size.height - 2.0 } else { 0.0 };
            let text_h = label(mtm, "Ag", if has_ruby { CJK } else { TEXT }, false).frame().size.height;
            parts.push((flag, 0.0, 0.0));
            let mut x = fw + 6.0;
            if r.ruby.is_empty() {
                let t = label(mtm, &r.text, TEXT, false);
                t.setLineBreakMode(NSLineBreakMode::ByTruncatingHead);
                let w = t.frame().size.width.min(MAX_W - 2.0 * PAD_X - x);
                t.setFrame(rect(0.0, 0.0, w, text_h));
                parts.push((t, x, 0.0));
                x += w;
            } else {
                // Words from the end back while they fit: the end is what's being typed.
                let mut words: Vec<(Retained<NSTextField>, Option<Retained<NSTextField>>, f64)> = Vec::new();
                let mut used = 0.0;
                for (t, rd) in r.ruby.iter().rev() {
                    let tl = label(mtm, t, CJK, false);
                    let rl = (!rd.is_empty()).then(|| label(mtm, rd, READING, true));
                    let w = tl.frame().size.width.max(rl.as_ref().map(|l| l.frame().size.width).unwrap_or(0.0));
                    if used + w > MAX_W - 2.0 * PAD_X - x - 14.0 && !words.is_empty() {
                        let dots = label(mtm, "…", TEXT, false);
                        words.push((dots.clone(), None, dots.frame().size.width));
                        break;
                    }
                    used += w;
                    words.push((tl, rl, w));
                }
                for (tl, rl, w) in words.into_iter().rev() {
                    let tw = tl.frame().size.width;
                    parts.push((tl, x + (w - tw) / 2.0, 0.0));
                    if let Some(rl) = rl {
                        let rw = rl.frame().size.width;
                        parts.push((rl, x + (w - rw) / 2.0, text_h - 3.0));
                    }
                    x += w;
                }
            }
            widest = widest.max(x);
            built.push((parts, text_h + reading_h));
        }
        let w = (widest + 2.0 * PAD_X).min(MAX_W);
        let n = built.len() as f64;
        let h: f64 = built.iter().map(|(_, rh)| rh).sum::<f64>() + (n - 1.0).max(0.0) * GAP + 2.0 * PAD_Y;
        // Top row first: AppKit counts y from the bottom.
        let mut top = h - PAD_Y;
        for (parts, rh) in built {
            let base = top - rh;
            for (l, x, dy) in parts {
                let f = l.frame().size;
                l.setFrame(rect(PAD_X + x, base + dy, f.width, f.height));
                content.addSubview(&l);
            }
            top = base - GAP;
        }
        (w, h)
    }

    /// Puts `rows` up (one language each, the first on top), under `caret`
    /// (top-left screen points) or at the foot of the screen.
    pub fn show(&self, mtm: MainThreadMarker, rows: &[Row], caret: Option<Rect>) {
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
        let (x, y) = match caret {
            // AX: top-left origin on the primary screen; AppKit: bottom-left. Under
            // the cursor, or over it when there's no room below (a message box at
            // the foot of the window): never on the line being typed.
            Some(c) => {
                let below = primary_h - (c.y + c.h) - h - 6.0;
                let floor = NSScreen::mainScreen(mtm).map(|m| m.visibleFrame().origin.y).unwrap_or(0.0);
                let y = if below < floor + 4.0 { primary_h - c.y + 6.0 } else { below };
                let right = NSScreen::mainScreen(mtm).map(|m| { let f = m.visibleFrame(); f.origin.x + f.size.width }).unwrap_or(1e9);
                ((c.x - 6.0).min(right - w - 4.0), y)
            }
            None => {
                let f = NSScreen::mainScreen(mtm).map(|m| m.visibleFrame()).unwrap_or(rect(0.0, 0.0, 1200.0, 800.0));
                (f.origin.x + (f.size.width - w) / 2.0, f.origin.y + 24.0)
            }
        };
        self.panel.setFrame_display(rect(x, y, w, h), true);
        self.bg.setFrame(rect(0.0, 0.0, w, h));
        if !self.shown.get() {
            self.panel.orderFrontRegardless();
            self.shown.set(true);
        }
    }

    pub fn hide(&self) {
        if self.shown.get() {
            self.panel.orderOut(None);
            self.shown.set(false);
        }
    }
}
