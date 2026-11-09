//! The text field being typed in, read through macOS Accessibility (the
//! one permission the app asks for): its text before the cursor and where
//! the cursor is on screen. Never keystrokes.

use std::ffi::c_void;

use objc2::rc::Retained;
use objc2_foundation::NSString;

type Ref = *const c_void;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CFRange {
    location: isize,
    length: isize,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
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
}

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    /// On while any password box has the keyboard (or Terminal's secure entry).
    fn IsSecureEventInputEnabled() -> bool;
}

/// Electron and Chrome apps (Claude, Slack, Discord, VS Code…) share their
/// text fields only once an assistive app asks: this asks, once per app.
pub fn wake(pid: i32) {
    let app = Owned(unsafe { AXUIElementCreateApplication(pid) });
    for name in ["AXManualAccessibility", "AXEnhancedUserInterface"] {
        let n = s(name);
        unsafe { AXUIElementSetAttributeValue(app.0, Retained::as_ptr(&n) as Ref, kCFBooleanTrue) };
    }
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(r: Ref);
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

/// Where to keep the preview clear of: the cursor and the field it's in.
#[derive(Clone, Copy, Default, Debug)]
pub struct Place {
    pub caret: Option<Rect>,
    pub frame: Option<Rect>,
}

/// The focused text field now.
pub struct Field {
    /// Its role and subrole (AXTextField / AXTextArea; AXSecureTextField for passwords).
    pub secure: bool,
    /// Its text up to the cursor.
    pub before: String,
    /// The cursor on screen (top-left origin, points), when the app says.
    pub caret: Option<Rect>,
    /// The field itself on screen (same coordinates), when the app says.
    pub frame: Option<Rect>,
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

/// The field being typed in, None when the focus isn't on editable text.
pub fn focused() -> Option<Field> {
    let el = focused_element(0.25)?;
    let role = attr(el.0, "AXRole").and_then(|r| string(&r)).unwrap_or_default();
    let subrole = attr(el.0, "AXSubrole").and_then(|r| string(&r)).unwrap_or_default();
    // A password box: said so by the app, or labelled as one, or macOS's secure
    // input is on (any password box focused, anywhere): never read or shown.
    let label: String = ["AXPlaceholderValue", "AXDescription", "AXTitle", "AXHelp"]
        .iter()
        .filter_map(|a| attr(el.0, a).and_then(|r| string(&r)))
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let secret_label = ["password", "passcode", "passphrase", "pin", "otp", "one-time", "token", "api key", "secret", "2fa", "verification code", "mot de passe", "passwort", "contraseña", "パスワード", "密码"]
        .iter()
        .any(|w| if *w == "pin" || *w == "otp" { label.split(|c: char| !c.is_alphanumeric()).any(|t| t == *w) } else { label.contains(w) });
    let secure = role == "AXSecureTextField" || subrole == "AXSecureTextField" || secret_label || unsafe { IsSecureEventInputEnabled() };
    if secure {
        return Some(Field { secure, before: String::new(), caret: None, frame: None });
    }
    if !matches!(role.as_str(), "AXTextField" | "AXTextArea" | "AXComboBox" | "AXSearchField" | "AXWebArea") {
        return None;
    }
    let value = attr(el.0, "AXValue").and_then(|r| string(&r))?;
    // Dots for letters: a password box that didn't say so.
    if !value.is_empty() && value.chars().all(|c| matches!(c, '•' | '●' | '∙' | '*')) {
        return Some(Field { secure: true, before: String::new(), caret: None, frame: None });
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
    Some(Field { secure, before, caret, frame })
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
    let Some(el) = focused_element(0.5) else { return "nothing focused".into() };
    let get = |a: &str| attr(el.0, a).and_then(|r| string(&r)).unwrap_or_default();
    format!("role={} subrole={} frame={:?}", get("AXRole"), get("AXSubrole"), frame_of(el.0))
}
