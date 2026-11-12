//! The text-only typing rules (core/src/text.rs).

use ccez_keys_core::text::{delete_span, digit_joins, form_tag, looks_secret, mark_after_space, sentence_of, space_after_mark, space_makes_stop, sentence_capitals, starts_sentence, word_before};

#[test]
fn the_sentence_typed() {
    assert_eq!(sentence_of("Bonjour. Ça va ? ".into()), "Ça va ?");
    assert_eq!(sentence_of("Bonjour. Ça va".into()), "Ça va");
    assert_eq!(sentence_of("今日は。元気".into()), "元気");
    assert_eq!(sentence_of("Hi!\nNew line".into()), "New line");
    // The start of a line, an empty one too: nothing.
    assert_eq!(sentence_of("First line.\n".into()), "");
    assert_eq!(sentence_of("First line.\n\n".into()), "");
    assert_eq!(sentence_of("".into()), "");
    // Any line break ends the line before, full stop or not.
    assert_eq!(sentence_of("First line\nNew".into()), "New");
    assert_eq!(sentence_of("First line.\r".into()), "");
    assert_eq!(sentence_of("First line.\r\n  ".into()), "");
    assert_eq!(sentence_of("First line\u{2029}".into()), "");
    assert_eq!(sentence_of("First line.\n   \t".into()), "");
    // However many end marks it has, the sentence is the sentence.
    assert_eq!(sentence_of("Attends...".into()), "Attends...");
    assert_eq!(sentence_of("Bon. Vraiment ?!".into()), "Vraiment ?!");
    assert_eq!(sentence_of("Hein ?? ".into()), "Hein ??");
    assert_eq!(sentence_of("Oui… Et toi".into()), "Et toi");
}

#[test]
fn the_word_before() {
    assert_eq!(word_before("j'aime l'été ".into(), 0), "l'été");
    assert_eq!(word_before("aujourd'hui il".into(), 2), "aujourd'hui");
    assert_eq!(word_before("नमस्ते ".into(), 0), "नमस्ते");
    assert_eq!(word_before("hello, ".into(), 0), "");
    assert_eq!(word_before("".into(), 3), "");
}

#[test]
fn held_delete_takes() {
    assert_eq!(delete_span("c'est-à-dire  ".into()), 14);
    assert_eq!(delete_span("Salut !! ".into()), 3);
    assert_eq!(delete_span("deux mots".into()), 4);
    assert_eq!(delete_span("".into()), 0);
}

#[test]
fn two_spaces() {
    assert!(space_makes_stop("ok ".into()));
    assert!(space_makes_stop("à 10 ".into()));
    assert!(!space_makes_stop("vraiment? ".into()));
    assert!(!space_makes_stop("  ".into()));
}

#[test]
fn marks_after_a_spaced_word() {
    let m = |l: &str, mark: &str, before: &str| mark_after_space(l.into(), mark.into(), before.into());
    assert_eq!(m("fr", "!", "Salut ").as_deref(), Some("\u{a0}! "));
    assert_eq!(m("fr", ".", "Salut ").as_deref(), Some(". "));
    assert_eq!(m("de", "?", "Wirklich ").as_deref(), Some("? "));
    assert_eq!(m("de", "a", "Wirklich ").as_deref(), None);
    assert_eq!(m("fr", ")", "(oui ").as_deref(), Some(") "));
    // A quote closes one that's open; else it opens one, after the space.
    assert_eq!(m("fr", "\"", "Il a dit \"oui ").as_deref(), Some("\" "));
    assert_eq!(m("fr", "\"", "Il a dit ").as_deref(), None);
    assert_eq!(m("fr", "'", "Il a dit 'oui. ").as_deref(), Some("' "));
    assert_eq!(m("fr", "'", "Il a dit ").as_deref(), None);
}

#[test]
fn a_space_after_marks() {
    for t in ["Oui.", "Oui,", "Quoi?", "Ça va ?", "Non!", "(oui)", "[1]", "{a}", "« oui »", "Il a dit \"oui\""] {
        assert!(space_after_mark(t.into()), "{t}");
    }
    for t in [".", " .", "Il a dit \"", "a-", "Oui:", "Oui;", ""] {
        assert!(!space_after_mark(t.into()), "{t}");
    }
}

#[test]
fn digits_join_after_a_mark() {
    assert!(digit_joins("3. ".into()));
    assert!(digit_joins("1, ".into()));
    assert!(!digit_joins("Oui. ".into()));
    assert!(!digit_joins("3.".into()));
}

#[test]
fn sentence_starts() {
    for t in ["", "Bonjour.\n", "Bonjour\n", "Bonjour. ", "Wirklich?! ", "« Non ! » ", "Hola. ¿", "Oui… ", "(Oui.) ", "Ça va ? ", "Erste Zeile\n  "] {
        assert!(starts_sentence(t.into()), "{t:?}");
    }
    for t in ["Bonjour", "Bonjour ", "Bonjour.", "z.B. ", "Ich, ", "Il a dit « ", "3:"] {
        assert!(!starts_sentence(t.into()), "{t:?}");
    }
}

#[test]
fn secrets_are_never_sent() {
    for t in ["my key is sk-proj-4fJk29dLqP0xZ8vB", "ghp_aB3dE5gH7jK9mN1pQ3", "token eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIx", "Tr0ub4dor&3xK9qZpW7m", "AKIAIOSFODNN7EXAMPLE", "4f9c2a1e8b7d6c5f3a2e1d0c9b8a7f6e"] {
        assert!(looks_secret(t.into()), "{t}");
    }
    for t in ["I'll meet you at the library tomorrow", "internationalization is hard", "see https://example.com/abc123def456ghi", "call 514-555-0182", "Antidisestablishmentarianism"] {
        assert!(!looks_secret(t.into()), "{t}");
    }
}

