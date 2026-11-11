//! The short meaning a word on screen shows (moved here from the Mac app
//! so the Mac and the phone say the same thing, 2026-11-11): a form's line
//! gives way to its base word's meaning, a word the dictionary pairs with
//! another ("[with à]") shows with it, and leading labels become tags
//! ("tr", "intr", "refl", "fig") shown as small pills.

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
/// shows with it ("parvenir à": "to reach"), and leading labels become
/// tags ("tr"). (word as shown, meaning, tags)
pub fn short_gloss(word: &str, meanings: &[String], base: impl Fn(&str) -> Vec<String>) -> (String, String, Vec<String>) {
    let own = meanings.iter().find(|m| form_of(m).is_none()).cloned();
    let picked = own.or_else(|| meanings.iter().find_map(|m| form_of(m)).and_then(|b| base(&b).into_iter().find(|m| form_of(m).is_none())));
    let Some(m) = picked else { return (word.to_string(), String::new(), Vec::new()) };
    let first = m.split(';').next().unwrap_or("").trim().to_string();
    // "(transitive) to hold back": the labels are tags (tags_of), not words of the meaning.
    let tags = tags_of(&first);
    let first = untagged(&first).to_string();
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
            (format!("{word} {w}"), gloss, tags)
        }
        _ => (word.to_string(), first, tags),
    }
}

/// The short tags a meaning's leading labels make ("(transitive)" → "tr",
/// "(reflexive, figuratively)" → "refl", "fig"), in order.
pub fn tags_of(meaning: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = meaning.trim();
    while let Some(inner) = rest.strip_prefix('(').and_then(|r| r.split_once(')')) {
        for label in inner.0.split(',') {
            let l = label.trim().to_lowercase();
            let tag = match l.as_str() {
                "transitive" => "tr",
                "intransitive" => "intr",
                "reflexive" | "pronominal" => "refl",
                "figuratively" | "figurative" => "fig",
                "informal" | "colloquial" => "informal",
                "slang" => "slang",
                "formal" => "formal",
                "literary" => "literary",
                _ => continue,
            };
            if !out.iter().any(|t| t == tag) {
                out.push(tag.to_string());
            }
        }
        rest = inner.1.trim_start();
    }
    out
}

/// A meaning without its leading labels.
pub fn untagged(meaning: &str) -> &str {
    let mut rest = meaning.trim();
    while let Some((_, after)) = rest.strip_prefix('(').and_then(|r| r.split_once(')')) {
        rest = after.trim_start();
    }
    rest
}

#[cfg(test)]
#[test]
fn leading_labels_become_tags() {
    assert_eq!(tags_of("(transitive) to hold back, rein in"), ["tr"]);
    assert_eq!(tags_of("(reflexive, figuratively) (informal) to calm down"), ["refl", "fig", "informal"]);
    assert_eq!(untagged("(transitive) to hold back, rein in"), "to hold back, rein in");
    assert!(tags_of("to reach").is_empty());
}

#[cfg(test)]
#[test]
fn glosses_skip_form_lines_and_take_their_partner_word() {
    let base = |w: &str| if w == "pencher" { vec!["to lean, to tilt".to_string()] } else { vec![] };
    assert_eq!(short_gloss("penche", &["inflection of pencher: first/third-person singular present".into()], base), ("penche".into(), "to lean, to tilt".into(), vec![]));
    assert_eq!(short_gloss("parvenir", &["to reach [with à]".into(), "to succeed".into()], base), ("parvenir à".into(), "to reach".into(), vec![]));
    assert_eq!(short_gloss("plage", &["beach; seaside".into()], base), ("plage".into(), "beach".into(), vec![]));
    assert_eq!(form_of("third-person plural present indicative/subjunctive of porter").as_deref(), Some("porter"));
    assert_eq!(form_of("a cup of tea"), None);
    assert_eq!(short_gloss("retenir", &["(transitive) to hold back, rein in".into()], base), ("retenir".into(), "to hold back, rein in".into(), vec!["tr".to_string()]));
}
