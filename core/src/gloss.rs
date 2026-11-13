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
    let (tags, mut first) = clean_sense(&m);
    // A short meaning gets the next everyday sense after it, when both fit
    // (owner, 2026-11-13: vorwerfen showed "to throw at" alone; its "to
    // accuse of" is the one in use): "to conclude, end; to resolve".
    if first.chars().count() < 28
        // Only the word's own senses, before any line pointing to another
        // word (déchu's "past participle of déchoir" brought déchoir's).
        && let Some(next) = second_sense(&if meanings.iter().all(|x| form_of(x).is_some()) { senses.clone() } else { meanings.iter().take_while(|x| form_of(x).is_none()).cloned().collect::<Vec<_>>() }, &m)
    {
        let (_, more) = clean_sense(&next);
        let more = first_item(&more);
        if !more.is_empty() && !first.contains(&more) && first.chars().count() + more.chars().count() + 2 <= 48 {
            first = format!("{first}; {more}");
        }
    }
    // "[with à]" or "(with à)": that word goes with it.
    let with = ["[with ", "(with "].iter().find_map(|open| {
        let i = first.find(open)?;
        let rest = &first[i + open.len()..];
        let j = rest.find([']', ')'])?;
        // "[with de ‘with/from’]": the word, not its gloss.
        let w = rest[..j].split(['‘', '\'', '"', '(']).next().unwrap_or("").trim();
        Some((i, i + open.len() + j + 1, w.to_string()))
    });
    match with {
        Some((a, b, w)) if !w.is_empty() && w.split_whitespace().count() <= 2 && !is_somebody(&w) => {
            let gloss = format!("{} {}", first[..a].trim(), first[b..].trim()).trim().replace(" ;", ";");
            (format!("{word} {w}"), spelled_out(&gloss), tags)
        }
        _ => (word.to_string(), spelled_out(&first), tags),
    }
}

/// A word's kind as a short tag first among `tags` ("v", "n", "a"…; owner,
/// 2026-11-13: "so I don't have to ask"): the dictionary's first part of
/// speech, else a meaning starting "to " is a verb.
pub fn with_pos(tags: Vec<String>, pos: Vec<String>, meaning: &str) -> Vec<String> {
    let tag = match pos.first().map(String::as_str) {
        Some("verb") => Some("v"),
        Some("noun" | "proper noun" | "name") => Some("n"),
        Some("adjective" | "adj") => Some("a"),
        Some("adverb" | "adv") => Some("adv"),
        Some("pronoun") => Some("pron"),
        Some("preposition" | "prep") => Some("prep"),
        Some("conjunction" | "conj") => Some("conj"),
        Some("interjection" | "intj") => Some("interj"),
        Some("numeral" | "num") => Some("num"),
        Some("article" | "determiner" | "det") => Some("det"),
        Some("particle") => Some("part"),
        Some(_) => None,
        None => meaning.starts_with("to ").then_some("v"),
    };
    match tag {
        Some(t) if !meaning.is_empty() => std::iter::once(t.to_string()).chain(tags.into_iter().filter(|x| x != t)).collect(),
        _ => tags,
    }
}

#[cfg(test)]
#[test]
fn a_words_kind_is_its_first_tag() {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    assert_eq!(with_pos(s(&["intr"]), s(&["verb"]), "to stand out"), ["v", "intr"]);
    assert_eq!(with_pos(vec![], s(&["adjective", "verb"]), "deposed"), ["a"]);
    // Chinese has no parts of speech in its dictionary: "to …" is a verb.
    assert_eq!(with_pos(vec![], vec![], "to bring about"), ["v"]);
    assert!(with_pos(vec![], vec![], "bankruptcy").is_empty());
}

/// "sb", "sth" and the like: a dictionary's stand-ins for an object.
fn is_somebody(w: &str) -> bool {
    matches!(w.trim_end_matches("'s"), "sb" | "sth" | "someone" | "something" | "somebody" | "one" | "oneself")
}

