//! What a preview line and a word's card say: the same on every system.
//! The short meaning a word on screen shows lives in the core now
//! (`ccez_keys_core::gloss`), so the phone's screen words say the same thing.

use crate::model::{Row, Tok};
use crate::preview::PENDING;

/// One line per language, in your order: "🇫🇷 je te retrouve …"; Japanese
/// and Chinese with their readings over the words.
pub fn rows(shown: &crate::worker::Shown, p: &crate::prefs::Prefs, flags: &std::collections::HashMap<String, String>) -> Vec<Row> {
    let max = if p.shown == 0 { usize::MAX } else { p.shown };
    let row = |lang: &str, toks: Vec<Tok>| Row { lang: lang.to_string(), flag: flags.get(lang).cloned().unwrap_or_else(|| lang.to_string()), toks };
    let mut rows: Vec<Row> =
        shown.lines.iter().take(max).map(|l| row(&l.lang, l.toks.iter().map(|t| Tok { reading: if p.readings { t.reading.clone() } else { String::new() }, ..t.clone() }).collect())).collect();
    // Languages still coming: a dim "…" in their place, as tall as they'll be.
    for lang in shown.pending.iter().take(max.saturating_sub(rows.len())) {
        let cjk = matches!(lang.as_str(), "ja" | "zh" | "zh-Hans" | "zh-Hant");
        let reading = if cjk && p.readings { " ".to_string() } else { String::new() };
        rows.push(row(lang, vec![Tok { text: "…".into(), reading, word: false, key: PENDING.into(), status: 0 }]));
    }
    rows
}
/// The card's meaning: the dictionary's senses (`dict`, "; " between), the
/// one the word has here (picked by the on-device model) first, on a line
/// of its own.
pub fn card_text(here: Option<&str>, dict: &str) -> String {
    let senses: Vec<&str> = dict.split("; ").filter(|m| !m.is_empty()).collect();
    match here {
        Some(h) if senses.is_empty() => h.to_string(),
        Some(h) => {
            let rest: Vec<&str> = senses.iter().copied().filter(|m| !m.eq_ignore_ascii_case(h)).take(2).collect();
            if rest.is_empty() { h.to_string() } else { format!("{h}\n{}", rest.join("; ")) }
        }
        None => dict.to_string(),
    }
}

/// A word's dictionary senses for the model to pick from: its own, or its
/// word's when it is only a form (up to six; form lines left out).
pub fn card_senses(engine: &ccez_keys_core::Engine, lang: &str, card: Option<&ccez_keys_core::Card>) -> Vec<String> {
    use ccez_keys_core::text::form_tag;
    let Some(c) = card else { return Vec::new() };
    let real = |ms: &[String]| ms.iter().filter(|m| form_tag((*m).clone()).is_empty()).take(6).cloned().collect::<Vec<_>>();
    let own = real(&c.meanings);
    if !own.is_empty() || c.lemma.is_empty() {
        return own;
    }
    engine.word_card(lang.into(), c.lemma.clone(), String::new()).map(|l| real(&l.meanings)).unwrap_or_default()
}

#[cfg(test)]
#[test]
fn the_sense_it_has_here_comes_first() {
    assert_eq!(card_text(Some("lane"), "trace; lead, clue; lane"), "lane\ntrace; lead, clue");
    assert_eq!(card_text(Some("lane"), ""), "lane");
    assert_eq!(card_text(None, "trace; lead, clue"), "trace; lead, clue");
    assert_eq!(card_text(Some("to read"), "to read"), "to read");
}

/// A card's face, as on the phone: the word with its article ("la gare";
/// the form typed when it's a form of another), a small line with its
/// reading and form ("past participle · unterhalten"), and up to three
/// dictionary meanings (its word's, when the form has none of its own).
pub fn card_face(engine: &ccez_keys_core::Engine, lang: &str, tok: &crate::model::Tok, card: Option<&ccez_keys_core::Card>) -> (String, String, String) {
    use ccez_keys_core::text::form_tag;
    let Some(c) = card else { return (tok.text.clone(), tok.reading.clone(), String::new()) };
    let real = |ms: &[String]| ms.iter().filter(|m| form_tag((*m).clone()).is_empty()).take(3).cloned().collect::<Vec<_>>();
    let mut meanings = real(&c.meanings);
    // Only a form of the card's own word gets the tag ("gegangen" of
    // "gehen"; "gare" is a word of its own, though also a form of "garer").
    let form = (!c.lemma.is_empty() && c.lemma != tok.text).then(|| c.meanings.iter().map(|m| form_tag(m.clone())).find(|t| t.ends_with(&format!(" · {}", c.lemma)))).flatten();
    if meanings.is_empty()
        && !c.lemma.is_empty()
        && c.lemma != tok.text
        && let Some(l) = engine.word_card(lang.into(), c.lemma.clone(), String::new())
    {
        meanings = real(&l.meanings);
    }
    let word = if form.is_some() || c.lemma.is_empty() { tok.text.clone() } else { c.lemma.clone() };
    let head = match c.article.as_str() {
        "" => word,
        a if a.ends_with('\'') => format!("{a}{word}"),
        a => format!("{a} {word}"),
    };
    let reading = if tok.reading.is_empty() { c.reading.clone() } else { tok.reading.clone() };
    let sub: Vec<String> = [Some(reading).filter(|r| !r.is_empty() && *r != tok.text), form].into_iter().flatten().collect();
    (head, sub.join("   "), meanings.join("; "))
}

#[cfg(all(test, unix))]
#[test]
fn card_faces_from_the_dictionaries() {
    // The dictionaries this Mac downloaded, in a scratch folder of their own.
    let dict = crate::paths::data().join("dict");
    if !dict.exists() {
        return;
    }
    let tmp = std::env::temp_dir().join(format!("ccez-keys-card-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&tmp);
    let _ = std::os::unix::fs::symlink(&dict, tmp.join("dict"));
    let e = ccez_keys_core::Engine::new(tmp.to_string_lossy().into_owned());
    let face = |lang: &str, word: &str| {
        let tok = crate::model::Tok { text: word.into(), reading: String::new(), word: true, key: word.into(), status: 0 };
        let c = e.word_card(lang.into(), word.into(), String::new());
        let f = card_face(&e, lang, &tok, c.as_ref());
        eprintln!("{lang} {word}: {f:?}");
        f
    };
    // Only languages whose dictionary this machine has.
    if e.dictionary_ready("fr".into()) {
        let (head, sub, meaning) = face("fr", "gare");
        assert_eq!((head.as_str(), sub.as_str()), ("la gare", ""));
        assert!(!meaning.is_empty());
    }
    if e.dictionary_ready("de".into()) {
        let (head, sub, meaning) = face("de", "gegangen");
        assert_eq!(head, "gegangen");
        assert!(sub.contains("participle"), "{sub}");
        assert!(!meaning.is_empty());
    }
    face("es", "estación");
    face("ja", "駅");
    let _ = std::fs::remove_dir_all(&tmp);
}