#[test]
fn form_tags() {
    assert_eq!(form_tag("participle past of unterhalten".into()), "⏪ participle · unterhalten");
    assert_eq!(form_tag("third-person singular present indicative of être".into()), "3rd 👤 ▶ · être");
    assert_eq!(form_tag("(archaic) plural of Buch.".into()), "👨‍👨‍👧‍👦 · Buch");
    assert_eq!(form_tag("third-person singular imperfect indicative of apparaître".into()), "3rd 👤 imperfect · apparaître");
    // Real meanings stay meanings.
    assert_eq!(form_tag("to entertain".into()), "");
    assert_eq!(form_tag("the end of the line".into()), "");
    assert_eq!(form_tag("a form; the shape of a thing".into()), "");
    assert_eq!(form_tag("a form of government".into()), "");
    assert_eq!(form_tag("a variant of chess".into()), "");
    assert_eq!(form_tag("in a tense state of mind".into()), "");
    assert_eq!(form_tag("feminine plural of beau".into()), "feminine 👨‍👨‍👧‍👦 · beau");
    assert_eq!(form_tag("first/third-person singular present subjunctive of aller".into()), "1st/3rd 👤 ▶ subjunctive · aller");
    assert_eq!(form_tag("inflection of garer:".into()), "inflection · garer");
}

/// Android's field kinds, as in text.rs (InputType, EditorInfo).
const TEXT: i32 = 1;
const NUMBER: i32 = 2;
const PHONE: i32 = 3;
const CAP_CHARACTERS: i32 = 0x1000;
const CAP_WORDS: i32 = 0x2000;
const CAP_SENTENCES: i32 = 0x4000;
const MULTI_LINE: i32 = 0x20000;
const NO_SUGGESTIONS: i32 = 0x80000;
const ACTION_GO: i32 = 2;
const ACTION_SEARCH: i32 = 3;
const ACTION_SEND: i32 = 4;
const NO_ENTER_ACTION: i32 = 0x4000_0000;

fn caps(input_type: i32, ime_options: i32) -> bool {
    sentence_capitals(input_type, ime_options, String::new())
}

#[test]
fn capitals_in_fields_written_in() {
    // A web comment box (Chrome's textarea: several lines, no flags).
    assert!(caps(TEXT | 0xa0 | MULTI_LINE, NO_ENTER_ACTION));
    assert!(caps(TEXT | MULTI_LINE, 0));
    assert!(caps(TEXT, NO_ENTER_ACTION));
    // Messages, an email's subject.
    assert!(caps(TEXT | 0x40, ACTION_SEND));
    assert!(caps(TEXT | 0x50, 0));
    assert!(caps(TEXT | 0x30, 0));
    // A plain line of text, in an app or on a web page.
    assert!(caps(TEXT, 0));
    assert!(caps(TEXT | 0xa0, ACTION_GO));
    // A field that asks for them (Chrome's text inputs: 0xc0a1).
    assert!(caps(0xc0a1, ACTION_GO));
    assert!(caps(TEXT | 0x60 | CAP_SENTENCES, 0));
}

#[test]
fn no_capitals_where_nothing_is_written_in_sentences() {
    // Passwords, web and email addresses.
    for v in [0x80, 0x90, 0xe0, 0x10, 0x20, 0xd0] {
        assert!(!caps(TEXT | v, 0), "{v:#x}");
        assert!(!caps(TEXT | v | MULTI_LINE, 0), "{v:#x}");
    }
    for v in [0x80, 0x90, 0xe0] {
        assert!(!caps(TEXT | v | CAP_SENTENCES, 0), "{v:#x}");
    }
    // Names and postal addresses follow their own flags (each word: the keyboard reads those).
    assert!(!caps(TEXT | 0x60, 0));
    assert!(!caps(TEXT | 0x60 | CAP_WORDS, 0));
    assert!(!caps(TEXT | 0x70, 0));
    // Filters, phonetic fields.
    assert!(!caps(TEXT | 0xb0, 0));
    assert!(!caps(TEXT | 0xc0, 0));
    // Searches: by their enter key or their hint.
    assert!(!caps(TEXT, ACTION_SEARCH));
    assert!(!caps(TEXT | 0xa0, ACTION_SEARCH));
    assert!(!caps(TEXT | MULTI_LINE, ACTION_SEARCH));
    // Chrome asks for sentence capitals in a search box too (0x40a1): a search all the same.
    assert!(!caps(0x40a1, 0x1200_0003));
    for hint in ["Search", "Search or type URL", "Rechercher", "Suchen", "Buscar en YouTube", "検索", "Поиск"] {
        assert!(!sentence_capitals(TEXT, 0, hint.into()), "{hint}");
    }
    // A hint that only has the letters inside a word is no search.
    for hint in ["Écrivez un message", "Research notes", "Para ti"] {
        assert!(sentence_capitals(TEXT, 0, hint.into()), "{hint}");
    }
    // Numbers, phones.
    assert!(!caps(NUMBER, 0));
    assert!(!caps(PHONE, 0));
    // A plain line with no suggestions: codes, usernames.
    assert!(!caps(TEXT | NO_SUGGESTIONS, 0));
    assert!(!caps(TEXT | 0xa0 | NO_SUGGESTIONS, 0));
    // Words or every letter alone aren't sentences (the keyboard follows those flags itself).
    assert!(!caps(TEXT | 0x60 | CAP_CHARACTERS, 0));
}
