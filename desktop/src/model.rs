//! The plain data every platform shares: the field being typed in (as read
//! from the system's accessibility API) and the preview's lines.

/// A rectangle on screen, top-left origin (as accessibility APIs give it), in points.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Where to keep the preview clear of: the cursor and the field it's in.
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct Place {
    pub caret: Option<Rect>,
    pub frame: Option<Rect>,
}

/// The focused text field now.
#[derive(Clone, Debug, Default)]
pub struct Field {
    /// Its role and subrole (AXTextField / AXTextArea; AXSecureTextField for passwords).
    pub secure: bool,
    /// Its text up to the cursor.
    pub before: String,
    /// The cursor on screen (top-left origin, points), when the app says.
    pub caret: Option<Rect>,
    /// The field itself on screen (same coordinates), when the app says.
    pub frame: Option<Rect>,
    /// Text selected in it (empty: just a cursor).
    pub selected: String,
}

/// A word on the preview (or a space or mark between words).
#[derive(Clone, PartialEq, Debug)]
pub struct Tok {
    pub text: String,
    /// Furigana or pinyin to put over it ("" for none).
    pub reading: String,
    /// A word (clickable), not a space or a mark.
    pub word: bool,
    /// Its dictionary form, for its status and card.
    pub key: String,
    /// 0 new, 1 learning, 2 known.
    pub status: u8,
}

/// One language on the preview: its flag and its words.
#[derive(Clone, PartialEq, Debug)]
pub struct Row {
    pub lang: String,
    pub flag: String,
    pub toks: Vec<Tok>,
}

/// Where the cursor likely is when the app won't say (Claude's and other
/// web text boxes): `before` laid out in `frame`'s width, words wrapped as
/// the box wraps them, each line `line_h` tall; the cursor at the end of the
/// last line. `width` measures a run of text in points.
pub fn estimate_caret(frame: Rect, before: &str, width: impl Fn(&str) -> f64, line_h: f64, pad: f64) -> Rect {
    let room = (frame.w - 2.0 * pad).max(40.0);
    let mut lines = 1usize;
    let mut line = String::new();
    for (i, para) in before.split('\n').enumerate() {
        if i > 0 {
            lines += 1;
            line.clear();
        }
        for word in para.split_inclusive(' ') {
            let next = format!("{line}{word}");
            if !line.is_empty() && width(next.trim_end()) > room {
                lines += 1;
                line = word.to_string();
            } else {
                line = next;
            }
        }
    }
    let x = frame.x + pad + width(&line).min(room);
    let y = frame.y + pad + (lines - 1) as f64 * line_h;
    Rect { x, y, w: 2.0, h: line_h }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cursor_is_guessed_at_the_end_of_the_wrapped_text() {
        let frame = Rect { x: 100.0, y: 500.0, w: 208.0, h: 60.0 };
        // 10 pt a character, a 200-point line.
        let w = |s: &str| s.chars().count() as f64 * 10.0;
        let one = estimate_caret(frame, "Je pense", w, 20.0, 4.0);
        assert_eq!((one.x, one.y), (100.0 + 4.0 + 80.0, 504.0));
        // Wraps after "lorem ipsum dolor " (18 chars = 180 pt).
        let two = estimate_caret(frame, "lorem ipsum dolor sit amet", w, 20.0, 4.0);
        assert_eq!((two.x, two.y), (100.0 + 4.0 + 80.0, 524.0));
        let nl = estimate_caret(frame, "a\nbc", w, 20.0, 4.0);
        assert_eq!((nl.x, nl.y), (100.0 + 4.0 + 20.0, 524.0));
    }
}
