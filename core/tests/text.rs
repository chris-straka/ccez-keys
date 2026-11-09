//! The text-only typing rules (core/src/text.rs).

use ccez_keys_core::text::{form_tag, looks_secret, delete_span, mark_after_space, sentence_of, space_makes_stop, word_before};

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
    assert_eq!(mark_after_space("fr".into(), "!".into()).as_deref(), Some("\u{a0}! "));
    assert_eq!(mark_after_space("fr".into(), ".".into()).as_deref(), Some(". "));
    assert_eq!(mark_after_space("en".into(), "?".into()).as_deref(), Some("? "));
    assert_eq!(mark_after_space("en".into(), "a".into()), None);
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
    assert_eq!(form_tag("participle past of unterhalten".into()), "past participle · unterhalten");
    assert_eq!(form_tag("third-person singular present indicative of être".into()), "third-person singular present indicative · être");
    assert_eq!(form_tag("(archaic) plural of Buch.".into()), "plural · Buch");
    // Real meanings stay meanings.
    assert_eq!(form_tag("to entertain".into()), "");
    assert_eq!(form_tag("the end of the line".into()), "");
    assert_eq!(form_tag("a form; the shape of a thing".into()), "");
}
