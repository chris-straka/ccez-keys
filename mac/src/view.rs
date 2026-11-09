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
    label: Retained<NSTextField>,
    shown: std::cell::Cell<bool>,
}

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
        let label = NSTextField::labelWithString(&NSString::from_str(""), mtm);
        label.setFont(Some(&NSFont::systemFontOfSize(14.0)));
        label.setTextColor(Some(&NSColor::colorWithSRGBRed_green_blue_alpha(0.95, 0.95, 0.95, 1.0)));
        label.setLineBreakMode(NSLineBreakMode::ByTruncatingHead);
        label.setUsesSingleLineMode(true);
        bg.addSubview(&label);
        View { panel, bg, label, shown: std::cell::Cell::new(false) }
    }

    /// Puts `text` up, under `caret` (top-left screen points) or at the foot of the screen.
    pub fn show(&self, mtm: MainThreadMarker, text: &str, caret: Option<Rect>) {
        self.label.setStringValue(&NSString::from_str(text));
        self.label.sizeToFit();
        let s = self.label.frame().size;
        let w = (s.width + 2.0 * PAD_X).min(MAX_W);
        let h = s.height + 2.0 * PAD_Y;
        self.label.setFrame(rect(PAD_X, PAD_Y, w - 2.0 * PAD_X, s.height));
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
