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
    let own: Vec<String> = meanings.iter().filter(|m| form_of(m).is_none()).cloned().collect();
    let senses = if own.is_empty() { meanings.iter().find_map(|m| form_of(m)).map(|b| base(&b).into_iter().filter(|m| form_of(m).is_none()).collect()).unwrap_or_default() } else { own };
    let Some(m) = everyday_sense(&senses) else {
        return (word.to_string(), String::new(), Vec::new());
    };
    let first = m.split(';').next().unwrap_or("").trim().to_string();
    // "to sap (to subvert by digging)": the explanation in brackets at the
    // end goes; the short meaning is what comes before it.
    let first = drop_trailing_note(&first);
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

/// Of a word's first few senses, the everyday one: the first that isn't
/// marked rare, old or of a trade (archaic, nautical, military…) and doesn't
/// need a bracketed explanation (saper: "to sap, do sapping work on (to
/// subvert by digging)" gives way to "to erode, wear down, undermine";
/// owner, 2026-11-11). The first sense when none is plainer.
fn everyday_sense(senses: &[String]) -> Option<String> {
    const NICHE: [&str; 16] =
        ["archaic", "obsolete", "dated", "rare", "historical", "nautical", "military", "law", "legal", "heraldry", "dialectal", "regional", "slang", "chiefly in compounds", "literary", "euphemistic"];
    let plain = |m: &String| {
        let mut labels = String::new();
        let mut rest = m.trim();
        while let Some((inner, after)) = rest.strip_prefix('(').and_then(|r| r.split_once(')')) {
            labels.push_str(&inner.to_lowercase());
            labels.push(',');
            rest = after.trim_start();
        }
        let niche = labels.split(',').any(|l| NICHE.contains(&l.trim()));
        // "to sap (to subvert by digging)": a gloss that needs explaining; a
        // short note ("water (H₂O)") is no reason to pass a sense over.
        let note = rest.trim_end().strip_suffix(')').and_then(|r| r.rsplit_once(" (")).map(|(_, n)| n);
        let explained = note.is_some_and(|n| n.split_whitespace().count() >= 3);
        !niche && !explained
    };
    // A specialist's sense ("(geography) cape") gives way to a plain or
    // figurative one a little further down ("goal, direction, course": the
    // "cap" of the news); dictionaries order senses by history, not use.
    const SPECIALIST: [&str; 22] = [
        "geography",
        "nautical",
        "botany",
        "zoology",
        "anatomy",
        "medicine",
        "chemistry",
        "physics",
        "mathematics",
        "geometry",
        "music",
        "sports",
        "computing",
        "architecture",
        "astronomy",
        "biology",
        "geology",
        "linguistics",
        "grammar",
        "printing",
        "mineralogy",
        "ichthyology",
    ];
    let specialist = |m: &String| {
        let mut rest = m.trim();
        let mut labels = Vec::new();
        while let Some((inner, after)) = rest.strip_prefix('(').and_then(|r| r.split_once(')')) {
            labels.extend(inner.split(',').map(|l| l.trim().to_lowercase()));
            rest = after.trim_start();
        }
        labels.iter().any(|l| SPECIALIST.contains(&l.as_str()))
    };
    senses.iter().take(4).find(|m| plain(m) && !specialist(m)).or_else(|| senses.iter().take(3).find(|m| plain(m))).or_else(|| senses.first()).cloned()
}

/// A meaning without a bracketed note at its end ("to sap (to subvert by
/// digging)" → "to sap"); kept whole when that's all there is.
fn drop_trailing_note(m: &str) -> String {
    let t = m.trim_end();
    if !t.ends_with(')') {
        return t.to_string();
    }
    let mut depth = 0i32;
    for (i, ch) in t.char_indices().rev() {
        match ch {
            ')' => depth += 1,
            '(' => {
                depth -= 1;
                if depth == 0 {
                    let before = t[..i].trim_end().trim_end_matches(',').trim_end();
                    return if before.is_empty() { t.to_string() } else { before.to_string() };
                }
            }
            _ => {}
        }
    }
    t.to_string()
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
    let base = |w: &str| {
        if w == "pencher" { vec!["to lean, to tilt".to_string()] } else { vec![] }
    };
    assert_eq!(short_gloss("penche", &["inflection of pencher: first/third-person singular present".into()], base), ("penche".into(), "to lean, to tilt".into(), vec![]));
    assert_eq!(short_gloss("parvenir", &["to reach [with à]".into(), "to succeed".into()], base), ("parvenir à".into(), "to reach".into(), vec![]));
    assert_eq!(short_gloss("plage", &["beach; seaside".into()], base), ("plage".into(), "beach".into(), vec![]));
    assert_eq!(form_of("third-person plural present indicative/subjunctive of porter").as_deref(), Some("porter"));
    assert_eq!(form_of("a cup of tea"), None);
    assert_eq!(short_gloss("retenir", &["(transitive) to hold back, rein in".into()], base), ("retenir".into(), "to hold back, rein in".into(), vec!["tr".to_string()]));
}

#[cfg(test)]
mod everyday_tests {
    use super::*;

    #[test]
    fn the_everyday_sense_without_its_bracketed_note() {
        let saper = vec!["(transitive) to sap, do sapping work on (to subvert by digging)".to_string(), "(transitive, figurative) to erode, wear down, undermine".to_string()];
        let forms = vec!["inflection of saper: first-person plural present indicative".to_string()];
        let base = |w: &str| if w == "saper" { saper.clone() } else { vec![] };
        let (shown, meaning, tags) = short_gloss("sapons", &forms, base);
        assert_eq!((shown.as_str(), meaning.as_str()), ("sapons", "to erode, wear down, undermine"));
        assert_eq!(tags, ["tr", "fig"]);
        assert_eq!(drop_trailing_note("to sap (to subvert by digging (slowly))"), "to sap");
        let none = |_: &str| vec![];
        assert_eq!(short_gloss("cap", &["(geography) cape".into(), "(archaic) head".into()], none).1, "cape");
        let cap = ["(geography) cape".into(), "(archaic) head".into(), "(nautical) heading".into(), "(figuratively) goal, direction, course".into()];
        assert_eq!(short_gloss("cap", &cap, none).1, "goal, direction, course");
        assert_eq!(short_gloss("retenir", &["(transitive) to hold back, rein in".into()], none).1, "to hold back, rein in");
        let wasser = ["water (H₂O)".into(), "(chiefly in compounds) alcoholic beverage, similar to brandy".into()];
        assert_eq!(short_gloss("Wasser", &wasser, none).1, "water");
    }
}
