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
