//! What to suggest while a word is typed, when to correct it on space,
//! and what the owner's own typing teaches (new words, next words).
//! Nothing here is logged: typed text never leaves this module except as
//! suggestions back to the keyboard.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::words::{Lexicon, Neighbours, fold, with_case};

/// Times a word the lists lack must be typed (and kept) before it's trusted.
pub const ACCEPT: u32 = 2;
/// One whole edit costs as much as this many e-folds of frequency.
const EDIT_WEIGHT: f32 = 3.0;
const COMPLETION_PENALTY: f32 = 0.4;

#[derive(Default, Serialize, Deserialize)]
pub struct UserWords {
    /// Words typed that the lists lack, with how often they were kept.
    pub words: HashMap<String, u32>,
    /// Folded previous word → next word → times.
    pub next: HashMap<String, HashMap<String, u32>>,
}

pub struct Lang {
    pub lex: Lexicon,
    pub user: UserWords,
}

impl Lang {
    pub fn new(lex: Lexicon, user: UserWords) -> Self {
        let mut lang = Lang { lex, user };
        let trusted: Vec<(String, u32)> = lang.user.words.iter().filter(|(_, n)| **n >= ACCEPT).map(|(w, n)| (w.clone(), *n)).collect();
        for (w, n) in trusted {
            lang.lex.insert(&w, user_logf(n));
        }
        lang
    }

    pub fn knows(&self, word: &str) -> bool {
        self.lex.knows(word)
    }
}

fn user_logf(n: u32) -> f32 {
    // Typed twice ≈ the 1500th commonest word; more often, commoner.
    -((3000.0 / n as f32).max(50.0)).ln()
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Suggestions {
    /// Best first, each in the typed word's capitals.
    pub words: Vec<String>,
    /// What space will turn the typed word into, if anything.
    pub correction: Option<String>,
    /// The typed word is a word as it stands (in any of the languages).
    pub typed_known: bool,
}

/// Words that look like addresses, numbers or names with inner capitals
/// (iPhone) are left as typed.
fn leave_alone(typed: &str) -> bool {
    typed.chars().any(|c| c.is_ascii_digit() || "@/._#:".contains(c))
        || typed.chars().skip(1).any(|c| c.is_uppercase()) && !typed.chars().all(|c| !c.is_lowercase())
}

/// `others` are the other languages typed in: a word known in any of
/// them is never corrected.
pub fn suggest(lang: &Lang, others: &[&Lang], typed: &str, keys: &Neighbours, autocorrect: bool) -> Suggestions {
    let n = typed.chars().count();
    let max = match n {
        0 => return Suggestions { words: vec![], correction: None, typed_known: false },
        1..=2 => 1.0,
        3..=4 => 1.3,
        _ => 2.0,
    };
    let own = own_logf(lang, typed);
    // How common the word is, as typed, in the other languages typed in.
    let other = others.iter().filter_map(|o| own_logf(o, typed)).reduce(f32::max);
    let typed_known = own.is_some() || others.iter().any(|o| o.knows(typed));
    let mut scored: Vec<Scored> = lang
        .lex
        .search(typed, max, 4, keys)
        .into_iter()
        .map(|c| {
            let e = &lang.lex.entries[c.id as usize];
            let score = e.logf - EDIT_WEIGHT * c.cost - if c.completion { COMPLETION_PENALTY } else { 0.0 };
            Scored { score, logf: e.logf, cost: c.cost, completion: c.completion, word: &e.word }
        })
        .collect();
    scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    let mut words: Vec<String> = Vec::new();
    for c in &scored {
        let w = with_case(typed, c.word);
        if !words.iter().any(|x| x.to_lowercase() == w.to_lowercase()) {
            words.push(w);
        }
        if words.len() == 4 {
            break;
        }
    }
    let correction = if autocorrect && !leave_alone(typed) { correct(typed, n, own, other, &scored) } else { None };
    Suggestions { words, correction, typed_known }
}

struct Scored<'a> {
    score: f32,
    logf: f32,
    cost: f32,
    completion: bool,
    word: &'a str,
}

/// About the 5000th commonest word.
const COMMON: f32 = -8.5;
/// A spelling this much commoner (e^2.5 ≈ 12×) wins over a rare one.
const MUCH_COMMONER: f32 = 2.5;

/// Among the language's 10000 commonest words (case aside).
pub fn common(lang: &Lang, word: &str) -> bool {
    own_logf(lang, word).is_some_and(|f| f > -(10_000f32).ln())
}

/// How likely these words are in this language, summed (a missing word
/// counts as rarer than any in the lists).
pub fn fit(lang: &Lang, words: &[String]) -> f32 {
    words.iter().map(|w| own_logf(lang, w).unwrap_or(-12.0)).sum()
}

