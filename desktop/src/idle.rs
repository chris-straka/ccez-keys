//! Where learning words can go on screen (float.rs puts them there): the
//! spots the front window has no content in and no other window covers, and
//! whether a video is playing: one that holds the display awake, or one in
//! the front window (Claude's holds nothing). The layout is pure (tested);
//! the system calls are at the foot.

use crate::model::Rect;

/// Spots for words: rectangles a `cols` × `rows` grid's cell in size, at
/// every quarter cell across and half cell down (owner, 2026-11-13: a fixed
/// grid missed the gap between Claude's sidebar and its chat, narrower than
/// two cells), that no window touches. They overlap; float.rs keeps words
/// apart. All in one coordinate space (top-left origin).
pub fn free_cells(screen: Rect, windows: &[Rect], cols: usize, rows: usize) -> Vec<Rect> {
    let (cw, ch) = (screen.w / cols as f64, screen.h / rows as f64);
    let (sx, sy) = (cw / 4.0, ch / 2.0);
    let steps = |span: f64, size: f64, step: f64| ((span - size) / step + 1e-6).floor().max(0.0) as usize + 1;
    let mut out = Vec::new();
    for r in 0..steps(screen.h, ch, sy) {
        for c in 0..steps(screen.w, cw, sx) {
            let cell = Rect { x: screen.x + c as f64 * sx, y: screen.y + r as f64 * sy, w: cw, h: ch };
            let covered = windows.iter().any(|w| w.x < cell.x + cell.w && cell.x < w.x + w.w && w.y < cell.y + cell.h && cell.y < w.y + w.h);
            if !covered {
                out.push(cell);
            }
        }
    }
    out
}

/// Where words stay off the screen: ccez-llm, whose replies are read with
/// their own word cards (the app, `studio.ccez.app`, or its site in a
/// browser, the address of its web page).
pub fn words_stay_off(app: &str, url: Option<&str>) -> bool {
    let host = url.and_then(|u| u.split("://").nth(1)).map(|r| r.split(['/', '?', '#', ':']).next().unwrap_or("")).unwrap_or("");
    app == "studio.ccez.app" || host == "llm.ccez.uk" || host.ends_with(".personalized-llm-client.pages.dev") || host == "personalized-llm-client.pages.dev"
}

/// Free spots on a screen with the front window's content known: inside the
/// front window, cells clear of its `content`; outside it, cells clear of the
/// `others` (other apps' windows). Content 2 points thin or less (a
/// window's edge line, a focus marker) blocks nothing. Top-left
/// coordinates throughout.
pub fn free_spots(screen: Rect, front: Option<Rect>, content: &[Rect], others: &[Rect], cols: usize, rows: usize) -> Vec<Rect> {
    let inside = |c: &Rect, w: &Rect| c.x >= w.x && c.y >= w.y && c.x + c.w <= w.x + w.w && c.y + c.h <= w.y + w.h;
    let hits = |c: &Rect, r: &Rect| r.x < c.x + c.w && c.x < r.x + r.w && r.y < c.y + c.h && c.y < r.y + r.h;
    free_cells(screen, &[], cols, rows)
        .into_iter()
        .filter(|c| match front {
            Some(w) if inside(c, &w) => !content.iter().any(|r| r.w > 2.0 && r.h > 2.0 && hits(c, r)),
            Some(w) if hits(c, &w) => false,
            _ => !others.iter().any(|r| hits(c, r)),
        })
        .collect()
}

/// Where each app's content keeps turning up (owner, 2026-11-11: "learn
/// which areas of my screen change the most depending on the
/// application"): per app, a mark per grid cell that's set when the cell
/// has content and fades a little on every look. A cell free right now but
/// marked lately (a chat column that grows, a feed that refreshes) is no
/// place for a word.
#[derive(Default)]
pub struct Busy {
    maps: std::collections::HashMap<String, Vec<f32>>,
}

/// How much a mark keeps per look (looks come every 0.3–2 s: about two
/// minutes to fade).
const KEEP: f32 = 0.97;
/// Marked above this: avoided.
const AVOID: f32 = 0.2;

impl Busy {
    /// One look at `app`: `all` the grid's cells in order, `free` those
    /// with nothing in them now. The free ones not busy lately.
    pub fn look(&mut self, app: &str, all: &[Rect], free: &[Rect]) -> Vec<Rect> {
        let map = self.maps.entry(app.to_string()).or_insert_with(|| vec![0.0; all.len()]);
        if map.len() != all.len() {
            *map = vec![0.0; all.len()];
        }
        for (i, c) in all.iter().enumerate() {
            map[i] = if free.contains(c) { map[i] * KEEP } else { 1.0 };
        }
        free.iter().filter(|c| all.iter().position(|a| a == *c).is_none_or(|i| map[i] < AVOID)).copied().collect()
    }
}

