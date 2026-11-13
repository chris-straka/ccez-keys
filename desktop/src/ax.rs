//! The text field being typed in, read through macOS Accessibility (the
//! one permission the app asks for): its text before the cursor and where
//! the cursor is on screen. Never keystrokes.

use std::ffi::c_void;

use objc2::rc::Retained;
use objc2_foundation::NSString;

type Ref = *const c_void;

pub use crate::model::{Field, Place, Rect};

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CFRange {
    location: isize,
    length: isize,
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXUIElementCreateSystemWide() -> Ref;
    fn AXUIElementCopyAttributeValue(element: Ref, attribute: Ref, value: *mut Ref) -> i32;
    fn AXUIElementCopyParameterizedAttributeValue(element: Ref, attribute: Ref, parameter: Ref, value: *mut Ref) -> i32;
    fn AXValueGetValue(value: Ref, kind: u32, out: *mut c_void) -> bool;
    fn AXValueCreate(kind: u32, value: *const c_void) -> Ref;
    fn AXIsProcessTrustedWithOptions(options: Ref) -> bool;
    fn AXUIElementSetMessagingTimeout(element: Ref, seconds: f32) -> i32;
    fn AXUIElementCreateApplication(pid: i32) -> Ref;
    fn AXUIElementSetAttributeValue(element: Ref, attribute: Ref, value: Ref) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFBooleanTrue: Ref;
    static kCFBooleanFalse: Ref;
}

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    /// On while any password box has the keyboard (or Terminal's secure entry).
    fn IsSecureEventInputEnabled() -> bool;
}

/// Electron and Chrome apps (Claude, Slack, Discord, VS Code, Brave…) share
/// their text fields only once an assistive app asks: this asks, once per
/// app, with AXManualAccessibility, the switch Chromium keeps for that.
/// Never AXEnhancedUserInterface: it treats us as a screen reader, and
/// Chromium pages then redrew with after-images and flicker (ccez-keeps and
/// ccez-llm, owner 2026-11-10). Turned back off where an older build set it.
pub fn wake(pid: i32) {
    let app = Owned(unsafe { AXUIElementCreateApplication(pid) });
    let manual = s("AXManualAccessibility");
    let enhanced = s("AXEnhancedUserInterface");
    unsafe {
        AXUIElementSetAttributeValue(app.0, Retained::as_ptr(&manual) as Ref, kCFBooleanTrue);
        AXUIElementSetAttributeValue(app.0, Retained::as_ptr(&enhanced) as Ref, kCFBooleanFalse);
    }
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(r: Ref);
    fn CFRetain(r: Ref) -> Ref;
    fn CFGetTypeID(r: Ref) -> usize;
    fn CFStringGetTypeID() -> usize;
    fn CFArrayGetTypeID() -> usize;
    fn CFArrayGetCount(a: Ref) -> isize;
    fn CFArrayGetValueAtIndex(a: Ref, i: isize) -> Ref;
}

const AX_CGPOINT: u32 = 1;
const AX_CGSIZE: u32 = 2;
const AX_CGRECT: u32 = 3;
const AX_CFRANGE: u32 = 4;

fn s(t: &str) -> Retained<NSString> {
    NSString::from_str(t)
}

/// A CF object we own, released when dropped.
struct Owned(Ref);
impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CFRelease(self.0) }
        }
    }
}

fn attr(el: Ref, name: &str) -> Option<Owned> {
    let mut out: Ref = std::ptr::null();
    let name = s(name);
    let err = unsafe { AXUIElementCopyAttributeValue(el, Retained::as_ptr(&name) as Ref, &mut out) };
    (err == 0 && !out.is_null()).then_some(Owned(out))
}

fn string(r: &Owned) -> Option<String> {
    // Only a string is read as one (an app may answer a number or nothing).
    if unsafe { CFGetTypeID(r.0) != CFStringGetTypeID() } {
        return None;
    }
    // CFString and NSString are the same object.
    let ns = unsafe { &*(r.0 as *const NSString) };
    Some(ns.to_string())
}

