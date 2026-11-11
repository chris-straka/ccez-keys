//! ⌥⏎ in the ChatGPT and Claude apps (owner, 2026-11-11): the English in the
//! message box is put into the first learning language, in place, so the
//! model reads the learning language and the owner sees it written. Plain ⏎
//! still sends as typed. A keyboard event tap catches the key (Accessibility
//! covers it); the translation runs off the main thread on its own helper.

use std::ffi::c_void;
use std::sync::{Arc, Mutex};

/// The apps it works in (ChatGPT's Mac app has shipped under both ids).
pub const APPS: [&str; 3] = ["com.openai.chat", "com.openai.codex", "com.anthropic.claudefordesktop"];

/// ⌥ alone held, on Return or the keypad's Enter.
pub fn is_swap_key(keycode: i64, flags: u64) -> bool {
    const ALT: u64 = 0x0008_0000;
    const OTHERS: u64 = 0x0002_0000 | 0x0004_0000 | 0x0010_0000; // shift, control, command
    matches!(keycode, 36 | 76) && flags & ALT != 0 && flags & OTHERS == 0
}

type Ref = *const c_void;
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventTapCreate(tap: u32, place: u32, options: u32, mask: u64, callback: extern "C" fn(Ref, u32, Ref, *mut c_void) -> Ref, info: *mut c_void) -> Ref;
    fn CGEventTapEnable(tap: Ref, enable: bool);
    fn CGEventGetIntegerValueField(event: Ref, field: u32) -> i64;
    fn CGEventGetDoubleValueField(event: Ref, field: u32) -> f64;
    fn CGEventGetFlags(event: Ref) -> u64;
}
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFMachPortCreateRunLoopSource(alloc: Ref, port: Ref, order: isize) -> Ref;
    fn CFRunLoopGetMain() -> Ref;
    fn CFRunLoopAddSource(rl: Ref, source: Ref, mode: Ref);
    static kCFRunLoopCommonModes: Ref;
}

struct Tap {
    port: Ref,
    on: Box<dyn Fn() -> bool>,
    fire: Box<dyn Fn()>,
}

extern "C" fn callback(_proxy: Ref, kind: u32, event: Ref, info: *mut c_void) -> Ref {
    let tap = unsafe { &*(info as *const Tap) };
    // Turned off by the system for a slow answer, or by input: back on.
    if kind == 0xFFFF_FFFE || kind == 0xFFFF_FFFF {
        unsafe { CGEventTapEnable(tap.port, true) };
        return event;
    }
    // Key down only (10); the keycode field is 9.
    if kind == 10 && is_swap_key(unsafe { CGEventGetIntegerValueField(event, 9) }, unsafe { CGEventGetFlags(event) }) && (tap.on)() {
        (tap.fire)();
        return std::ptr::null();
    }
    event
}

struct ScrollTap {
    port: Ref,
    on: Box<dyn Fn() -> bool>,
    step: Box<dyn Fn(f64, bool)>,
}

extern "C" fn scroll_callback(_proxy: Ref, kind: u32, event: Ref, info: *mut c_void) -> Ref {
    let tap = unsafe { &*(info as *const ScrollTap) };
    if kind == 0xFFFF_FFFE || kind == 0xFFFF_FFFF {
        unsafe { CGEventTapEnable(tap.port, true) };
        return event;
    }
    const SHIFT: u64 = 0x0002_0000;
    // A scroll (22) with Shift held, while the preview is up.
    if kind != 22 || unsafe { CGEventGetFlags(event) } & SHIFT == 0 || !(tap.on)() {
        return event;
    }
    // Precise (trackpad) scrolls count points, wheels lines; either axis
    // (macOS may already have turned a Shift + wheel sideways).
    let precise = unsafe { CGEventGetIntegerValueField(event, 88) } != 0;
    let (a, b) = if precise {
        (unsafe { CGEventGetDoubleValueField(event, 96) }, unsafe { CGEventGetDoubleValueField(event, 97) })
    } else {
        (unsafe { CGEventGetIntegerValueField(event, 11) } as f64, unsafe { CGEventGetIntegerValueField(event, 12) } as f64)
    };
    (tap.step)(if a.abs() >= b.abs() { a } else { b }, precise);
    std::ptr::null()
}

