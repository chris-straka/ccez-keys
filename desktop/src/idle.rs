//! Where learning words can go on screen (float.rs puts them there): the
//! spots the front window has no content in and no other window covers, and
//! whether a video holds the display awake. The layout is pure (tested); the
//! system calls are at the foot.

use crate::model::Rect;

/// Spots for words: the cells of a `cols` × `rows` grid over `screen` that no
/// window touches, each as its rectangle, in an order spread over the screen.
/// All in one coordinate space (top-left origin).
pub fn free_cells(screen: Rect, windows: &[Rect], cols: usize, rows: usize) -> Vec<Rect> {
    let (cw, ch) = (screen.w / cols as f64, screen.h / rows as f64);
    let mut out = Vec::new();
    for r in 0..rows {
        for c in 0..cols {
            let cell = Rect { x: screen.x + c as f64 * cw, y: screen.y + r as f64 * ch, w: cw, h: ch };
            let covered = windows.iter().any(|w| w.x < cell.x + cell.w && cell.x < w.x + w.w && w.y < cell.y + cell.h && cell.y < w.y + w.h);
            if !covered {
                out.push(cell);
            }
        }
    }
    // Every other cell first, so a few words don't bunch in one corner.
    let even = out.iter().step_by(2);
    let odd = out.iter().skip(1).step_by(2);
    even.chain(odd).copied().collect()
}

/// Free spots on a screen with the front window's content known: inside the
/// front window, cells clear of its `content`; outside it, cells clear of the
/// `others` (other apps' windows). Top-left coordinates throughout.
pub fn free_spots(screen: Rect, front: Option<Rect>, content: &[Rect], others: &[Rect], cols: usize, rows: usize) -> Vec<Rect> {
    let inside = |c: &Rect, w: &Rect| c.x >= w.x && c.y >= w.y && c.x + c.w <= w.x + w.w && c.y + c.h <= w.y + w.h;
    let hits = |c: &Rect, r: &Rect| r.x < c.x + c.w && c.x < r.x + r.w && r.y < c.y + c.h && c.y < r.y + r.h;
    free_cells(screen, &[], cols, rows)
        .into_iter()
        .filter(|c| match front {
            Some(w) if inside(c, &w) => !content.iter().any(|r| hits(c, r)),
            Some(w) if hits(c, &w) => false,
            _ => !others.iter().any(|r| hits(c, r)),
        })
        .collect()
}

#[cfg(target_os = "macos")]
mod sys {
    use std::ffi::c_void;

    use objc2::rc::Retained;
    use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString};

    use crate::model::Rect;

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventSourceSecondsSinceLastEventType(state: i32, kind: u32) -> f64;
        fn CGWindowListCopyWindowInfo(option: u32, relative_to: u32) -> *mut c_void;
    }
    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IOPMCopyAssertionsStatus(out: *mut *mut c_void) -> i32;
    }

    /// Time since the last key, click or pointer move, anywhere.
    pub fn idle() -> std::time::Duration {
        // Combined session state, any input event.
        let s = unsafe { CGEventSourceSecondsSinceLastEventType(0, u32::MAX) };
        std::time::Duration::from_secs_f64(s.max(0.0))
    }

    /// A video (or anything else) is keeping the display awake.
    pub fn video() -> bool {
        let mut out: *mut c_void = std::ptr::null_mut();
        if unsafe { IOPMCopyAssertionsStatus(&mut out) } != 0 || out.is_null() {
            return false;
        }
        let Some(d) = (unsafe { Retained::from_raw(out as *mut NSDictionary<NSString, NSNumber>) }) else {
            return false;
        };
        d.objectForKey(&NSString::from_str("PreventUserIdleDisplaySleep")).is_some_and(|n| n.integerValue() > 0)
    }

    /// The on-screen windows of apps (not the desktop, menu bar or Dock), in
    /// screen points from the top-left of the primary screen.
    pub fn windows(skip_pid: i32) -> Vec<Rect> {
        // On screen only, desktop elements left out.
        let raw = unsafe { CGWindowListCopyWindowInfo(1 | 16, 0) };
        let Some(list) = (unsafe { Retained::from_raw(raw as *mut NSArray<NSDictionary<NSString, objc2::runtime::AnyObject>>) }) else {
            return Vec::new();
        };
        let num = |d: &NSDictionary<NSString, objc2::runtime::AnyObject>, k: &str| -> Option<f64> {
            let o = d.objectForKey(&NSString::from_str(k))?;
            o.downcast_ref::<NSNumber>().map(|n| n.doubleValue())
        };
        let mut out = Vec::new();
        for w in list.iter() {
            // Layer 0: ordinary app windows.
            if num(&w, "kCGWindowLayer") != Some(0.0) || num(&w, "kCGWindowOwnerPID") == Some(skip_pid as f64) {
                continue;
            }
            let Some(b) = w.objectForKey(&NSString::from_str("kCGWindowBounds")) else {
                continue;
            };
            let Some(b) = b.downcast_ref::<NSDictionary>() else {
                continue;
            };
            let b: &NSDictionary<NSString, objc2::runtime::AnyObject> = unsafe { std::mem::transmute(b) };
            if let (Some(x), Some(y), Some(w), Some(h)) = (num(b, "X"), num(b, "Y"), num(b, "Width"), num(b, "Height"))
                && w > 40.0
                && h > 40.0
            {
                out.push(Rect { x, y, w, h });
            }
        }
        out
    }
}

#[cfg(target_os = "macos")]
pub use sys::{video, windows};

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect { x: 0.0, y: 0.0, w: 1200.0, h: 800.0 };

    #[test]
    fn words_go_only_where_no_window_is() {
        // A window over the left half: only right-half cells.
        let cells = free_cells(SCREEN, &[Rect { x: 0.0, y: 0.0, w: 600.0, h: 800.0 }], 4, 2);
        assert_eq!(cells.len(), 4);
        assert!(cells.iter().all(|c| c.x >= 600.0));
        // A full-screen window: nowhere.
        assert!(free_cells(SCREEN, &[SCREEN], 4, 2).is_empty());
        assert_eq!(free_cells(SCREEN, &[], 4, 2).len(), 8);
    }

    #[test]
    fn inside_the_front_window_only_its_content_blocks() {
        // A full-screen window with text down its middle column.
        let text = [Rect { x: 400.0, y: 0.0, w: 400.0, h: 800.0 }];
        let spots = free_spots(SCREEN, Some(SCREEN), &text, &[], 6, 4);
        assert!(!spots.is_empty());
        assert!(spots.iter().all(|c| c.x + c.w <= 400.0 || c.x >= 800.0));
        // Half on the window's edge: never.
        let half = Rect { x: 0.0, y: 0.0, w: 700.0, h: 800.0 };
        let spots = free_spots(SCREEN, Some(half), &[], &[], 6, 4);
        assert!(spots.iter().all(|c| c.x + c.w <= 700.0 || c.x >= 700.0));
    }
}