/// Asks once for Accessibility (macOS shows its own prompt); true when granted.
pub fn trusted(prompt: bool) -> bool {
    use objc2_foundation::{NSDictionary, NSNumber};
    let key = s("AXTrustedCheckOptionPrompt");
    let dict = NSDictionary::from_slices(&[&*key], &[&*NSNumber::new_bool(prompt)]);
    unsafe { AXIsProcessTrustedWithOptions(&*dict as *const _ as Ref) }
}

/// Where the field is on screen: its frame, from AXPosition and AXSize.
fn frame_of(el: Ref) -> Option<Rect> {
    let mut r = Rect::default();
    let pos = attr(el, "AXPosition")?;
    let size = attr(el, "AXSize")?;
    let mut p = [0f64; 2];
    let mut z = [0f64; 2];
    if !unsafe { AXValueGetValue(pos.0, AX_CGPOINT, p.as_mut_ptr() as *mut c_void) && AXValueGetValue(size.0, AX_CGSIZE, z.as_mut_ptr() as *mut c_void) } {
        return None;
    }
    (r.x, r.y, r.w, r.h) = (p[0], p[1], z[0], z[1]);
    (r.w > 0.0 && r.h > 0.0).then_some(r)
}

/// The front window as idle words see it.
#[derive(Default)]
pub struct Layout {
    /// The window itself.
    pub window: Option<Rect>,
    /// Its content: text, pictures, controls.
    pub content: Vec<Rect>,
    /// What in it has a video's shape (idle::Videos says which are videos).
    pub videos: Vec<crate::idle::Video>,
}

/// An element's children (none for a leaf or an app that won't say).
fn children(el: Ref) -> Vec<Owned> {
    let Some(a) = attr(el, "AXChildren").filter(|a| unsafe { CFGetTypeID(a.0) == CFArrayGetTypeID() }) else {
        return Vec::new();
    };
    (0..unsafe { CFArrayGetCount(a.0) }).map(|i| unsafe { CFArrayGetValueAtIndex(a.0, i) }).filter(|c| !c.is_null()).map(|c| Owned(unsafe { CFRetain(c) })).collect()
}

/// A web element's classes (Chromium's AXDOMClassList).
fn classes(el: Ref) -> Vec<String> {
    let Some(a) = attr(el, "AXDOMClassList").filter(|a| unsafe { CFGetTypeID(a.0) == CFArrayGetTypeID() }) else {
        return Vec::new();
    };
    (0..unsafe { CFArrayGetCount(a.0) })
        .map(|i| unsafe { CFArrayGetValueAtIndex(a.0, i) })
        .filter(|c| !c.is_null() && unsafe { CFGetTypeID(*c) == CFStringGetTypeID() })
        .map(|c| unsafe { &*(c as *const NSString) }.to_string())
        .collect()
}

/// A video's controls, when they're up (its time slider is among its
/// children): whether its play button says paused (its class "pause"),
/// looked for among the controls and one level into them, where Chromium
/// keeps it.
fn controls(video: Ref) -> Option<bool> {
    let role = |el: Ref| attr(el, "AXRole").and_then(|r| string(&r)).unwrap_or_default();
    let kids = children(video);
    if !kids.iter().skip(1).any(|c| role(c.0) == "AXSlider") {
        return None;
    }
    let is_mark = |el: Ref| role(el) == "AXButton" && classes(el).iter().any(|c| c == "pause");
    Some(kids.iter().skip(1).any(|c| is_mark(c.0) || children(c.0).iter().any(|g| is_mark(g.0))))
}

