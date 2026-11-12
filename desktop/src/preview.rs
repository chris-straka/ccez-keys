//! When the preview shows, what it says and when it asks for a translation:
//! the same on every system, ticked a few times a second with what the system
//! reports (the field being typed in, the pointer, a held modifier) and the
//! translations that came back. No system calls here.
//!
//! The owner's rules (docs/mac.md): not while typing; once typing stops, when
//! every language is in, all together (after 0.2 s when they're already in,
//! else 0.5 s); then 8 s (up to 20 s for a long
//! sentence), and while the pointer is over it; gone at once on typing again,
//! leaving the field, a new line or an emptied field. Nothing secret-looking is
//! ever translated or shown.

use std::time::{Duration, Instant};

use ccez_keys_core::text::{looks_secret, sentence_of};

use crate::model::{Field, Place, Row};
use crate::worker::{Ask, Shown};

/// The words of a line still being translated ("…" held in its place).
pub const PENDING: &str = "\u{1}pending";
/// The pause in typing that asks for a translation.
const PAUSE: Duration = Duration::from_millis(200);
/// Typing has stopped this long: the preview may show (never while typing).
const STILL: Duration = Duration::from_millis(500);
/// The pointer was over it this recently: it stays.
const HOVER: Duration = Duration::from_secs(1);

/// How long it stays once it shows: 8 s, and a tenth of a second a character
/// past 40, up to 20 s.
pub fn linger(chars: usize) -> Duration {
    Duration::from_millis(8000 + (chars.saturating_sub(40) as u64 * 100).min(12_000))
}

/// What the system reports this tick.
pub struct Inputs<'a> {
    pub now: Instant,
    /// Paused, or an app where it's off.
    pub off: bool,
    /// The field being typed in (None: not a text field).
    pub field: Option<Field>,
    /// The "show it again" modifier (⌥ on the Mac, Alt elsewhere) is held.
    pub held: bool,
    /// The pointer is over the preview or its card.
    pub under_mouse: bool,
    /// How many languages to show (0: all).
    pub shown: usize,
    /// The owner's languages, in order.
    pub langs: &'a [String],
}

/// What to do this tick.
#[derive(Debug, Default)]
pub struct Outputs {
    /// Put the preview (`line`, at `place`) up; else take it down (and its card).
    pub show: bool,
    /// Ask for this translation.
    pub ask: Option<Ask>,
    /// A new sentence: an open card goes.
    pub close_card: bool,
}

pub struct Preview {
    pub sentence: String,
    pub place: Place,
    /// What's shown, the sentence it translates, and whether every language is in.
    pub line: Vec<Row>,
    pub line_for: String,
    pub complete: bool,
    /// A translation put in place of the sentence: not translated again while
    /// the sentence still starts with it.
    pub swapped: String,
    changed: Instant,
    seq: u64,
    asked: bool,
    asking: String,
    appeared: Instant,
    hovered: Option<Instant>,
}

impl Preview {
    pub fn new(now: Instant) -> Preview {
        Preview {
            sentence: String::new(),
            place: Place::default(),
            line: Vec::new(),
            line_for: String::new(),
            complete: true,
            swapped: String::new(),
            changed: now,
            seq: 0,
            asked: true,
            asking: String::new(),
            appeared: now,
            hovered: None,
        }
    }

    /// Something was clicked on it: it stays a while from now.
    pub fn touch(&mut self, now: Instant) {
        self.appeared = now;
    }

    /// Asked again on the next tick (a word's status changed: its colour).
    pub fn redo(&mut self) {
        self.sentence.clear();
    }

    /// A translation now stands in place of the sentence.
    pub fn swapped_in(&mut self, text: String) {
        self.swapped = text;
        self.line.clear();
    }

    fn drop_answers(&mut self) {
        self.seq += 1;
    }

