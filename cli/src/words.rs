//! keys-words: builds data/words/<lang>.txt, the keyboard's word lists.
//!
//!     keys-words <freq dir> <out dir> [--de-dict de.tsv.gz]
//!
//! <freq dir> holds wordfreq's ranked lists (one word per line, commonest
//! first; ccez-tube keeps them in android/app/src/main/assets/levels/freq,
//! made by ccez-lang's scripts/levels.ts from wordfreq v3.2). wordfreq
//! lower-cases everything, so:
//! - German takes each word's capitals from a Wiktionary dictionary
//!   (ccez-tube's assets/dict/de.tsv.gz): "haus" becomes "Haus" when only
//!   the capitalised form is a headword.
//! - English gets I, I'm and the days and months capitalised.
//! data/words/<lang>-extra.txt (word, tab, rank) adds what the lists split
//! apart, like French contractions (j'ai, c'est).

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::Path;

const LANGS: &[&str] = &["en", "fr", "de", "es"];

const EN_CAPS: &[&str] = &[
    "i", "i'm", "i'll", "i've", "i'd", "monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday", "january",
    "february", "april", "june", "july", "august", "september", "october", "november", "december", "english", "french",
    "german", "spanish", "japanese", "chinese", "arabic",
];

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: keys-words <freq dir> <out dir> [--de-dict de.tsv.gz]");
        std::process::exit(2);
    }
    let freq = Path::new(&args[1]);
    let out = Path::new(&args[2]);
    let de_dict = args.iter().position(|a| a == "--de-dict").and_then(|i| args.get(i + 1));
    let de_heads = de_dict.map(|p| headwords(Path::new(p))).unwrap_or_default();
    for lang in LANGS {
        let text = fs::read_to_string(freq.join(format!("{lang}.txt"))).expect("freq list");
        let common: HashSet<String> = text.lines().take(5000).map(|l| l.trim().to_string()).collect();
        let mut words: Vec<String> = Vec::new();
        let mut seen = HashSet::new();
        for w in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
            let forms: Vec<String> = match *lang {
                "en" if EN_CAPS.contains(&w) => vec![capitalise(w).replace("I'M", "I'm")],
                "de" => german(&eszett(w, &de_heads), &de_heads, &common),
                _ => vec![w.to_string()],
            };
            for w in forms {
                if seen.insert(w.clone()) {
                    words.push(w);
                }
            }
        }
        let head = format!(
            "# ccez-keys {lang} words, commonest first: wordfreq v3.2 (Robyn Speer, CC BY-SA 4.0){}. Built by keys-words.\n",
            if *lang == "de" { "; capitals from Wiktionary via kaikki.org (CC BY-SA 4.0)" } else { "" }
        );
        fs::write(out.join(format!("{lang}.txt")), head + &words.join("\n") + "\n").expect("write");
        println!("{lang}: {} words", words.len());
    }
}

fn capitalise(w: &str) -> String {
    let mut c = w.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// Parts of speech a lower-case German word can have that make it a
/// word of its own (not just a form or a rare preposition).
const OWN: &[&str] = &["verb", "adjective", "adverb", "pronoun", "conjunction", "determiner", "article", "particle", "numeral", "interjection", "contraction"];

/// A wordfreq word (lower case) as German writes it: "Haus" when the
/// capitalised headword is a noun and the lower-case one is only a form of
/// something else; both ("essen", "Essen") when each is a word.
fn german(w: &str, heads: &HashMap<String, Head>, common: &HashSet<String>) -> Vec<String> {
    let cap = capitalise(w);
    let cap_noun = heads.get(&cap).is_some_and(|h| h.pos.iter().any(|p| p == "noun"));
    if !cap_noun || cap == w {
        return vec![w.to_string()];
    }
    // Its own kind of word, or a form of a common one ("muss" of müssen;
    // "haus", the imperative of the rare hausen, doesn't count).
    let low_own = heads.get(w).is_some_and(|h| {
        h.pos.iter().any(|p| OWN.contains(&p.as_str())) || h.form_of.iter().any(|t| t.to_lowercase() != w && common.contains(t.as_str()))
    });
    if low_own { vec![w.to_string(), cap] } else { vec![cap] }
}

/// A dictionary headword: its parts of speech, and, when it is only the
/// Swiss spelling of another word ("gross" for groß), that word.
#[derive(Default)]
struct Head {
    pos: Vec<String>,
    /// Words this is a form of ("muss" → müssen).
    form_of: Vec<String>,
    swiss_of: Option<String>,
    swiss_only: bool,
}

const SWISS: &str = "Switzerland and Liechtenstein standard spelling of ";

/// wordfreq folds ß into "ss" (Python's casefold), so its German has no ß:
/// put it back where the "ss" spelling is only the Swiss one ("gross" →
/// groß, "strasse" → Straße) or not a word at all ("heisst" → heißt),
/// and keep it where "ss" is right (dass, muss, Kuss).
fn eszett(w: &str, heads: &HashMap<String, Head>) -> String {
    if !w.contains("ss") || heads.is_empty() {
        return w.to_string();
    }
    let head = heads.get(w).or_else(|| heads.get(&capitalise(w)));
    if let Some(h) = head {
        if let Some(t) = &h.swiss_of {
            return t.clone();
        }
        if !h.swiss_only {
            return w.to_string();
        }
    }
    let n = w.matches("ss").count();
    for mask in 1..(1u32 << n) {
        let mut out = String::new();
        let mut i = 0;
        let mut rest = w;
        while let Some(at) = rest.find("ss") {
            out.push_str(&rest[..at]);
            out.push_str(if mask >> i & 1 == 1 { "ß" } else { "ss" });
            rest = &rest[at + 2..];
            i += 1;
        }
        out.push_str(rest);
        if heads.contains_key(&out) || heads.contains_key(&capitalise(&out)) {
            return out;
        }
    }
    w.to_string()
}

/// ccez-tube's German dictionary: headword, tab,
/// [ipa, [[pos, gender, senses]...], [[form of, label]...]].
fn headwords(path: &Path) -> HashMap<String, Head> {
    let mut gz = flate2::read::GzDecoder::new(fs::File::open(path).expect("dictionary"));
    let mut text = String::new();
    gz.read_to_string(&mut text).expect("gzip");
    let mut out: HashMap<String, Head> = HashMap::new();
    for line in text.lines() {
        let Some((head, json)) = line.split_once('\t') else { continue };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else { continue };
        let h = out.entry(head.to_string()).or_default();
        let mut swiss = 0;
        let mut other = 0;
        for sense in v.get(1).and_then(|s| s.as_array()).into_iter().flatten() {
            if let Some(p) = sense.get(0).and_then(|p| p.as_str()) {
                h.pos.push(p.to_string());
            }
            for g in sense.get(2).and_then(|g| g.as_array()).into_iter().flatten().filter_map(|g| g.as_str()) {
                if let Some(t) = g.split(SWISS).nth(1) {
                    swiss += 1;
                    h.swiss_of.get_or_insert_with(|| t.split_whitespace().next().unwrap_or("").to_string());
                } else {
                    other += 1;
                }
            }
        }
        for form in v.get(2).and_then(|f| f.as_array()).into_iter().flatten() {
            let label = form.get(1).and_then(|l| l.as_str()).unwrap_or("");
            if let Some(t) = form.get(0).and_then(|t| t.as_str()) {
                h.form_of.push(t.to_string());
            }
            if label.contains("Switzerland") {
                swiss += 1;
            } else {
                other += 1;
            }
        }
        h.swiss_only = swiss > 0 && other == 0;
    }
    out
}