/// A video's picture is at least this big each way (points).
const VIDEO_MIN: f64 = 72.0;

/// A web page's video with the browser's own controls, as Chromium apps
/// (Claude, Brave) give it to Accessibility: a group whose first child is an
/// empty group over the very same frame (the picture); the controls, while
/// they're up, come after it. Nothing names it a video, and to the layout
/// its picture is one more empty pane, so words went on it (owner,
/// 2026-11-12). `role` and `first` are a childless first child's, `parent`
/// reads its parent's role and frame (asked only when the child fits). The
/// frame of what has that shape; [Videos] says which are videos.
pub fn video_frame(role: &str, first: Rect, parent: impl FnOnce() -> Option<(String, Rect)>) -> Option<Rect> {
    if role != "AXGroup" || first.w < VIDEO_MIN || first.h < VIDEO_MIN {
        return None;
    }
    let (parent_role, frame) = parent()?;
    let near = |a: f64, b: f64| (a - b).abs() < 1.5;
    (parent_role == "AXGroup" && near(frame.x, first.x) && near(frame.y, first.y) && near(frame.w, first.w) && near(frame.h, first.h)).then_some(frame)
}

/// What has a video's shape (video_frame), as one look at a window found it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Video {
    pub frame: Rect,
    /// Its controls are up: its time slider is there. Other things have the
    /// shape (Discord has one); only a video has the slider.
    pub controls: bool,
    /// Its play button says paused (its class "pause": the same in every
    /// language).
    pub paused: bool,
}

/// The videos known in each app. A video's controls stay up while it's
/// paused or over and leave while it plays, when only its shape is left: so
/// a shape counts as a video, playing, only where a video of that size was
/// seen with its controls lately (it's paused before it plays, and the
/// pointer over it brings them back).
#[derive(Default)]
pub struct Videos {
    /// Per app: each one's width and height, and when it was last there.
    known: std::collections::HashMap<String, Vec<(f64, f64, std::time::Instant)>>,
}

/// A video not on screen this long is forgotten.
const VIDEO_KEPT: std::time::Duration = std::time::Duration::from_secs(600);

impl Videos {
    /// One look at `app`'s window, with the shapes `found` in it: its
    /// videos, and whether each is playing.
    pub fn look(&mut self, app: &str, found: &[Video], now: std::time::Instant) -> Vec<(Rect, bool)> {
        let known = self.known.entry(app.to_string()).or_default();
        known.retain(|k| now.duration_since(k.2) < VIDEO_KEPT);
        let mut out = Vec::new();
        for v in found {
            let near = |a: f64, b: f64| (a - b).abs() < 1.5;
            match known.iter_mut().find(|k| near(k.0, v.frame.w) && near(k.1, v.frame.h)) {
                Some(k) => k.2 = now,
                None if v.controls => known.push((v.frame.w, v.frame.h, now)),
                None => continue,
            }
            out.push((v.frame, !v.controls || !v.paused));
        }
        out
    }
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
pub use sys::{idle, video, windows};

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect { x: 0.0, y: 0.0, w: 1200.0, h: 800.0 };

    #[test]
    fn no_words_over_ccez_llm() {
        assert!(words_stay_off("studio.ccez.app", None));
        assert!(words_stay_off("com.brave.Browser", Some("https://llm.ccez.uk/chat/1")));
        assert!(!words_stay_off("com.brave.Browser", Some("https://www.lemonde.fr/")));
        assert!(!words_stay_off("com.anthropic.claudefordesktop", None));
    }

    #[test]
    fn words_go_only_where_no_window_is() {
        // A window over the left half: only right-half cells.
        let cells = free_cells(SCREEN, &[Rect { x: 0.0, y: 0.0, w: 600.0, h: 800.0 }], 4, 2);
        assert!(!cells.is_empty());
        assert!(cells.iter().all(|c| c.x >= 600.0));
        // A full-screen window: nowhere.
        assert!(free_cells(SCREEN, &[SCREEN], 4, 2).is_empty());
        // Quarter cells across (13), half cells down (3).
        assert_eq!(free_cells(SCREEN, &[], 4, 2).len(), 39);
    }