/// The dictionary's shorthand spelled out (owner, 2026-11-13: "what is
/// sb?"): "to talk with sb" → "to talk with someone".
fn spelled_out(m: &str) -> String {
    let mut out = String::new();
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        out.push_str(match word.as_str() {
            "sb" => "someone",
            "sth" => "something",
            "sb's" => "someone's",
            "sth's" => "something's",
            w => w,
        });
        word.clear();
    };
    for c in m.chars() {
        if c.is_alphanumeric() || c == '\'' {
            word.push(c);
        } else {
            flush(&mut word, &mut out);
            out.push(c);
        }
    }
    flush(&mut word, &mut out);
    out
}

/// English words in everyday use (the keyboard's English list, commonest
/// first): a meaning in a word outside it ("to fulgurate") tells a learner
/// nothing.
pub fn common_english(w: &str) -> bool {
    static WORDS: std::sync::OnceLock<std::collections::HashSet<&'static str>> = std::sync::OnceLock::new();
    WORDS
        .get_or_init(|| include_str!("../../data/words/en.txt").lines().filter(|l| !l.starts_with('#')).collect())
        .contains(w.to_lowercase().as_str())
}

/// A meaning with its bracketed note at the end weighed (owner, 2026-11-13:
/// the full definition, not a cut one):
/// - a qualifier with "sb"/"sth" stays, unbracketed: "to talk (with sb)" →
///   "to talk with sb";
/// - a meaning in one rare English word gives way to the note explaining
///   it: "to fulgurate (to emit lightning)" → "to emit lightning";
/// - a short list of what it's said of stays: "to quench (fire, thirst, or
///   lime)", "to investigate (by the police)";
/// - anything longer is an explanation, and goes ("to read (to look at and
///   understand symbols, words, or data)" → "to read").
fn keep_note(m: &str) -> String {
    let Some((before, note)) = split_note(m) else { return m.trim_end().to_string() };
    let words = note.split_whitespace().count();
    let mentions = note.split(|c: char| !c.is_alphanumeric() && c != '\'').any(is_somebody);
    if mentions && words <= 5 {
        return format!("{before} {note}");
    }
    let core = before.strip_prefix("to ").unwrap_or(&before);
    let rare = !core.contains([' ', ',']) && !common_english(core);
    // Only a note that explains (a "to …" or "a …" definition) stands in
    // for it: "to quench (fire, thirst, or lime)" keeps its word.
    let explains_word = ["to ", "a ", "an ", "the "].iter().any(|p| note.starts_with(p));
    if rare && explains_word && words <= 6 {
        return note;
    }
    let plain = note.chars().all(|c| c.is_alphabetic() || matches!(c, ' ' | ',' | '\'' | '-'));
    let explains = note.starts_with("to ") || note.starts_with("by extension") || note.starts_with("e.g.") || note.starts_with("i.e.");
    if plain && !explains && words <= 4 {
        return format!("{before} ({note})");
    }
    before
}

/// "to sap (to subvert by digging (slowly))" → ("to sap", "to subvert by
/// digging (slowly)"); None without a note at the end, or when the note is
/// all there is.
fn split_note(m: &str) -> Option<(String, String)> {
    let t = m.trim_end();
    if !t.ends_with(')') {
        return None;
    }
    let mut depth = 0i32;
    for (i, ch) in t.char_indices().rev() {
        match ch {
            ')' => depth += 1,
            '(' => {
                depth -= 1;
                if depth == 0 {
                    // "down(wards)": a bracket inside a word is the word's own.
                    if t[..i].chars().last().is_some_and(|c| c.is_alphanumeric()) {
                        return None;
                    }
                    let before = t[..i].trim_end().trim_end_matches(',').trim_end();
                    return (!before.is_empty()).then(|| (before.to_string(), t[i + 1..t.len() - 1].trim().to_string()));
                }
            }
            _ => {}
        }
    }
    None
}