/// How common the typed word is here as written (case aside), if it's a word.
fn own_logf(lang: &Lang, typed: &str) -> Option<f32> {
    let lower = typed.to_lowercase();
    lang.lex.same_folded(typed).filter(|e| e.word.to_lowercase() == lower).map(|e| e.logf).reduce(f32::max)
}

/// `other`: how common the word is, as typed, in another language typed
/// in. A word common there ("the" while on French) changes only to a
/// spelling about as common here or more ("deja" → déjà, though "deja"
/// is Spanish too).
fn correct(typed: &str, n: usize, own: Option<f32>, other: Option<f32>, scored: &[Scored]) -> Option<String> {
    let pick = |w: &str| Some(with_case(typed, w)).filter(|w| w != typed);
    // Blocked only when the other language's word is clearly the commoner
    // ("the" over "thé"); near-equal ones go to the language being typed
    // ("tres" → très on French, though "tres" is Spanish).
    let blocked = |logf: f32| other.is_some_and(|o| o > COMMON && logf + 1.0 < o);
    // A real word elsewhere ("allons", "manger" while still on English)
    // never becomes a different word here; only its accents may change.
    let elsewhere = other.is_some_and(|o| o > COMMON);
    let mid_word = scored.first().is_some_and(|c| c.completion);
    match own {
        // A word as typed: only its capitals, accents or apostrophe can
        // change, and only to a spelling far commoner ("deja" → déjà,
        // "jai" → j'ai, "i" → I, "haus" → Haus).
        Some(own) => {
            let lower = typed.to_lowercase();
            let near = scored.iter().filter(|c| !c.completion && c.cost <= 0.3).max_by(|a, b| a.logf.partial_cmp(&b.logf).unwrap_or(std::cmp::Ordering::Equal))?;
            let only_capitals = near.word.to_lowercase() == lower && near.word.chars().any(char::is_uppercase) && !scored.iter().any(|c| c.word == lower);
            if only_capitals {
                return pick(near.word);
            }
            if blocked(near.logf) {
                return None;
            }
            if near.logf >= own + MUCH_COMMONER {
                return pick(near.word);
            }
            // A rare word one slip or swap from a far commoner one ("teh").
            let slip = scored.iter().find(|c| !c.completion && c.cost > 0.0 && c.cost <= 0.7)?;
            if own < COMMON && slip.logf >= own + 5.5 && !elsewhere { pick(slip.word) } else { None }
        }
        None => {
            // Mid-word ("bonj"): a longer word is likelier than a fix.
            if mid_word {
                return None;
            }
            let best = scored.first()?;
            if blocked(best.logf) || (elsewhere && best.cost > 0.3) {
                return None;
            }
            let limit = match n {
                1..=2 => 0.3,
                3..=4 => 1.0,
                _ => 1.6,
            };
            let clear = scored.iter().filter(|c| c.word != best.word).find(|c| !c.completion).is_none_or(|c2| best.score - c2.score >= 0.5);
            if best.cost <= limit && clear { pick(best.word) } else { None }
        }
    }
}

/// A word the owner kept. `strong` when they undid a correction to keep
/// it: trusted at once.
pub fn learn(lang: &mut Lang, prev: &str, word: &str, strong: bool) {
    if !word.chars().any(|c| c.is_alphabetic()) || word.chars().count() > 40 {
        return;
    }
    if !lang.knows(word) || lang.user.words.contains_key(word) {
        let n = lang.user.words.entry(word.to_string()).or_insert(0);
        *n = if strong { (*n).max(ACCEPT) } else { *n + 1 };
        let n = *n;
        if n >= ACCEPT {
            lang.lex.insert(word, user_logf(n));
        }
    }
    let prev = fold(prev);
    if !prev.is_empty() {
        let next = lang.user.next.entry(prev).or_default();
        *next.entry(word.to_string()).or_insert(0) += 1;
        if next.len() > 64 {
            // Keep the 48 most used.
            let mut v: Vec<(String, u32)> = next.drain().collect();
            v.sort_by(|a, b| b.1.cmp(&a.1));
            v.truncate(48);
            next.extend(v);
        }
    }
}

/// Next words after `prev`, from what the owner has typed after it.
pub fn predict(lang: &Lang, prev: &str) -> Vec<String> {
    let Some(next) = lang.user.next.get(&fold(prev)) else { return vec![] };
    let mut v: Vec<(&String, &u32)> = next.iter().collect();
    v.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    v.into_iter().take(3).map(|(w, _)| w.clone()).collect()
}
