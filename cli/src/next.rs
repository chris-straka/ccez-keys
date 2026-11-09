//! keys-next: builds data/next/<lang>.txt, the words likely to come next,
//! from Tatoeba's sentences (everyday sentences, CC BY 2.0 FR).
//!
//!     bzcat fra_sentences.tsv.bz2 | keys-next fr > data/next/fr.txt
//!
//! Input: Tatoeba's per-language export (`id \t lang \t sentence`).
//! Output: `previous \t next next next…`, the previous word in lower case,
//! the next ones commonest first, each in its commonest spelling ("Haus").
//! Only words split by spaces alone count as following each other: after
//! a comma or a full stop the keyboard predicts nothing.

use std::collections::HashMap;
use std::io::{BufRead, Write};

/// How many next words per previous word.
const KEEP: usize = 5;
/// Previous words kept: the commonest.
const PREVS: usize = 12_000;

fn word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '\'' || c == '’' || c == '-' || c == '\u{200C}' || c == '\u{200D}' || is_mark(c)
}

/// Combining marks (Hindi vowel signs, Hebrew and Arabic points): the common blocks.
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
}

/// Tatoeba's stock names (Tom and Mary, in every language and case): never predicted.
fn stock_name(w: &str) -> bool {
    let lower = w.to_lowercase();
    let capital = w.chars().next().is_some_and(char::is_uppercase);
    const LATIN: &[&str] = &[
        "tom", "toms", "tomu", "toma", "tomem", "tomowi", "tomovi", "tomot", "tomi", "tomit", "tomnak", "tomás", "tomas", "mary", "marys", "marie",
        "maria", "marias", "marit", "maryt", "marinak", "marii", "marię", "mari", "maryho", "marii",
    ];
    if capital && (LATIN.contains(&lower.as_str()) || lower.starts_with("tom'") || lower.starts_with("mary'")) {
        return true;
    }
    if capital && ((lower.starts_with("том") && lower.chars().count() <= 6) || lower == "мэри" || lower == "мери") {
        return true;
    }
    // Sentence writers' own names, frequent in some languages (Rima, Layla, Ziri, Yanni, Sami, Mina, Skura).
    const OTHERS: &[&str] = &["rima", "layla", "ziri", "yanni", "sami", "mina", "skura", "tatoeba", "سامي", "زيري", "ليلى", "ياني", "ريما"];
    if OTHERS.iter().any(|n| lower == *n || lower.starts_with(&format!("{n}'"))) {
        return true;
    }
    ["תום", "טום", "מרי", "توم", "ماري", "톰", "메리"].iter().any(|n| w.starts_with(n) && w.chars().count() <= n.chars().count() + 2)
}

/// The words of a sentence, each with whether only spaces came before it.
fn words(s: &str) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut gap_ok = false; // only spaces since the last word
    let mut seen_word = false;
    for c in s.chars() {
        if word_char(c) {
            cur.push(if c == '’' { '\'' } else { c });
        } else {
            if !cur.is_empty() {
                out.push((std::mem::take(&mut cur), seen_word && gap_ok));
                seen_word = true;
                gap_ok = true;
            }
            if !c.is_whitespace() {
                gap_ok = false;
            }
        }
    }
    if !cur.is_empty() {
        out.push((cur, seen_word && gap_ok));
    }
    // Hyphens and apostrophes only inside a word.
    out.into_iter()
        .map(|(w, f)| (w.trim_matches(|c| c == '-' || c == '\'').to_string(), f))
        .filter(|(w, _)| w.chars().any(char::is_alphabetic))
        .collect()
}

fn main() {
    let lang = std::env::args().nth(1).expect("usage: keys-next <lang> < sentences.tsv");
    // previous (lower) → next (lower) → count; next (lower) → spelling → count.
    let mut pairs: HashMap<String, HashMap<String, u32>> = HashMap::new();
    let mut spelled: HashMap<String, HashMap<String, u32>> = HashMap::new();
    let mut sentences = 0u32;
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { continue };
        let Some(text) = line.splitn(3, '\t').nth(2) else { continue };
        sentences += 1;
        let ws = words(text);
        for i in 1..ws.len() {
            let (next, joined) = &ws[i];
            if !joined {
                continue;
            }
            if stock_name(next) || stock_name(&ws[i - 1].0) {
                continue;
            }
            let prev = ws[i - 1].0.to_lowercase();
            let lower = next.to_lowercase();
            // A capital that only starts the sentence isn't the word's spelling.
            *spelled.entry(lower.clone()).or_default().entry(next.clone()).or_insert(0) += 1;
            *pairs.entry(prev).or_default().entry(lower).or_insert(0) += 1;
        }
    }
    let spelling = |lower: &str| -> String {
        spelled.get(lower).and_then(|m| m.iter().max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0))).map(|(w, _)| w.clone())).unwrap_or_else(|| lower.to_string())
    };
    let mut prevs: Vec<(&String, u32, &HashMap<String, u32>)> = pairs.iter().map(|(p, m)| (p, m.values().sum(), m)).filter(|(_, n, _)| *n >= 4).collect();
    prevs.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    let mut out = std::io::BufWriter::new(std::io::stdout().lock());
    writeln!(out, "# ccez-keys {lang} next words: built by keys-next from {sentences} Tatoeba sentences (tatoeba.org, CC BY 2.0 FR).").unwrap();
    for (prev, _, next) in prevs.into_iter().take(PREVS) {
        let mut v: Vec<(&String, &u32)> = next.iter().filter(|(_, n)| **n >= 2).collect();
        if v.is_empty() {
            continue;
        }
        v.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        let words: Vec<String> = v.into_iter().take(KEEP).map(|(w, _)| spelling(w)).collect();
        writeln!(out, "{prev}\t{}", words.join(" ")).unwrap();
    }
}
