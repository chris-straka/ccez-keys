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

pub struct View {
    panel: Retained<NSPanel>,
    bg: Retained<NSBox>,
    /// One label per language, stacked (owner, 2026-11-09: each on its own line).
    labels: Vec<Retained<NSTextField>>,
    shown: std::cell::Cell<bool>,
}

/// Most languages shown at once.
pub const LINES: usize = 4;
const GAP: f64 = 3.0;

fn rect(x: f64, y: f64, w: f64, h: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
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
        let labels = (0..LINES)
            .map(|_| {
                let l = NSTextField::labelWithString(&NSString::from_str(""), mtm);
                l.setFont(Some(&NSFont::systemFontOfSize(14.0)));
                l.setTextColor(Some(&NSColor::colorWithSRGBRed_green_blue_alpha(0.95, 0.95, 0.95, 1.0)));
                // A long sentence shows its end: the words being typed.
                l.setLineBreakMode(NSLineBreakMode::ByTruncatingHead);
                l.setUsesSingleLineMode(true);
                bg.addSubview(&l);
                l
            })
            .collect();
        View { panel, bg, labels, shown: std::cell::Cell::new(false) }
    }

    /// Puts `lines` up (one language each, the first on top), under `caret`
    /// (top-left screen points) or at the foot of the screen.
    pub fn show(&self, mtm: MainThreadMarker, lines: &[String], caret: Option<Rect>) {
        let n = lines.len().min(LINES);
        if n == 0 {
            return self.hide();
        }
        let mut widest: f64 = 0.0;
        let mut line_h: f64 = 18.0;
        for (i, l) in self.labels.iter().enumerate() {
            let text = lines.get(i).filter(|_| i < n).map(String::as_str).unwrap_or("");
            l.setStringValue(&NSString::from_str(text));
            l.sizeToFit();
            l.setHidden(i >= n);
            if i < n {
                let s = l.frame().size;
                widest = widest.max(s.width);
                line_h = s.height;
            }
        }
        let w = (widest + 2.0 * PAD_X).min(MAX_W);
        let h = n as f64 * line_h + (n as f64 - 1.0) * GAP + 2.0 * PAD_Y;
        // Top line first: AppKit counts y from the bottom.
        for (i, l) in self.labels.iter().take(n).enumerate() {
            let y = h - PAD_Y - (i as f64 + 1.0) * line_h - i as f64 * GAP;
            l.setFrame(rect(PAD_X, y, w - 2.0 * PAD_X, line_h));
        }
        let screens = NSScreen::screens(mtm);
        let primary_h = screens.firstObject().map(|p| p.frame().size.height).unwrap_or(900.0);
        let (x, y) = match caret {
            // AX: top-left origin on the primary screen; AppKit: bottom-left.
            Some(c) => (c.x - 6.0, primary_h - (c.y + c.h) - h - 6.0),
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