/// Where the front window's content is (its text, pictures and controls,
/// from its Accessibility tree), the window itself, and what in it has a
/// video's shape: what idle words keep clear of. Stops after `most` elements
/// or `budget`, whichever comes first.
pub fn content_frames(pid: i32, most: usize, budget: std::time::Duration) -> Layout {
    const CONTENT: [&str; 16] = [
        "AXStaticText",
        "AXTextArea",
        "AXTextField",
        "AXLink",
        "AXButton",
        "AXImage",
        "AXHeading",
        "AXCheckBox",
        "AXPopUpButton",
        "AXMenuButton",
        "AXRadioButton",
        "AXComboBox",
        "AXSearchField",
        "AXSlider",
        "AXCell",
        "AXIncrementor",
    ];
    let start = std::time::Instant::now();
    let app = Owned(unsafe { AXUIElementCreateApplication(pid) });
    unsafe { AXUIElementSetMessagingTimeout(app.0, 0.25) };
    let Some(window) = attr(app.0, "AXFocusedWindow").or_else(|| attr(app.0, "AXMainWindow")) else {
        return Layout::default();
    };
    let frame = frame_of(window.0);
    let mut out = Vec::new();
    let mut videos = Vec::new();
    // Each element; a first child comes with its parent and how many
    // children that has (a video is told by its first child).
    let mut queue: std::collections::VecDeque<(Owned, Option<(Owned, isize)>)> = std::collections::VecDeque::new();
    queue.push_back((window, None));
    let mut seen = 0;
    while let Some((el, up)) = queue.pop_front() {
        seen += 1;
        if seen > most || start.elapsed() > budget {
            break;
        }
        let kids = attr(el.0, "AXChildren").filter(|a| unsafe { CFGetTypeID(a.0) == CFArrayGetTypeID() });
        let n = kids.as_ref().map(|a| unsafe { CFArrayGetCount(a.0) }).unwrap_or(0);
        let role = attr(el.0, "AXRole").and_then(|r| string(&r)).unwrap_or_default();
        // Content, and any small leaf with a size (a web page's text runs, an
        // icon). A big empty leaf is a backdrop (a web app's panes), not content.
        let small = |f: &Rect| frame.is_none_or(|w| f.w * f.h < w.w * w.h * 0.08);
        if (CONTENT.contains(&role.as_str()) || n == 0)
            && let Some(f) = frame_of(el.0)
        {
            // A video's picture (the browser gives it no class): noted, with
            // its controls when they're up. More than the picture and no time
            // slider: not a video.
            if n == 0
                && let Some((parent, siblings)) = &up
                && let Some(v) = crate::idle::video_frame(&role, f, || Some((attr(parent.0, "AXRole").and_then(|r| string(&r))?, frame_of(parent.0)?)))
                && classes(el.0).is_empty()
            {
                match (*siblings, controls(parent.0)) {
                    (_, Some(paused)) => videos.push(crate::idle::Video { frame: v, controls: true, paused }),
                    (1, None) => videos.push(crate::idle::Video { frame: v, controls: false, paused: false }),
                    _ => {}
                }
            }
            if frame.is_none_or(|w| f.w < w.w * 0.95 || f.h < w.h * 0.95) && (CONTENT.contains(&role.as_str()) || small(&f)) {
                out.push(f);
                if CONTENT.contains(&role.as_str()) && role != "AXCell" {
                    continue;
                }
            }
        }
        if let Some(a) = kids {
            for i in 0..n {
                let c = unsafe { CFArrayGetValueAtIndex(a.0, i) };
                if !c.is_null() {
                    let up = (i == 0).then(|| (Owned(unsafe { CFRetain(el.0) }), n));
                    queue.push_back((Owned(unsafe { CFRetain(c) }), up));
                }
            }
        }
    }
    Layout { window: frame, content: out, videos }
}

/// A run of text's width in points in the system font at 15 pt (what web
/// text boxes mostly use), for guessing where their cursor is.
fn text_width(t: &str) -> f64 {
    use objc2_foundation::{NSAttributedString, NSDictionary};
    let font = objc2_app_kit::NSFont::systemFontOfSize(15.0);
    let attrs = unsafe { NSDictionary::<NSString, objc2::runtime::AnyObject>::from_slices(&[objc2_app_kit::NSFontAttributeName], &[&*font as &objc2::runtime::AnyObject]) };
    let a = unsafe { NSAttributedString::new_with_attributes(&NSString::from_str(t), &attrs) };
    objc2_app_kit::NSAttributedStringNSStringDrawing::size(&*a).width
}