/// A sense as a card shows it, and its labels as tags: the labels read
/// first ("(transitive) to hold back" → tr), then the explanation after the
/// word cut, the note at the end weighed (keep_note), and a long one ended
/// at a comma.
fn clean_sense(m: &str) -> (Vec<String>, String) {
    let part = first_part(m).trim();
    let tags = tags_of(part);
    let body = cut_explanation(&drop_long_bracket(untagged(part).trim()));
    // "to sap (to subvert by digging)": the explanation in brackets at the
    // end goes; a qualifier stays ("to talk (with sb)"), and a rare English
    // word gives way to its explanation.
    let body = keep_note(&body);
    (tags, cap(&body, 48))
}

/// Up to `max` characters, ended at the last comma or semicolon outside
/// brackets that fits; a meaning with none stays whole.
fn cap(m: &str, max: usize) -> String {
    if m.chars().count() <= max {
        return m.to_string();
    }
    let mut depth = 0i32;
    let mut cut = None;
    for (n, (i, c)) in m.char_indices().enumerate() {
        if n >= max {
            break;
        }
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            ',' | ';' if depth <= 0 => cut = Some(i),
            _ => {}
        }
    }
    cut.map(|i| m[..i].trim_end().to_string()).unwrap_or_else(|| m.to_string())
}

/// The first of a meaning's comma-separated items, brackets kept whole
/// ("to accuse of, to reproach someone" → "to accuse of").
fn first_item(m: &str) -> String {
    let mut depth = 0i32;
    for (i, c) in m.char_indices() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            ',' | ';' if depth <= 0 => return m[..i].trim().to_string(),
            _ => {}
        }
    }
    m.trim().to_string()
}

/// A label that marks a sense as out of everyday use, anywhere in it
/// ("the general meaning, which is now rare").
fn niche_label(l: &str) -> bool {
    const OUT: [&str; 6] = ["archaic", "obsolete", "dated", "rare", "historical", "dialectal"];
    let l = l.to_lowercase();
    OUT.iter().any(|w| l.split(|c: char| !c.is_alphabetic()).any(|x| x == *w))
}

/// The next sense worth adding after `chosen`: among the first four, not
/// out of use, not a narrowing of the one before ("(specifically) …").
fn second_sense(senses: &[String], chosen: &str) -> Option<String> {
    let at = senses.iter().position(|s| s == chosen)?;
    senses.iter().take(4).enumerate().filter(|(i, _)| *i != at).map(|(_, s)| s).find(|s| {
        let labels: Vec<String> = s.trim().strip_prefix('(').and_then(|r| r.split_once(')')).map(|(l, _)| l.split(',').map(|x| x.trim().to_lowercase()).collect()).unwrap_or_default();
        // Only grammar labels (or none): a field's sense ("(geography) cape") never.
        const GRAMMAR: [&str; 12] = ["transitive", "intransitive", "reflexive", "ditransitive", "ambitransitive", "figuratively", "figurative", "informal", "colloquial", "auxiliary", "impersonal", "transitive or intransitive"];
        labels.iter().all(|l| GRAMMAR.contains(&l.as_str()))
            && form_of(s).is_none()
            && !untagged(s).trim().is_empty()
            // A name, not a meaning ("a surname", "a male given name").
            && !["surname", "given name", "placename", "place name"].iter().any(|n| untagged(s).contains(n))
    }).cloned()
}

