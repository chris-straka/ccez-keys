//! The text-only typing rules (core/src/text.rs).

use ccez_keys_core::text::{delete_span, digit_joins, form_tag, looks_secret, mark_after_space, sentence_of, space_after_mark, space_makes_stop, starts_sentence, word_before};

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