/// The field being typed in, None when the focus isn't on editable text.
pub fn focused() -> Option<Field> {
    let el = focused_element(0.25)?;
    let role = attr(el.0, "AXRole").and_then(|r| string(&r)).unwrap_or_default();
    let subrole = attr(el.0, "AXSubrole").and_then(|r| string(&r)).unwrap_or_default();
    // A password box: said so by the app, or labelled as one, or macOS's secure
    // input is on (any password box focused, anywhere): never read or shown.
    let label: String = ["AXPlaceholderValue", "AXDescription", "AXTitle", "AXHelp"].iter().filter_map(|a| attr(el.0, a).and_then(|r| string(&r))).collect::<Vec<_>>().join(" ").to_lowercase();
    let secret_label =
        ["password", "passcode", "passphrase", "pin", "otp", "one-time", "token", "api key", "secret", "2fa", "verification code", "mot de passe", "passwort", "contraseña", "パスワード", "密码"]
            .iter()
            .any(|w| if *w == "pin" || *w == "otp" { label.split(|c: char| !c.is_alphanumeric()).any(|t| t == *w) } else { label.contains(w) });
    let secure = role == "AXSecureTextField" || subrole == "AXSecureTextField" || secret_label || unsafe { IsSecureEventInputEnabled() };
    if secure {
        return Some(Field { secure, ..Default::default() });
    }
    if !matches!(role.as_str(), "AXTextField" | "AXTextArea" | "AXComboBox" | "AXSearchField" | "AXWebArea") {
        return None;
    }
    let value = attr(el.0, "AXValue").and_then(|r| string(&r))?;
    // Dots for letters: a password box that didn't say so.
    if !value.is_empty() && value.chars().all(|c| matches!(c, '•' | '●' | '∙' | '*')) {
        return Some(Field { secure: true, ..Default::default() });
    }
    let mut range = CFRange::default();
    let sel = attr(el.0, "AXSelectedTextRange")?;
    if !unsafe { AXValueGetValue(sel.0, AX_CFRANGE, &mut range as *mut _ as *mut c_void) } {
        return None;
    }
    // AX counts UTF-16 units.
    let units: Vec<u16> = value.encode_utf16().collect();
    let at = (range.location.max(0) as usize).min(units.len());
    let before = String::from_utf16_lossy(&units[..at]);
    // A selection previews itself (owner, 2026-11-11).
    let end = (at + range.length.max(0) as usize).min(units.len());
    let selected = String::from_utf16_lossy(&units[at..end]);
    // Where the cursor is: the bounds of the character before it (or of the empty range).
    let caret = {
        let r = CFRange { location: (at as isize - 1).max(0), length: if at > 0 { 1 } else { 0 } };
        let param = Owned(unsafe { AXValueCreate(AX_CFRANGE, &r as *const _ as *const c_void) });
        let name = s("AXBoundsForRange");
        let mut out: Ref = std::ptr::null();
        let err = unsafe { AXUIElementCopyParameterizedAttributeValue(el.0, Retained::as_ptr(&name) as Ref, param.0, &mut out) };
        if err == 0 && !out.is_null() {
            let o = Owned(out);
            let mut rect = Rect::default();
            unsafe { AXValueGetValue(o.0, AX_CGRECT, &mut rect as *mut _ as *mut c_void) }.then_some(rect).filter(|r| r.h > 0.0)
        } else {
            None
        }
    };
    // Some apps (Electron) answer with nothing or a zero box: kept only when it's inside the field.
    let frame = frame_of(el.0);
    let caret = caret.filter(|c| (c.x != 0.0 || c.y != 0.0) && frame.is_none_or(|f| c.y + c.h >= f.y - 2.0 && c.y <= f.y + f.h + 2.0));
    // No cursor from the app (Claude's box, other web text boxes): where the
    // text typed ends, laid out in the box (owner, 2026-11-11: the preview
    // sat at the box's left edge, far from the cursor).
    let caret = caret.or_else(|| frame.map(|f| crate::model::estimate_caret(f, &before, text_width, 20.0, 2.0)));
    Some(Field { secure, before, caret, frame, selected })
}

