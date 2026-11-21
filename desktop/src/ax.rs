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
    fn AXUIElementCopyElementAtPosition(application: Ref, x: f32, y: f32, element: *mut Ref) -> i32;
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
    fn CFEqual(a: Ref, b: Ref) -> u8;
    fn CFGetTypeID(r: Ref) -> usize;
    fn CFStringGetTypeID() -> usize;
    fn CFURLGetTypeID() -> usize;
    fn CFURLGetString(url: Ref) -> Ref;
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
    /// The page's address, when it's a browser's (its web area says).
    pub url: Option<String>,
    /// Text-bearing elements seen (text, headings, links, fields, cells).
    pub texts: usize,
    /// The walk got through the window: not stopped by `most` or `budget`.
    pub complete: bool,
}

/// Roles that carry text a word must never cover.
const TEXT_ROLES: [&str; 8] = ["AXStaticText", "AXTextArea", "AXTextField", "AXLink", "AXHeading", "AXCell", "AXSearchField", "AXComboBox"];

/// Whether the front window's layout can be trusted to say where its text
/// isn't: a walk that stopped early, or one that found a window but no text
/// in it (a Chromium app whose accessibility is off or switched itself off
/// again shows only empty groups: owner, 2026-11-18, words over a Claude
/// chat), can't. Then the whole window counts as taken.
pub fn layout_known(texts: usize, complete: bool) -> bool {
    complete && texts > 0
}

/// Whether what a hit-test lands on is content a word must not sit on:
/// anything but the backdrop (the window, its web area or scroll area, a
/// big pane). A group the size of a paragraph or a message counts as content,
/// so the gap between two lines of text does too.
pub fn hit_is_content(role: &str, frame: Option<Rect>, window: Option<Rect>) -> bool {
    const BACKDROP: [&str; 6] = ["AXWindow", "AXWebArea", "AXScrollArea", "AXSplitGroup", "AXLayoutArea", "AXApplication"];
    if BACKDROP.contains(&role) {
        return false;
    }
    if role == "AXGroup" || role == "AXUnknown" || role.is_empty() {
        return match (frame, window) {
            (Some(f), Some(w)) => f.w * f.h < w.w * w.h * 0.25,
            _ => true,
        };
    }
    true
}

/// Points to test under a word: its middle and four points just inside its corners.
pub fn sample_points(r: Rect) -> [(f64, f64); 5] {
    let (ix, iy) = ((r.w * 0.1).min(4.0), (r.h * 0.2).min(3.0));
    [
        (r.x + r.w / 2.0, r.y + r.h / 2.0),
        (r.x + ix, r.y + iy),
        (r.x + r.w - ix, r.y + iy),
        (r.x + ix, r.y + r.h - iy),
        (r.x + r.w - ix, r.y + r.h - iy),
    ]
}

/// Whether anything of app `pid` lies under any of `points` (top-left
/// screen coordinates): Accessibility's hit-test, which follows scrolling
/// at once, unlike the layout walk. `Some(true)`: content; `Some(false)`:
/// clear; `None`: the app didn't answer in time (a short timeout), not sure.
pub fn content_under(pid: i32, points: &[(f64, f64)], window: Option<Rect>) -> Option<bool> {
    let app = Owned(unsafe { AXUIElementCreateApplication(pid) });
    unsafe { AXUIElementSetMessagingTimeout(app.0, 0.15) };
    for &(x, y) in points {
        let mut out: Ref = std::ptr::null();
        let err = unsafe { AXUIElementCopyElementAtPosition(app.0, x as f32, y as f32, &mut out) };
        if err != 0 || out.is_null() {
            // -25204 (cannot complete) and the like: unknown, so not clear.
            // -25212 (no value): nothing of the app there.
            if err == -25212 {
                continue;
            }
            if std::env::var_os("CCEZ_KEYS_DEBUG_UNDER").is_some() {
                eprintln!("under ({x}, {y}): AX error {err}");
            }
            return None;
        }
        let el = Owned(out);
        let role = attr(el.0, "AXRole").and_then(|r| string(&r)).unwrap_or_default();
        if hit_is_content(&role, frame_of(el.0), window) {
            if std::env::var_os("CCEZ_KEYS_DEBUG_UNDER").is_some() {
                eprintln!("under ({x}, {y}): {role} {:?}", frame_of(el.0));
            }
            return Some(true);
        }
    }
    Some(false)
}