    #[test]
    fn a_gap_narrower_than_two_cells_still_takes_a_word() {
        // Claude on 2026-11-13: sidebar to x 360, chat from 696, a 1920 wide screen.
        let window = Rect { x: 0.0, y: 30.0, w: 1920.0, h: 1050.0 };
        let content = [
            Rect { x: 0.0, y: 30.0, w: 360.0, h: 1050.0 },
            Rect { x: 696.0, y: 30.0, w: 900.0, h: 1050.0 },
            // A window's edge line and a focus marker: nothing.
            Rect { x: 0.0, y: 30.0, w: 1920.0, h: 1.0 },
            Rect { x: 500.0, y: 400.0, w: 2.0, h: 2.0 },
        ];
        let spots = free_spots(window, Some(window), &content, &[], 8, 6);
        assert!(spots.iter().any(|c| c.x >= 360.0 && c.x + c.w <= 696.0));
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

    /// Claude's window with a video in a reply, as read on 2026-11-12.
    const REEL: Rect = Rect { x: 715.0, y: 139.0, w: 457.0, h: 394.0 };

    #[test]
    fn a_video_is_a_group_over_an_empty_group_of_the_same_frame() {
        let group = |f: Rect| move || Some(("AXGroup".to_string(), f));
        assert_eq!(video_frame("AXGroup", REEL, group(REEL)), Some(REEL));
        // A pane inside a bigger group, a picture, a control's icon: not one.
        assert_eq!(video_frame("AXGroup", REEL, group(Rect { h: 900.0, ..REEL })), None);
        assert_eq!(video_frame("AXImage", REEL, group(REEL)), None);
        let icon = Rect { x: 1022.0, y: 461.0, w: 40.0, h: 40.0 };
        assert_eq!(video_frame("AXGroup", icon, group(icon)), None);
        assert_eq!(video_frame("AXGroup", REEL, || Some(("AXWebArea".to_string(), REEL))), None);
        assert_eq!(video_frame("AXGroup", REEL, || None), None);
    }

    #[test]
    fn a_video_counts_once_seen_with_its_controls() {
        let t = std::time::Instant::now();
        let later = |s: u64| t + std::time::Duration::from_secs(s);
        let shape = Video { frame: REEL, controls: false, paused: false };
        let paused = Video { frame: REEL, controls: true, paused: true };
        let mut v = Videos::default();
        // The shape alone (Discord has one): no video.
        assert!(v.look("discord", &[shape], t).is_empty());
        // Paused with its controls up: a video, not playing.
        assert_eq!(v.look("claude", &[paused], t), vec![(REEL, false)]);
        // Play pressed (controls still up), then the controls gone: playing.
        assert_eq!(v.look("claude", &[Video { paused: false, ..paused }], later(2)), vec![(REEL, true)]);
        assert_eq!(v.look("claude", &[shape], later(6)), vec![(REEL, true)]);
        // Scrolled along the page: the same video. Known in Claude only.
        let moved = Video { frame: Rect { y: 300.0, ..REEL }, ..shape };
        assert_eq!(v.look("claude", &[moved], later(8)), vec![(moved.frame, true)]);
        assert!(v.look("discord", &[shape], later(8)).is_empty());
        // Still playing long after: known while it stays; gone a while: forgotten.
        assert_eq!(v.look("claude", &[shape], later(500)).len(), 1);
        assert_eq!(v.look("claude", &[shape], later(1000)).len(), 1);
        assert!(v.look("claude", &[], later(1100)).is_empty());
        assert!(v.look("claude", &[shape], later(1800)).is_empty());
    }

    #[test]
    fn no_word_goes_on_a_video() {
        // Its picture counted as content: every spot left is clear of it.
        let window = Rect { x: 0.0, y: 30.0, w: 1920.0, h: 1050.0 };
        let hits = |c: &Rect| REEL.x < c.x + c.w && c.x < REEL.x + REEL.w && REEL.y < c.y + c.h && c.y < REEL.y + REEL.h;
        assert!(free_spots(window, Some(window), &[], &[], 8, 6).iter().any(hits));
        let spots = free_spots(window, Some(window), &[REEL], &[], 8, 6);
        assert!(!spots.is_empty() && !spots.iter().any(hits));
    }

    #[test]
    fn a_cell_that_had_content_lately_is_avoided_until_it_fades() {
        let all = free_cells(SCREEN, &[], 2, 1);
        let (left, right) = (all[0], *all.last().unwrap());
        let mut b = Busy::default();
        // Content on the right, then gone: the right stays avoided a while.
        assert_eq!(b.look("claude", &all, &[left]), vec![left]);
        assert_eq!(b.look("claude", &all, &[left, right]), vec![left]);
        for _ in 0..60 {
            b.look("claude", &all, &[left, right]);
        }
        assert_eq!(b.look("claude", &all, &[left, right]).len(), 2);
        // Another app has its own map.
        assert_eq!(b.look("notes", &all, &[left, right]).len(), 2);
    }
}
