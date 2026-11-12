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

const ENDS: &[char] = &['.', '!', '?', '。', '！', '？', '…', '\n'];

/// The sentence being typed: the last one, also when the cursor sits just
/// after its end ("Bonjour. Ça va ? " → "Ça va ?"). Nothing at the start of
/// a line, an empty one too.
#[uniffi::export]
pub fn sentence_of(before: String) -> String {
    // Only the line being typed on: a line break ends what came before, with
    // or without a full stop (Electron and web fields use \r and U+2028/9 too).
    const BREAKS: [char; 4] = ['\n', '\r', '\u{2028}', '\u{2029}'];
    let line = before.rfind(BREAKS).map(|i| &before[i + before[i..].chars().next().map_or(1, char::len_utf8)..]).unwrap_or(&before);
    let t = line.trim_end();
    // Its own end marks, however many ("Attends...", "Vraiment ?!"), and the
    // spaces French puts before them.
    let body = t.trim_end_matches(ENDS).trim_end_matches([' ', '\u{a0}', '\u{202f}']);
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

/// A mark typed just after a space the keyboard put in (after a word
/// picked, or a mark) goes before that space: what replaces the space, or
/// None to type the mark as usual. French puts a no-break space before
/// ! ? ; : ("Salut" + "!" → "Salut ! ", the mark never wrapped onto a line
/// of its own). A quote goes before it only when it closes one: `"` when
/// one is open in the paragraph, `'` after a mark ("Oui." + "'").
/// `before`: the text before the cursor, that space at its end.
#[uniffi::export]
pub fn mark_after_space(lang: String, mark: String, before: String) -> Option<String> {
    if mark.chars().count() != 1 {
        return None;
    }
    let m = mark.chars().next()?;
    let closes = match m {
        '.' | ',' | '!' | '?' | ';' | ':' | ')' | ']' | '}' | '»' | '”' | '’' => true,
        '"' => paragraph(&before).matches('"').count() % 2 == 1,
        '\'' => before.trim_end().chars().last().is_some_and(|c| ".,!?…".contains(c)),
        _ => false,
    };
    if !closes {
        return None;
    }
    let french = lang == "fr" && "!?;:".contains(m);
    Some(format!("{}{} ", if french { "\u{a0}" } else { "" }, mark))
}

/// The line the cursor is on.
fn paragraph(before: &str) -> &str {
    before.rsplit(['\n', '\r', '\u{2028}', '\u{2029}']).next().unwrap_or("")
}

/// Whether a space goes in after the mark just typed (the end of
/// `before`): after . , ! ? and closing ) ] } » ” ’. A full stop or comma
/// only straight after text (".5" stays); ! and ? after a space too ("Ça
/// va ?"). A `"` when it closes a quote open in the paragraph.
#[uniffi::export]
pub fn space_after_mark(before: String) -> bool {
    let mut it = before.chars().rev();
    let (Some(m), Some(prev)) = (it.next(), it.next()) else { return false };
    let after_text = !prev.is_whitespace();
    match m {
        '.' | ',' => after_text,
        '!' | '?' | ')' | ']' | '}' | '»' | '”' | '’' => true,
        '"' => after_text && paragraph(&before).matches('"').count() % 2 == 0,
        _ => false,
    }
}

/// A digit typed after the space put in after "3." or "1,": the space
/// goes ("3.14", "1,5").
#[uniffi::export]
pub fn digit_joins(before: String) -> bool {
    let cs: Vec<char> = before.chars().rev().take(3).collect();
    matches!(cs.as_slice(), [' ', '.' | ',' | ':', d] if d.is_ascii_digit())
}

/// Marks that may open a sentence, before its first letter ("¿", "« ").
const OPENING: &[char] = &['¿', '¡', '(', '[', '{', '«', '"', '\'', '“', '‘', '„', '‚', '-', '—', '–'];
/// Marks that may close one, after its end mark ("Non !» ", "(Oui.) ").
const CLOSING: &[char] = &[')', ']', '}', '»', '"', '\'', '”', '’'];

/// Whether the next letter starts a sentence, so takes a capital: at the
/// start of the text or of a line, or after a sentence's end and a space
/// ("Oui. ", "Vraiment ?! ", "« Non ! » ", "Hola. ¿"). Not straight after
/// a mark (no space yet), nor after an abbreviation's dots ("z.B. ").
#[uniffi::export]
pub fn starts_sentence(before: String) -> bool {
    let cs: Vec<char> = before.chars().collect();
    let mut i = cs.len();
    while i > 0 && OPENING.contains(&cs[i - 1]) {
        i -= 1;
    }
    let opened = i;
    while i > 0 && matches!(cs[i - 1], ' ' | '\t' | '\u{a0}' | '\u{202f}') {
        i -= 1;
    }
    if i == 0 || matches!(cs[i - 1], '\n' | '\r' | '\u{2028}' | '\u{2029}') {
        return true;
    }
    if i == opened {
        return false;
    }
    // Closing marks, and the spaces French puts inside them ("Non ! » ").
    while i > 0 && (CLOSING.contains(&cs[i - 1]) || matches!(cs[i - 1], ' ' | '\u{a0}' | '\u{202f}')) {
        i -= 1;
    }
    let Some(&end) = i.checked_sub(1).map(|k| &cs[k]) else { return false };
    if !matches!(end, '.' | '!' | '?' | '…') {
        return false;
    }
    if end == '.' {
        // "z.B.", "e.g.": a dot earlier in the same word.
        let mut k = i - 1;
        while k > 0 && cs[k - 1].is_alphabetic() {
            k -= 1;
        }
        if k > 0 && k < i - 1 && cs[k - 1] == '.' {
            return false;
        }
    }
    true
}

/// Android's field kinds (InputType, EditorInfo), as the keyboard is told them.
mod field {
    pub const CLASS: i32 = 0xf;
    pub const CLASS_TEXT: i32 = 1;
    pub const VARIATION: i32 = 0xff0;
    pub const NORMAL: i32 = 0x00;
    pub const EMAIL_SUBJECT: i32 = 0x30;
    pub const SHORT_MESSAGE: i32 = 0x40;
    pub const LONG_MESSAGE: i32 = 0x50;
    pub const PASSWORD: i32 = 0x80;
    pub const VISIBLE_PASSWORD: i32 = 0x90;
    pub const WEB_EDIT_TEXT: i32 = 0xa0;
    pub const WEB_PASSWORD: i32 = 0xe0;
    pub const CAP_SENTENCES: i32 = 0x4000;
    pub const MULTI_LINE: i32 = 0x20000;
    pub const NO_SUGGESTIONS: i32 = 0x80000;
    pub const ACTION: i32 = 0xff;
    pub const ACTION_SEARCH: i32 = 3;
    pub const NO_ENTER_ACTION: i32 = 0x4000_0000;
}

/// "Search" in a field's hint, in the languages the keyboard types.
const SEARCH_HINTS: &[&str] = &[
    "search", "recherch", "suche", "busca", "cerca", "pesquis", "zoek", "sök", "søg", "szukaj", "hledat", "поиск", "пошук", "検索", "搜索",
    "검색", "αναζήτ", "بحث", "חיפוש", "खोज",
];

/// A search word in the hint, at a word's start ("Search…", "Rechercher").
fn hint_searches(hint: &str) -> bool {
    let hint = hint.to_lowercase();
    SEARCH_HINTS.iter().any(|w| hint.match_indices(w).any(|(i, _)| !hint[..i].chars().next_back().is_some_and(char::is_alphabetic)))
}

/// Whether a field gets capitals at the start of its sentences when it
/// doesn't ask for them (owner, 2026-11-12): any field sentences are
/// written in. Several lines (a web comment or message box), a message or
/// an email's subject, and a plain line of text that isn't a search. Never
/// passwords, addresses (web, email), names and postal addresses (their
/// own flags), filters, phonetic fields, searches, numbers, or a plain
/// line with no suggestions (codes, usernames). A field that asks for
/// sentence capitals has them, unless it's a search or a password.
/// Capitals for each word or every letter are the field's own flags,
/// read by the keyboard as before.
#[uniffi::export]
pub fn sentence_capitals(input_type: i32, ime_options: i32, hint: String) -> bool {
    use field::*;
    if input_type & CLASS != CLASS_TEXT {
        return false;
    }
    let variation = input_type & VARIATION;
    if matches!(variation, PASSWORD | VISIBLE_PASSWORD | WEB_PASSWORD) {
        return false;
    }
    // A search, even one that asks: Chrome asks for sentence capitals in every field of a page.
    if ime_options & ACTION == ACTION_SEARCH || hint_searches(&hint) {
        return false;
    }
    if input_type & CAP_SENTENCES != 0 {
        return true;
    }
    if !matches!(variation, NORMAL | EMAIL_SUBJECT | SHORT_MESSAGE | LONG_MESSAGE | WEB_EDIT_TEXT) {
        return false;
    }
    if input_type & MULTI_LINE != 0 || ime_options & NO_ENTER_ACTION != 0 || variation != NORMAL && variation != WEB_EDIT_TEXT {
        return true;
    }
    input_type & NO_SUGGESTIONS == 0
}

/// Text that looks like a password, an API key or a token: never shown,
/// translated or sent to the AI. A long run of letters and digits mixed,
/// or a known key prefix.
#[uniffi::export]
pub fn looks_secret(text: String) -> bool {
    const PREFIXES: &[&str] = &["sk-", "sk_", "pk_", "ghp_", "gho_", "ghs_", "github_pat_", "xox", "AKIA", "AIza", "eyJ", "glpat-", "-----BEGIN"];
    text.split_whitespace().any(|t| {
        let t = t.trim_matches(|c: char| "\"'`,;()[]{}<>".contains(c));
        if PREFIXES.iter().any(|p| t.starts_with(p)) && t.len() >= 12 {
            return true;
        }
        let n = t.chars().count();
        let digits = t.chars().filter(char::is_ascii_digit).count();
        let letters = t.chars().filter(char::is_ascii_alphabetic).count();
        let upper = t.chars().filter(char::is_ascii_uppercase).count();
        // Long and unbroken, letters mixed with digits (or odd capitals), as keys and passwords are.
        n >= 16 && t.is_ascii() && !t.contains("://") && letters > 0 && (digits >= 2 || (upper >= 3 && upper < letters)) && t.chars().all(|c| c.is_ascii_graphic())
    })
}

/// Words a dictionary meaning uses when it only names its word's form.
const FORM_WORDS: [&str; 14] = [
    "plural", "singular", "feminine", "masculine", "inflection", "form", "participle", "tense", "conjugation", "spelling", "variant",
    "alternative", "abbreviation", "diminutive",
];

/// Every word such a note may use before its "of" (Wiktionary's grammar
/// labels): anything else ("a form of government") is a real meaning.
const GRAMMAR_WORDS: &[&str] = &[
    "plural", "singular", "dual", "feminine", "masculine", "neuter", "common", "inflection", "inflected", "form", "forms", "participle",
    "past", "present", "future", "perfect", "imperfect", "pluperfect", "preterite", "aorist", "tense", "conjugation", "conjugated",
    "spelling", "variant", "alternative", "abbreviation", "diminutive", "augmentative", "first", "second", "third", "first-person",
    "second-person", "third-person", "person", "indicative", "subjunctive", "conditional", "imperative", "infinitive", "gerund",
    "nominative", "accusative", "dative", "genitive", "vocative", "instrumental", "locative", "definite", "indefinite", "comparative",
    "superlative", "short", "long", "strong", "weak", "mixed", "attributive", "predicative", "obsolete", "archaic", "dated", "rare",
    "nonstandard", "informal", "formal", "polite", "familiar", "eye", "dialectal", "simple", "compound", "active", "passive",
    "affirmative", "negative", "contraction", "clipping", "misspelling", "romanization", "apocopic", "elided", "stem", "and", "or",
];

/// A meaning that only names the word's form, as a short tag: "participle
/// past of unterhalten" → "⏪ participle · unterhalten"; "" for a real
/// meaning ("to entertain").
#[uniffi::export]
pub fn form_tag(meaning: String) -> String {
    let t = meaning.trim();
    // A leading note in brackets ("(archaic) …") isn't part of it.
    let t = if t.starts_with('(') { t.find(") ").map_or(t, |i| &t[i + 2..]) } else { t };
    let Some(of) = t.find(" of ") else { return String::new() };
    let grammar = t[..of].to_lowercase();
    // A form word, and only grammar words, in the same clause.
    let words: Vec<&str> = grammar.split(|c: char| !c.is_alphanumeric() && c != '-').filter(|w| !w.is_empty()).collect();
    if grammar.contains(';') || !words.iter().any(|w| FORM_WORDS.contains(w)) || !words.iter().all(|w| GRAMMAR_WORDS.contains(w)) {
        return String::new();
    }
    let Some(lemma) = t[of + 4..].split_whitespace().next().map(|w| w.trim_end_matches(['.', ',', ';', ':'])) else { return String::new() };
    let mut grammar = grammar;
    // Wiktionary's tag order ("participle past") as said ("past participle").
    for tense in ["past", "present", "perfect", "future"] {
        grammar = grammar.replace(&format!("participle {tense}"), &format!("{tense} participle"));
    }
    format!("{} · {lemma}", short_grammar(&grammar))
}

/// Grammar words short, as every app draws them (owner, 2026-10-24, from
/// ccez-llm): persons 1st 2nd 3rd, singular 👤, plural 👨‍👨‍👧‍👦, present ▶,
/// past ⏪, future ⏩, continuous 🔁; "indicative", the default mood, goes.
/// "third-person singular imperfect indicative" → "3rd 👤 imperfect".
pub fn short_grammar(grammar: &str) -> String {
    const SHORT: [(&str, &str); 11] = [
        ("first-person", "1st"),
        ("second-person", "2nd"),
        ("third-person", "3rd"),
        ("singular", "👤"),
        ("plural", "👨‍👨‍👧‍👦"),
        ("continuous", "🔁"),
        ("progressive", "🔁"),
        ("present", "▶"),
        ("past", "⏪"),
        ("future", "⏩"),
        ("indicative", ""),
    ];
    let one = |w: &str| SHORT.iter().find(|(long, _)| *long == w).map_or(w.to_string(), |(_, short)| short.to_string());
    // "first/third-person": each side ("1st/3rd").
    let word = |w: &str| match w.split_once('/') {
        Some((a, b)) if b.ends_with("-person") && !a.ends_with("-person") => format!("{}/{}", one(&format!("{a}-person")), one(b)),
        _ => w.split('/').map(one).collect::<Vec<_>>().join("/"),
    };
    grammar.split_whitespace().map(word).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ")
}