/// Of a word's first few senses, the everyday one. Not one marked rare,
/// old or of a trade (archaic, nautical…), and a specialist's sense
/// ("(geography) cape") gives way to a plain or figurative one ("goal,
/// direction, course"): dictionaries order senses by history, not use.
/// The first of those (a sense explained in brackets is still the one:
/// lesen is "to read"); the first sense when none is plainer.
fn everyday_sense(senses: &[String]) -> Option<String> {
    const NICHE: [&str; 16] =
        ["archaic", "obsolete", "dated", "rare", "historical", "nautical", "military", "law", "legal", "heraldry", "dialectal", "regional", "slang", "chiefly in compounds", "literary", "euphemistic"];
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
    let labels = |m: &String| {
        let mut rest = m.trim();
        let mut out = Vec::new();
        while let Some((inner, after)) = rest.strip_prefix('(').and_then(|r| r.split_once(')')) {
            out.extend(inner.split(',').map(|l| l.trim().to_lowercase()));
            rest = after.trim_start();
        }
        out
    };
    let plain = |m: &&String| !labels(m).iter().any(|l| NICHE.contains(&l.as_str()) || niche_label(l));
    let specialist = |m: &&String| labels(m).iter().any(|l| SPECIALIST.contains(&l.as_str()));
    // "of sound", "figurative senses": a heading over senses, not a meaning.
    let heading = |m: &&String| {
        let t = untagged(m).trim();
        (t.starts_with("of ") && t.split_whitespace().count() <= 3) || t.ends_with(" senses")
    };
    let senses: Vec<&String> = senses.iter().filter(|m| !heading(m)).collect();
    if senses.is_empty() {
        return None;
    }
    let mut picks: Vec<&String> = senses.iter().copied().take(4).filter(plain).filter(|m| !specialist(m)).collect();
    if picks.is_empty() {
        picks = senses.iter().copied().take(3).filter(plain).collect();
    }
    picks.first().or(senses.first()).map(|m| (*m).clone())
}

/// A long meaning without the explanation after its word ("water, a
/// liquid that is transparent, …" → "water"; "April, the fourth month…" →
/// "April"); short ones as they are ("an, a").
fn cut_explanation(m: &str) -> String {
    if m.split_whitespace().count() <= 5 {
        return m.to_string();
    }
    for lead in [", a ", ", an ", ", the "] {
        if let Some(i) = m.find(lead)
            && i > 0
        {
            return m[..i].to_string();
        }
    }
    m.to_string()
}

/// A meaning up to its first ';' outside brackets ("trace (rest of …;
/// (by extension) any small amount); …" keeps its bracket whole).
fn first_part(m: &str) -> &str {
    let mut depth = 0i32;
    for (i, c) in m.char_indices() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            ';' if depth <= 0 => return &m[..i],
            _ => {}
        }
    }
    m
}