/// Watches scrolls on the main run loop: with Shift held and `on` true,
/// `step` gets the amount and the scroll goes no further.
pub fn scroll_tap(on: impl Fn() -> bool + 'static, step: impl Fn(f64, bool) + 'static) -> bool {
    let tap = Box::leak(Box::new(ScrollTap { port: std::ptr::null(), on: Box::new(on), step: Box::new(step) }));
    let port = unsafe { CGEventTapCreate(1, 0, 0, 1 << 22, scroll_callback, tap as *mut ScrollTap as *mut c_void) };
    if port.is_null() {
        return false;
    }
    tap.port = port;
    unsafe {
        let source = CFMachPortCreateRunLoopSource(std::ptr::null(), port, 0);
        CFRunLoopAddSource(CFRunLoopGetMain(), source, kCFRunLoopCommonModes);
        CGEventTapEnable(port, true);
    }
    true
}

/// Starts watching for ⌥⏎ on the main run loop: `on` says whether it applies
/// now (the app in front, the switch); `fire` does the swap.
pub fn start(on: impl Fn() -> bool + 'static, fire: impl Fn() + 'static) -> bool {
    let tap = Box::leak(Box::new(Tap { port: std::ptr::null(), on: Box::new(on), fire: Box::new(fire) }));
    // Session tap, at the head, able to drop events; key downs.
    let port = unsafe { CGEventTapCreate(1, 0, 0, 1 << 10, callback, tap as *mut Tap as *mut c_void) };
    if port.is_null() {
        return false;
    }
    tap.port = port;
    unsafe {
        let source = CFMachPortCreateRunLoopSource(std::ptr::null(), port, 0);
        CFRunLoopAddSource(CFRunLoopGetMain(), source, kCFRunLoopCommonModes);
        CGEventTapEnable(port, true);
    }
    true
}

/// The swaps' own translator: started at launch and kept running, its
/// language's model loaded by a first word, so a press answers in a moment
/// (owner, 2026-11-11: starting one per press was slow).
pub struct Swapper {
    tx: std::sync::mpsc::Sender<String>,
}

impl Swapper {
    pub fn start(to: String, done: Arc<Mutex<Option<String>>>, log: fn(&str, &str)) -> Swapper {
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        std::thread::spawn(move || {
            use crate::worker::Translator;
            let mut helper = crate::apple::Helper::start();
            if let Some(h) = helper.as_mut() {
                let _ = h.translate("en", &to, "Hello");
            }
            for _ in rx {
                if helper.is_none() {
                    helper = crate::apple::Helper::start();
                }
                let Some(h) = helper.as_mut() else {
                    log("warn", "swap: no translator");
                    continue;
                };
                if !swap(h, &to, &done, log) {
                    // A broken helper is started again on the next press.
                    helper = None;
                }
            }
        });
        Swapper { tx }
    }

    pub fn press(&self) {
        let _ = self.tx.send(String::new());
    }
}

/// The swap itself: the box's text into `to`, put back in its place. `done`
/// gets the note for the toast. False when the translator broke.
fn swap(h: &mut crate::apple::Helper, to: &str, done: &Arc<Mutex<Option<String>>>, log: fn(&str, &str)) -> bool {
    use crate::worker::Translator;
    {
        let Some(text) = crate::ax::field_text().filter(|t| !t.trim().is_empty()) else { return true };
        if ccez_keys_core::text::looks_secret(text.clone()) {
            return true;
        }
        let out = match h.translate("en", to, &text) {
            Ok((t, _)) => t,
            Err(e) => {
                log("warn", &format!("swap into {to}: {e}"));
                *done.lock().unwrap() = Some("Couldn't translate".into());
                return !e.contains("Broken pipe");
            }
        };
        // Still the same text in front: else it moved on and is left alone.
        if crate::ax::field_text().as_deref() != Some(text.as_str()) {
            return true;
        }
        let len = text.encode_utf16().count();
        match crate::ax::replace_range(0, len, &out) {
            crate::ax::Put::Done => {}
            crate::ax::Put::Selected => crate::keys::type_text(&out),
            crate::ax::Put::Nothing => {
                log("info", "swap: the box took no text");
                return true;
            }
        }
        log("info", &format!("swapped the message box into {to}"));
        *done.lock().unwrap() = Some(format!("Put into {}", to.to_uppercase()));
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_option_return() {
        assert!(is_swap_key(36, 0x0008_0000));
        assert!(is_swap_key(76, 0x0008_0100));
        assert!(!is_swap_key(36, 0));
        assert!(!is_swap_key(36, 0x0008_0000 | 0x0010_0000), "⌘⌥⏎ is the app's");
        assert!(!is_swap_key(0, 0x0008_0000));
    }
}