    pub fn tick(&mut self, i: Inputs, answer: Option<Shown>, render: &dyn Fn(&Shown) -> Vec<Row>) -> Outputs {
        let mut out = Outputs::default();
        if i.off {
            return out;
        }
        let Some(field) = i.field.filter(|f| !f.secure) else {
            self.sentence.clear();
            self.line.clear();
            return out;
        };
        // Text selected: that (owner, 2026-11-11). Else the sentence up to the
        // cursor, still shown after its full stop and a space; a new line or an
        // emptied field starts blank (sentence_of).
        let picked = field.selected.trim();
        let sentence = if picked.is_empty() { sentence_of(field.before.clone()) } else { picked.chars().take(400).collect() };
        // Invisible characters aren't text: Discord's empty box holds a
        // zero-width one, which trim() keeps, and its "sentence" showed as
        // flags with nothing beside them (owner, 2026-11-12).
        let sentence: String = sentence.chars().filter(|c| !invisible(*c)).collect();
        // Just swapped in for a translation (and typed on from there): nothing to show.
        if !self.swapped.is_empty() {
            if sentence.starts_with(self.swapped.as_str()) {
                return out;
            }
            self.swapped.clear();
        }
        // Nothing secret is ever shown or translated: the sentence, or what's near it.
        let tail: String = field.before.chars().rev().take(300).collect::<Vec<_>>().into_iter().rev().collect();
        if sentence.trim().is_empty() || looks_secret(tail) || looks_secret(sentence.clone()) || looks_secret(field.selected.clone()) {
            if !self.sentence.is_empty() {
                // An answer still on its way for the old sentence is dropped.
                self.drop_answers();
            }
            self.sentence.clear();
            // A new or emptied field starts blank, not with the last translation.
            self.line.clear();
            return out;
        }
        if self.sentence != sentence {
            // Another sentence (not this one typed on): the old translation goes at once.
            let n = self.sentence.chars().count().min(sentence.chars().count()).min(12);
            if n == 0 || self.sentence.chars().take(n).ne(sentence.chars().take(n)) {
                self.line.clear();
                out.close_card = true;
            } else {
                // Typed on: lines still waiting were for the old words.
                self.line.retain(|r| !r.toks.iter().any(|t| t.key == PENDING));
            }
            self.sentence = sentence.clone();
            // Answers for what was there before don't belong to this one.
            self.drop_answers();
            self.place = Place { caret: field.caret, frame: field.frame };
            self.changed = i.now;
            self.asked = false;
        } else if field.caret.is_some() || field.frame.is_some() {
            self.place = Place { caret: field.caret, frame: field.frame };
        }
        // Translated once typing pauses a moment, not for every letter.
        if !self.asked && i.now.duration_since(self.changed) >= PAUSE {
            self.asked = true;
            self.complete = false;
            self.drop_answers();
            let langs: Vec<String> = if i.shown == 0 { i.langs.to_vec() } else { i.langs.iter().take(i.shown + 2).cloned().collect() };
            self.asking = sentence.clone();
            out.ask = Some(Ask { seq: self.seq, text: sentence, from: "en".into(), langs, max: if i.shown == 0 { usize::MAX } else { i.shown } });
        }
        if let Some(shown) = answer.filter(|s| s.seq == self.seq) {
            self.line = render(&shown);
            self.line_for = self.asking.clone();
            // It shows when the last language is in: counted from then.
            if shown.pending.is_empty() {
                self.appeared = i.now;
            }
            self.complete = shown.pending.is_empty();
        }
        // Not while typing; once stopped, when every language is in, all
        // together; then a while from when it showed, or while pointed at.
        // Every line already in for this very sentence (from the session's
        // cache, say): it may show once typing pauses, not after the full wait
        // (owner, 2026-11-11: show it sooner when it's ready).
        let ready = self.complete && self.line_for == self.sentence;
        let typing = i.now.duration_since(self.changed) < if ready { PAUSE } else { STILL };
        let recent = i.now.duration_since(self.appeared) < linger(self.sentence.chars().count());
        if i.under_mouse {
            self.hovered = Some(i.now);
        }
        let hovered = self.hovered.is_some_and(|t| i.now.duration_since(t) < HOVER);
        out.show = (((recent || hovered) && !typing && self.complete) || i.held) && !self.line.is_empty();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Tok;
    use crate::worker::Line;

    fn field(before: &str) -> Option<Field> {
        Some(Field { secure: false, before: before.into(), ..Default::default() })
    }

    fn render(s: &Shown) -> Vec<Row> {
        s.lines.iter().map(|l| Row { lang: l.lang.clone(), flag: String::new(), toks: l.toks.clone() }).collect()
    }

    fn answer(seq: u64, text: &str, pending: &[&str]) -> Shown {
        let tok = Tok { text: text.into(), reading: String::new(), word: true, key: text.into(), status: 0 };
        Shown { seq, lines: vec![Line { lang: "fr".into(), toks: vec![tok] }], pending: pending.iter().map(|p| p.to_string()).collect() }
    }

    /// Ticks with `before` typed, `at` ms after the start.
    struct Run {
        p: Preview,
        t0: Instant,
        langs: Vec<String>,
    }

    impl Run {
        fn new() -> Run {
            let t0 = Instant::now();
            Run { p: Preview::new(t0), t0, langs: vec!["fr".into(), "de".into()] }
        }
        fn tick(&mut self, at: u64, before: &str, answer: Option<Shown>) -> Outputs {
            let i = Inputs { now: self.t0 + Duration::from_millis(at), off: false, field: field(before), held: false, under_mouse: false, shown: 6, langs: &self.langs };
            self.p.tick(i, answer, &render)
        }
        fn hover(&mut self, at: u64, before: &str) -> Outputs {
            let i = Inputs { now: self.t0 + Duration::from_millis(at), off: false, field: field(before), held: false, under_mouse: true, shown: 6, langs: &self.langs };
            self.p.tick(i, None, &render)
        }
    }

    #[test]
    fn asks_once_typing_pauses_and_shows_only_when_all_are_in() {
        let mut r = Run::new();
        assert!(r.tick(0, "Je", None).ask.is_none());
        let o = r.tick(250, "Je", None);
        let ask = o.ask.expect("asked after a pause");
        assert_eq!(ask.text, "Je");
        // One language in, one to come: not yet.
        assert!(!r.tick(400, "Je", Some(answer(ask.seq, "I", &["de"]))).show);
        // Every line in once typing has paused: it shows at once.
        assert!(r.tick(450, "Je", Some(answer(ask.seq, "I", &[]))).show);
    }

    #[test]
    fn a_line_still_coming_waits_for_the_full_pause() {
        let mut r = Run::new();
        r.tick(0, "Je", None);
        let ask = r.tick(250, "Je", None).ask.unwrap();
        // Typed on: the line in hand was for "Je", so the full wait holds.
        r.tick(300, "Je", Some(answer(ask.seq, "I", &[])));
        assert!(!r.tick(350, "Je v", None).show);
        assert!(!r.tick(600, "Je v", None).show);
    }

    #[test]
    fn hidden_while_typing_and_answers_for_old_words_dropped() {
        let mut r = Run::new();
        r.tick(0, "Hello", None);
        let ask = r.tick(250, "Hello", None).ask.unwrap();
        r.tick(300, "Hello", Some(answer(ask.seq, "Bonjour", &[])));
        assert!(r.tick(800, "Hello", None).show);
        // A key: hidden, and the late answer for "Hello" isn't taken for "Hello t".
        assert!(!r.tick(900, "Hello t", None).show);
        r.tick(950, "Hello t", Some(answer(ask.seq, "Bonjour", &[])));
        assert_eq!(r.p.line_for, "Hello");
    }

    #[test]
    fn stays_its_time_then_while_pointed_at() {
        let mut r = Run::new();
        r.tick(0, "Hi", None);
        let ask = r.tick(250, "Hi", None).ask.unwrap();
        r.tick(600, "Hi", Some(answer(ask.seq, "Salut", &[])));
        assert!(r.tick(600 + 7_900, "Hi", None).show);
        assert!(r.hover(600 + 7_950, "Hi").show);
        assert!(r.hover(600 + 30_000, "Hi").show, "pointed at, it stays");
        assert!(r.tick(600 + 30_500, "Hi", None).show, "a moment after");
        assert!(!r.tick(600 + 32_000, "Hi", None).show);
    }

    #[test]
    fn a_new_line_or_empty_field_starts_blank() {
        let mut r = Run::new();
        r.tick(0, "Hi", None);
        let ask = r.tick(250, "Hi", None).ask.unwrap();
        r.tick(600, "Hi", Some(answer(ask.seq, "Salut", &[])));
        assert!(r.tick(700, "Hi", None).show);
        assert!(!r.tick(800, "Hi\n", None).show);
        assert!(r.p.line.is_empty());
        r.tick(900, "Hi", None);
        assert!(!r.tick(1000, "", None).show);
        // Discord's empty box: a zero-width character, which is no text.
        let z = r.tick(1100, "\u{FEFF}", None);
        assert!(!z.show && z.ask.is_none());
        assert!(r.tick(1500, "\u{200B}", None).ask.is_none(), "nothing asked for it either");
    }

    #[test]
    fn nothing_secret_is_asked_or_shown() {
        let mut r = Run::new();
        r.tick(0, "my key sk-proj-abcdefghijklmnop1234", None);
        assert!(r.tick(400, "my key sk-proj-abcdefghijklmnop1234", None).ask.is_none());
        let i = Inputs { now: r.t0, off: false, field: Some(Field { secure: true, before: "hunter2".into(), ..Default::default() }), held: false, under_mouse: false, shown: 6, langs: &r.langs };
        assert!(r.p.tick(i, None, &render).ask.is_none());
    }

    #[test]
    fn a_swapped_sentence_isnt_translated_back() {
        let mut r = Run::new();
        r.p.swapped_in("Je vais".into());
        r.tick(0, "Je vais", None);
        assert!(r.tick(400, "Je vais à", None).ask.is_none());
        // Something else typed: back to normal.
        r.tick(500, "Hello", None);
        assert!(r.tick(800, "Hello", None).ask.is_some());
    }

    #[test]
    fn a_selection_is_previewed_and_a_cursor_mid_sentence_previews_up_to_it() {
        let mut r = Run::new();
        let sel = Field { before: "I said hello. ".into(), selected: "the weather is nice".into(), ..Default::default() };
        let i = Inputs { now: r.t0, off: false, field: Some(sel.clone()), held: false, under_mouse: false, shown: 6, langs: &r.langs };
        r.p.tick(i, None, &render);
        let i = Inputs { now: r.t0 + Duration::from_millis(250), off: false, field: Some(sel), held: false, under_mouse: false, shown: 6, langs: &r.langs };
        assert_eq!(r.p.tick(i, None, &render).ask.unwrap().text, "the weather is nice");
        // The cursor placed inside "We went to the beach": up to the cursor.
        r.tick(1000, "Hi. We went to", None);
        assert_eq!(r.tick(1300, "Hi. We went to", None).ask.unwrap().text, "We went to");
    }

    #[test]
    fn longer_sentences_stay_longer() {
        assert_eq!(linger(10), Duration::from_secs(8));
        assert_eq!(linger(140), Duration::from_secs(18));
        assert_eq!(linger(1000), Duration::from_secs(20));
    }
}

/// Characters with no look of their own: zero-width spaces and joiners,
/// direction marks, the byte-order mark.
fn invisible(c: char) -> bool {
    matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2064}' | '\u{FEFF}')
}
