//! The typing rules that only look at text: what the sentence being typed
//! is, the word before the cursor, how much a held delete takes, when two
//! spaces make a full stop, how a mark goes after a word the keyboard
//! spaced. The keyboard reads the text before the cursor and asks these.

/// A letter, digit, apostrophe, hyphen, or a mark that belongs to a letter
/// (Hindi vowel signs, Thai marks, the joiners): part of a word.
pub fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '\'' | '’' | '-' | '\u{200C}' | '\u{200D}') || is_mark(c)
}

fn is_mark(c: char) -> bool {
    let u = c as u32;
    (0x0300..=0x036F).contains(&u)
        || (0x0591..=0x05C7).contains(&u)
        || (0x0610..=0x061A).contains(&u)
        || (0x064B..=0x065F).contains(&u)
        || (0x0900..=0x0903).contains(&u)
        || (0x093A..=0x094F).contains(&u)
        || (0x0951..=0x0957).contains(&u)
        || (0x0962..=0x0963).contains(&u)
        || (0x0E31..=0x0E31).contains(&u)
        || (0x0E34..=0x0E3A).contains(&u)
        || (0x0E47..=0x0E4E).contains(&u)
}

const ENDS: &[char] = &['.', '!', '?', '。', '！', '？', '\n'];

/// The sentence being typed: the last one, also when the cursor sits just
/// after its end ("Bonjour. Ça va ? " → "Ça va ?"). Nothing at the start of
/// a line, an empty one too.
#[uniffi::export]
pub fn sentence_of(before: String) -> String {
    if before.trim_end_matches([' ', '\t']).ends_with('\n') {
        return String::new();
    }
    let t = before.trim_end();
    let body = match t.chars().last() {
        Some(c) if ENDS.contains(&c) => &t[..t.len() - c.len_utf8()],
        _ => t,
    };
    let start = body.rfind(|c| ENDS.contains(&c)).map(|i| i + body[i..].chars().next().map_or(1, char::len_utf8)).unwrap_or(0);
    t[start..].trim().to_string()
}

/// The word before the cursor, leaving out the last `skip` characters (the
/// word being typed): "j'aime l'été" → "l'été".
#[uniffi::export]
pub fn word_before(before: String, skip: u32) -> String {
    let chars: Vec<char> = before.chars().collect();
    let keep = chars.len().saturating_sub(skip as usize);
    let t: String = chars[..keep].iter().collect();
    let t = t.trim_end();
    let start = t.char_indices().rev().take_while(|(_, c)| is_word_char(*c)).last().map(|(i, _)| i).unwrap_or(t.len());
    t[start..].to_string()
}

/// How many characters a held delete takes: the spaces before the cursor,
/// then the word before them, or else the run of marks ("c'est-à-dire  "
/// → all of it; "Salut !! " → "!! ").
#[uniffi::export]
pub fn delete_span(before: String) -> u32 {
    let trimmed = before.trim_end();
    let spaces = before.chars().count() - trimmed.chars().count();
    let body = match trimmed.chars().last() {
        None => 0,
        Some(c) if is_word_char(c) => trimmed.chars().rev().take_while(|c| is_word_char(*c)).count(),
        Some(_) => trimmed.chars().rev().take_while(|c| !is_word_char(*c) && !c.is_whitespace()).count(),
    };
    (spaces + body) as u32
}

/// Two spaces make a full stop only after a letter or a digit ("ok  " →
/// "ok. "; not after "?" or another space).
#[uniffi::export]
pub fn space_makes_stop(before: String) -> bool {
    let mut it = before.chars().rev();
    matches!((it.next(), it.next()), (Some(' '), Some(c)) if c.is_alphanumeric())
}

/// A mark typed just after a word the keyboard spaced goes before that
/// space: what replaces the space, or None to type the mark as usual.
/// French puts a no-break space before ! ? ; : ("Salut" + "!" → "Salut ! ",
/// the mark never wrapped onto a line of its own).
#[uniffi::export]
pub fn mark_after_space(lang: String, mark: String) -> Option<String> {
    if mark.chars().count() != 1 || !".,!?;:)".contains(mark.as_str()) {
        return None;
    }
    let french = lang == "fr" && "!?;:".contains(mark.as_str());
    Some(format!("{}{} ", if french { "\u{a0}" } else { "" }, mark))
}