/// The element with the keyboard: asked of the whole system, or else of the
/// app in front (Electron apps sometimes answer only that).
fn focused_element(timeout: f32) -> Option<Owned> {
    let sys = Owned(unsafe { AXUIElementCreateSystemWide() });
    unsafe { AXUIElementSetMessagingTimeout(sys.0, timeout) };
    if let Some(el) = attr(sys.0, "AXFocusedUIElement") {
        return Some(el);
    }
    let pid = objc2_app_kit::NSWorkspace::sharedWorkspace().frontmostApplication()?.processIdentifier();
    let app = Owned(unsafe { AXUIElementCreateApplication(pid) });
    unsafe { AXUIElementSetMessagingTimeout(app.0, timeout) };
    attr(app.0, "AXFocusedUIElement")
}

/// For `--probe`: the focused element's role, subrole and frame, whatever it is.
pub fn describe() -> String {
    let Some(el) = focused_element(0.5) else {
        return "nothing focused".into();
    };
    let get = |a: &str| attr(el.0, a).and_then(|r| string(&r)).unwrap_or_default();
    format!("role={} subrole={} frame={:?}", get("AXRole"), get("AXSubrole"), frame_of(el.0))
}

/// What putting text in place of a range came to.
#[derive(Debug, PartialEq)]
pub enum Put {
    /// The field holds it now.
    Done,
    /// The range is selected (the app wouldn't take the text): typing replaces it.
    Selected,
    /// Neither: the field is left as it was.
    Nothing,
}

/// The focused field's whole text (None: no text field, or a password box).
pub fn field_text() -> Option<String> {
    let f = focused()?;
    if f.secure {
        return None;
    }
    let el = focused_element(0.5)?;
    attr(el.0, "AXValue").and_then(|r| string(&r))
}

/// The focused field's selection (UTF-16 units), if it says.
pub fn selection() -> Option<(usize, usize)> {
    let el = focused_element(0.5)?;
    let r = attr(el.0, "AXSelectedTextRange")?;
    let mut got = CFRange::default();
    let ok = unsafe { AXValueGetValue(r.0, AX_CFRANGE, &mut got as *mut _ as *mut c_void) };
    ok.then_some((got.location.max(0) as usize, got.length.max(0) as usize))
}

/// The focused field's text range `start..start+len` (UTF-16 units, as AX
/// counts) replaced by `text` through Accessibility. Web and Electron fields
/// apply changes a moment later, so it looks again after a pause; a field
/// that took neither the text nor the selection is left alone.
pub fn replace_range(start: usize, len: usize, text: &str) -> Put {
    let Some(el) = focused_element(0.5) else {
        return Put::Nothing;
    };
    let range = CFRange { location: start as isize, length: len as isize };
    let value = Owned(unsafe { AXValueCreate(AX_CFRANGE, &range as *const _ as *const c_void) });
    let name = s("AXSelectedTextRange");
    unsafe { AXUIElementSetAttributeValue(el.0, Retained::as_ptr(&name) as Ref, value.0) };
    std::thread::sleep(std::time::Duration::from_millis(120));
    if selection() != Some((start, len)) {
        return Put::Nothing;
    }
    let before = attr(el.0, "AXValue").and_then(|r| string(&r)).unwrap_or_default();
    let new = s(text);
    let name = s("AXSelectedText");
    let err = unsafe { AXUIElementSetAttributeValue(el.0, Retained::as_ptr(&name) as Ref, Retained::as_ptr(&new) as Ref) };
    std::thread::sleep(std::time::Duration::from_millis(150));
    let after = attr(el.0, "AXValue").and_then(|r| string(&r)).unwrap_or_default();
    if err == 0 && after != before {
        // Taken (web fields may store its spaces as no-break ones: any change counts).
        Put::Done
    } else if selection() == Some((start, len)) {
        Put::Selected
    } else {
        Put::Nothing
    }
}
