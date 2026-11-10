//! Keys sent to the app in front, as if typed: for putting a translation in
//! place of the sentence where the app won't take it through Accessibility.

use std::ffi::c_void;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventCreateKeyboardEvent(source: *const c_void, key: u16, down: bool) -> *mut c_void;
    fn CGEventKeyboardSetUnicodeString(event: *mut c_void, len: usize, chars: *const u16);
    fn CGEventPost(tap: u32, event: *mut c_void);
    fn CFRelease(r: *const c_void);
}

const DELETE: u16 = 51;

fn key(code: u16, chars: Option<&[u16]>) {
    for down in [true, false] {
        unsafe {
            let e = CGEventCreateKeyboardEvent(std::ptr::null(), code, down);
            if let Some(c) = chars {
                CGEventKeyboardSetUnicodeString(e, c.len(), c.as_ptr());
            }
            CGEventPost(0, e);
            CFRelease(e);
        }
    }
}

/// `n` presses of Delete.
pub fn delete(n: usize) {
    for _ in 0..n {
        key(DELETE, None);
        std::thread::sleep(std::time::Duration::from_millis(4));
    }
}

/// `text` typed, a few characters per event (a key event carries at most 20 UTF-16 units).
pub fn type_text(text: &str) {
    let mut chunk: Vec<u16> = Vec::new();
    for ch in text.chars() {
        let mut buf = [0u16; 2];
        let u = ch.encode_utf16(&mut buf);
        if chunk.len() + u.len() > 16 {
            key(0, Some(&chunk));
            chunk.clear();
            std::thread::sleep(std::time::Duration::from_millis(4));
        }
        chunk.extend_from_slice(u);
    }
    if !chunk.is_empty() {
        key(0, Some(&chunk));
    }
}
