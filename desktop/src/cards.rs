//! What a preview line and a word's card say: the same on every system.

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
/// The card's meaning: what it means here (on-device model) over the dictionary's.
pub fn card_text(here: Option<&str>, dict: &str) -> String {
    match here {
        Some(h) if dict.is_empty() || h.eq_ignore_ascii_case(dict) => h.to_string(),
        Some(h) => format!("{h}\n{dict}"),
        None => dict.to_string(),
    }
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

/// The word a dictionary line only points at ("inflection of pencher: …",
/// "plural of cheval"), when that's all the line is.
pub fn form_of(meaning: &str) -> Option<String> {
    let m = meaning.trim();
    let lower = m.to_lowercase();
    let at = lower.rfind(" of ")?;
    let head = &lower[..at];
    const FORMS: [&str; 16] = [
        "inflection",
        "plural",
        "singular",
        "participle",
        "person",
        "feminine",
        "masculine",
        "form",
        "tense",
        "indicative",
        "subjunctive",
        "imperative",
        "conditional",
        "gerund",
        "spelling",
        "infinitive",
    ];
    if !FORMS.iter().any(|f| head.contains(f)) || head.split_whitespace().count() > 7 {
        return None;
    }
    let target = m[at + 4..].split([':', ',', ';', '(']).next()?.trim().trim_end_matches('.');
    (!target.is_empty() && target.split_whitespace().count() <= 3).then(|| target.to_string())
}

/// A word as it shows on screen with its short meaning: a form's line gives
/// way to its base word's meaning ("penche": "to lean", not "inflection of
/// pencher: …"), and a word the dictionary pairs with another ("[with à]")
/// shows with it ("parvenir à": "to reach"). (word as shown, meaning)
pub fn short_gloss(word: &str, meanings: &[String], base: impl Fn(&str) -> Vec<String>) -> (String, String) {
    let own = meanings.iter().find(|m| form_of(m).is_none()).cloned();
    let picked = own.or_else(|| meanings.iter().find_map(|m| form_of(m)).and_then(|b| base(&b).into_iter().find(|m| form_of(m).is_none())));
    let Some(m) = picked else { return (word.to_string(), String::new()) };
    let first = m.split(';').next().unwrap_or("").trim().to_string();
    // "[with à]" or "(with à)": that word goes with it.
    let with = ["[with ", "(with "].iter().find_map(|open| {
        let i = first.find(open)?;
        let rest = &first[i + open.len()..];
        let j = rest.find([']', ')'])?;
        Some((i, i + open.len() + j + 1, rest[..j].trim().to_string()))
    });
    match with {
        Some((a, b, w)) if !w.is_empty() && w.split_whitespace().count() <= 2 => {
            let gloss = format!("{} {}", first[..a].trim(), first[b..].trim()).trim().to_string();
            (format!("{word} {w}"), gloss)
        }
        _ => (word.to_string(), first),
    }
}

#[cfg(test)]
#[test]
fn glosses_skip_form_lines_and_take_their_partner_word() {
    let base = |w: &str| if w == "pencher" { vec!["to lean, to tilt".to_string()] } else { vec![] };
    assert_eq!(short_gloss("penche", &["inflection of pencher: first/third-person singular present".into()], base), ("penche".into(), "to lean, to tilt".into()));
    assert_eq!(short_gloss("parvenir", &["to reach [with à]".into(), "to succeed".into()], base), ("parvenir à".into(), "to reach".into()));
    assert_eq!(short_gloss("plage", &["beach; seaside".into()], base), ("plage".into(), "beach".into()));
    assert_eq!(form_of("third-person plural present indicative/subjunctive of porter").as_deref(), Some("porter"));
    assert_eq!(form_of("a cup of tea"), None);
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