/// A long note in square brackets at the end goes ("to investigate [(optional)
/// with gegen (+ accusative) ‘someone’ …]"); a short "[with à]" stays for
/// the word it goes with.
fn drop_long_bracket(m: &str) -> String {
    let t = m.trim_end();
    if !t.ends_with(']') {
        return t.to_string();
    }
    let mut depth = 0i32;
    for (i, ch) in t.char_indices().rev() {
        match ch {
            ']' => depth += 1,
            '[' => {
                depth -= 1;
                if depth == 0 {
                    let inner = &t[i + 1..t.len() - 1];
                    let short_with = inner.starts_with("with ") && inner.split_whitespace().count() <= 3;
                    let before = t[..i].trim_end();
                    return if short_with || before.is_empty() { t.to_string() } else { before.to_string() };
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
    assert_eq!(short_gloss("parvenir", &["to reach [with à]".into(), "to succeed".into()], base), ("parvenir à".into(), "to reach; to succeed".into(), vec![]));
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
        assert_eq!((shown.as_str(), meaning.as_str()), ("sapons", "to sap, do sapping work on; to erode"));
        assert_eq!(tags, ["tr"]);
        assert_eq!(split_note("to sap (to subvert by digging (slowly))"), Some(("to sap".into(), "to subvert by digging (slowly)".into())));
        let none = |_: &str| vec![];
        assert_eq!(short_gloss("cap", &["(geography) cape".into(), "(archaic) head".into()], none).1, "cape");
        let cap = ["(geography) cape".into(), "(archaic) head".into(), "(nautical) heading".into(), "(figuratively) goal, direction, course".into()];
        assert_eq!(short_gloss("cap", &cap, none).1, "goal, direction, course");
        assert_eq!(short_gloss("retenir", &["(transitive) to hold back, rein in".into()], none).1, "to hold back, rein in");
        let wasser = ["water (H₂O)".into(), "(chiefly in compounds) alcoholic beverage, similar to brandy".into()];
        assert_eq!(short_gloss("Wasser", &wasser, none).1, "water");
        let lesen = [
            "(transitive or intransitive) to read (to look at and understand symbols, words, or data)".into(),
            "(reflexive) to read (to give off a certain impression while being read; of a text)".into(),
            "(transitive) to select and gather or harvest (grapes, etc.)".into(),
        ];
        assert_eq!(short_gloss("lesen", &lesen, none).1, "to read");
        let loeschen = ["(transitive) to quench (fire, thirst, or lime)".into(), "(transitive) to delete, to erase (remembrance, memory, data)".into()];
        assert_eq!(short_gloss("löschen", &loeschen, none).1, "to quench (fire, thirst, or lime)");
        let spur = ["trace (rest of something that indicates its former presence; (by extension) any small amount)".into(), "trail, track, spoor".into()];
        assert_eq!(short_gloss("Spur", &spur, none).1, "trace; trail");
        let ermitteln = ["(intransitive) to investigate (by the police) [(optional) with gegen (+ accusative) ‘someone’ and wegen (+ genitive) ‘for something’]".into()];
        assert_eq!(short_gloss("ermitteln", &ermitteln, none).1, "to investigate (by the police)");
        // The whole meaning (owner, 2026-11-13): a qualifier stays, spelled out;
        // a rare English word gives way to what it means.
        assert_eq!(short_gloss("谈话", &["to talk (with sb)".into(), "to have a conversation".into()], none).1, "to talk with someone; to have a conversation");
        assert_eq!(short_gloss("fulgurer", &["to fulgurate (to emit lightning)".into()], none).1, "to emit lightning");
        assert_eq!(short_gloss("x", &["to give sth to sb".into()], none).1, "to give something to someone");
        let vorwerfen = ["(transitive) to throw at, throw forward".into(), "(specifically) to feed to (throw at an animal as food)".into(), "(figuratively, ditransitive) to accuse of, to reproach someone for something".into()];
        assert_eq!(short_gloss("vorwerfen", &vorwerfen, none).1, "to throw at, throw forward; to accuse of");
        let verhaften = ["(ambitransitive, the general meaning, which is now rare) to stop moving and instead stick".into(), "(transitive) to imprison, to put into confinement after judicial writ (the state of which is called Haft) (this is the most common signification in its strict definition)".into(), "(transitive, now rare) to legally subject to eventual foreclosure".into()];
        assert_eq!(short_gloss("verhaften", &verhaften, none), ("verhaften".into(), "to imprison".into(), vec!["tr".to_string()]));
        assert_eq!(short_gloss("Wahl", &["choice, selection".into(), "a surname".into()], none).1, "choice, selection");
        let dechu = ["deposed, dethroned".into(), "past participle of déchoir".into(), "to wane (diminish)".into()];
        assert_eq!(short_gloss("déchu", &dechu, none).1, "deposed, dethroned");
        assert_eq!(short_gloss("beschließen", &["to conclude, end; to terminate".into(), "to resolve, decide, determine".into()], none).1, "to conclude, end; to resolve");
        assert_eq!(short_gloss("runterhalten", &["(transitive, colloquial) to hold something down(wards)".into()], none).1, "to hold something down(wards)");
        assert_eq!(short_gloss("parvenir", &["(intransitive) to reach [with à]".into()], none).0, "parvenir à");
        assert_eq!(short_gloss("défaillir", &["to faint, feel faint [with de ‘with/from’]".into()], none), ("défaillir de".into(), "to faint, feel faint".into(), vec![]));
        assert_eq!(short_gloss("eau", &["water, a liquid that is transparent, colorless, odorless, and tasteless in its pure form".into()], none).1, "water");
        assert_eq!(short_gloss("un", &["an, a".into()], none).1, "an, a");
        assert_eq!(short_gloss("répercuter", &["of sound".into(), "of light".into(), "figurative senses".into()], none).1, "");
    }
}