/// A URL attribute as text (a CFURL, or a string).
fn url_text(r: &Owned) -> Option<String> {
    if unsafe { CFGetTypeID(r.0) == CFURLGetTypeID() } {
        let s = unsafe { CFURLGetString(r.0) };
        // Got, not copied: the URL owns it.
        return (!s.is_null()).then(|| unsafe { &*(s as *const NSString) }.to_string());
    }
    string(r)
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
    let mut url = None;
    // Each element; a first child comes with its parent and how many
    // children that has (a video is told by its first child).
    let mut queue: std::collections::VecDeque<(Owned, Option<(Owned, isize)>)> = std::collections::VecDeque::new();
    queue.push_back((window, None));
    let mut seen = 0;
    let mut texts = 0;
    let mut complete = true;
    let hits = |a: &Rect, b: &Rect| a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
    while let Some((el, up)) = queue.pop_front() {
        seen += 1;
        if seen > most || start.elapsed() > budget {
            complete = false;
            break;
        }
        let kids = attr(el.0, "AXChildren").filter(|a| unsafe { CFGetTypeID(a.0) == CFArrayGetTypeID() });
        let n = kids.as_ref().map(|a| unsafe { CFArrayGetCount(a.0) }).unwrap_or(0);
        let role = attr(el.0, "AXRole").and_then(|r| string(&r)).unwrap_or_default();
        let at = frame_of(el.0);
        // Scrolled out of the window: nothing in it can be under a word, and
        // a long page's hidden part used to use up the walk (owner,
        // 2026-11-18). A window with no frame, or an element without one: look in.
        if let (Some(f), Some(w)) = (at, frame)
            && !hits(&f, &w)
        {
            continue;
        }
        if TEXT_ROLES.contains(&role.as_str()) {
            texts += 1;
        }
        if url.is_none() && role == "AXWebArea" {
            url = attr(el.0, "AXURL").and_then(|r| url_text(&r));
        }
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
    Layout { window: frame, content: out, videos, url, texts, complete }
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

fn string_for_range(el: Ref, start: usize, len: usize) -> Option<String> {
    if len == 0 {
        return Some(String::new());
    }
    let r = CFRange { location: start as isize, length: len as isize };
    let param = Owned(unsafe { AXValueCreate(AX_CFRANGE, &r as *const _ as *const c_void) });
    let name = s("AXStringForRange");
    let mut out: Ref = std::ptr::null();
    let err = unsafe { AXUIElementCopyParameterizedAttributeValue(el, Retained::as_ptr(&name) as Ref, param.0, &mut out) };
    (err == 0 && !out.is_null()).then(|| Owned(out)).and_then(|o| string(&o))
}

fn is_bullet_masked(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| matches!(c, '•' | '●' | '∙' | '*'))
}

/// Stateful reader for the focused text field: caches label attributes per
/// AXUIElement across polls and reads only a window around the cursor when the
/// app supports `AXStringForRange`.
#[derive(Default)]
pub struct FocusReader {
    last_el: Option<Owned>,
    last_secret_label: bool,
}

impl FocusReader {
    pub fn read(&mut self) -> Option<Field> {
        if unsafe { IsSecureEventInputEnabled() } {
            return Some(Field { secure: true, ..Default::default() });
        }
        let el = focused_element(0.25)?;
        let role = attr(el.0, "AXRole").and_then(|r| string(&r)).unwrap_or_default();
        let subrole = attr(el.0, "AXSubrole").and_then(|r| string(&r)).unwrap_or_default();
        if role == "AXSecureTextField" || subrole == "AXSecureTextField" {
            return Some(Field { secure: true, ..Default::default() });
        }
        if !matches!(role.as_str(), "AXTextField" | "AXTextArea" | "AXComboBox" | "AXSearchField" | "AXWebArea") {
            return None;
        }
        // A password box labelled as one: cache the 4 label reads per focused element.
        let same = self.last_el.as_ref().is_some_and(|prev| unsafe { CFEqual(prev.0, el.0) != 0 });
        let secret_label = if same {
            self.last_secret_label
        } else {
            let label: String = ["AXPlaceholderValue", "AXDescription", "AXTitle", "AXHelp"]
                .iter()
                .filter_map(|a| attr(el.0, a).and_then(|r| string(&r)))
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase();
            let sec = [
                "password", "passcode", "passphrase", "pin", "otp", "one-time", "token", "api key", "secret", "2fa",
                "verification code", "mot de passe", "passwort", "contraseña", "パスワード", "密码",
            ]
            .iter()
            .any(|w| if *w == "pin" || *w == "otp" { label.split(|c: char| !c.is_alphanumeric()).any(|t| t == *w) } else { label.contains(w) });
            self.last_el = Some(Owned(unsafe { CFRetain(el.0) }));
            self.last_secret_label = sec;
            sec
        };
        if secret_label {
            return Some(Field { secure: true, ..Default::default() });
        }
        let mut range = CFRange::default();
        let sel = attr(el.0, "AXSelectedTextRange")?;
        if !unsafe { AXValueGetValue(sel.0, AX_CFRANGE, &mut range as *mut _ as *mut c_void) } {
            return None;
        }
        let at = range.location.max(0) as usize;
        let sel_len = range.length.max(0) as usize;
        const BEFORE_WIN: usize = 2000;
        const SEL_WIN: usize = 400;
        let start = at.saturating_sub(BEFORE_WIN);
        let before_part = if at == 0 {
            Some(String::new())
        } else {
            string_for_range(el.0, start, at - start).filter(|s| !s.is_empty())
        };
        let sel_part = if sel_len == 0 {
            Some(String::new())
        } else {
            string_for_range(el.0, at, sel_len.min(SEL_WIN)).filter(|s| !s.is_empty())
        };
        let (before, selected) = match (before_part, sel_part) {
            (Some(b), Some(s)) => {
                if is_bullet_masked(&b) || is_bullet_masked(&s) {
                    return Some(Field { secure: true, ..Default::default() });
                }
                (b, s)
            }
            _ => {
                let value = attr(el.0, "AXValue").and_then(|r| string(&r))?;
                if is_bullet_masked(&value) {
                    return Some(Field { secure: true, ..Default::default() });
                }
                let units: Vec<u16> = value.encode_utf16().collect();
                let clamped_at = at.min(units.len());
                let end = (clamped_at + sel_len).min(units.len());
                (String::from_utf16_lossy(&units[..clamped_at]), String::from_utf16_lossy(&units[clamped_at..end]))
            }
        };
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
        Some(Field { secure: false, before, caret, frame, selected })
    }
}

/// The field being typed in, None when the focus isn't on editable text.
pub fn focused() -> Option<Field> {
    FocusReader::default().read()
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

#[cfg(test)]
mod tests {
    use super::*;

    const WIN: Rect = Rect { x: 0.0, y: 0.0, w: 1600.0, h: 1000.0 };

    #[test]
    fn a_window_whose_text_was_not_seen_is_not_trusted() {
        assert!(layout_known(12, true));
        assert!(!layout_known(0, true), "no text seen: a Chromium app with its accessibility off");
        assert!(!layout_known(40, false), "a walk that stopped early");
    }

    #[test]
    fn what_a_hit_lands_on_counts_as_content_unless_it_is_backdrop() {
        let small = Some(Rect { x: 100.0, y: 100.0, w: 600.0, h: 60.0 });
        let pane = Some(Rect { x: 0.0, y: 0.0, w: 1500.0, h: 900.0 });
        assert!(hit_is_content("AXStaticText", small, Some(WIN)));
        assert!(hit_is_content("AXLink", small, Some(WIN)));
        assert!(hit_is_content("AXGroup", small, Some(WIN)), "a paragraph: the gap between its lines too");
        assert!(!hit_is_content("AXGroup", pane, Some(WIN)), "a big pane is backdrop");
        assert!(!hit_is_content("AXWebArea", small, Some(WIN)));
        assert!(!hit_is_content("AXScrollArea", pane, Some(WIN)));
        assert!(!hit_is_content("AXWindow", Some(WIN), Some(WIN)));
        assert!(hit_is_content("AXGroup", None, Some(WIN)), "no frame: not sure, so content");
    }

    #[test]
    fn a_word_is_tested_in_its_middle_and_near_its_corners() {
        let r = Rect { x: 100.0, y: 200.0, w: 120.0, h: 30.0 };
        let p = sample_points(r);
        assert_eq!(p[0], (160.0, 215.0));
        for (x, y) in p {
            assert!(x > r.x && x < r.x + r.w && y > r.y && y < r.y + r.h, "({x}, {y}) inside the word");
        }
        assert!(p.iter().any(|&(x, y)| x < 110.0 && y < 210.0), "top left corner");
        assert!(p.iter().any(|&(x, y)| x > 210.0 && y > 220.0), "bottom right corner");
    }
}
