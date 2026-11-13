//! What to suggest while a word is typed, when to correct it on space,
//! and what the owner's own typing teaches (new words, next words).
//! Nothing here is logged: typed text never leaves this module except as
//! suggestions back to the keyboard.

use std::collections::{BTreeSet, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::words::{Lexicon, Neighbours, fold, with_case};

/// Times a word the lists lack must be typed (and kept) before it's trusted.
pub const ACCEPT: u32 = 2;
/// One whole edit costs as much as this many e-folds of frequency.
const EDIT_WEIGHT: f32 = 3.0;
const COMPLETION_PENALTY: f32 = 0.4;
/// Each edit past the first, on top of EDIT_WEIGHT.
const FAR_WEIGHT: f32 = 3.0;

#[derive(Default, Serialize, Deserialize)]
pub struct UserWords {
    /// Words typed that the lists lack, with how often they were kept.
    pub words: HashMap<String, u32>,
    /// Folded previous word → next word → times.
    pub next: HashMap<String, HashMap<String, u32>>,
    /// Words of the lists the owner typed (as the lists spell them), with
    /// how often: the ones used most are offered sooner.
    #[serde(default)]
    pub used: HashMap<String, u32>,
    /// Words taken out of the suggestions (lower case): never offered or
    /// put in again, until typed and held to save them.
    #[serde(default)]
    pub removed: BTreeSet<String>,
}

/// Words kept in `used`; past `USED_MAX`, the least used go down to this.
const USED_KEEP: usize = 3000;
const USED_MAX: usize = 4000;

pub struct Lang {
    pub lex: Lexicon,
    pub user: UserWords,
    /// Folded previous word → the words likely next, commonest first (data/next).
    pub next: HashMap<String, Vec<String>>,
    /// Words always put right, whatever the other languages say (lower case →
    /// right): "im" → I'm in English, though "im" is German.
    pub fixes: HashMap<String, String>,
}

impl Lang {
    pub fn new(lex: Lexicon, user: UserWords) -> Self {
        let mut lang = Lang { lex, user, next: HashMap::new(), fixes: HashMap::new() };
        let trusted: Vec<(String, u32)> = lang.user.words.iter().chain(&lang.user.used).filter(|(_, n)| **n >= ACCEPT).map(|(w, n)| (w.clone(), *n)).collect();
        for (w, n) in trusted {
            lang.lex.insert(&w, user_logf(n));
        }
        lang
    }

    /// A word here, as written (case aside), unless taken out of the suggestions.
    pub fn knows(&self, word: &str) -> bool {
        self.lex.knows(word) && !self.removed(word)
    }

    fn removed(&self, word: &str) -> bool {
        !self.user.removed.is_empty() && self.user.removed.contains(&word.to_lowercase())
    }
}

/// The fixes every keyboard makes in English: the apostrophe of a
/// contraction, and I in capitals. Only words that are nothing else
/// ("ill", "were", "its" and "lets" are words).
pub fn fixes(lang: &str) -> HashMap<String, String> {
    const EN: &[(&str, &str)] = &[
        ("i", "I"), ("im", "I'm"), ("i'm", "I'm"), ("ive", "I've"), ("i've", "I've"), ("i'd", "I'd"), ("i'll", "I'll"),
        ("dont", "don't"), ("doesnt", "doesn't"), ("didnt", "didn't"), ("cant", "can't"), ("wont", "won't"), ("isnt", "isn't"),
        ("arent", "aren't"), ("wasnt", "wasn't"), ("werent", "weren't"), ("hasnt", "hasn't"), ("havent", "haven't"),
        ("hadnt", "hadn't"), ("couldnt", "couldn't"), ("shouldnt", "shouldn't"), ("wouldnt", "wouldn't"), ("mustnt", "mustn't"),
        ("neednt", "needn't"), ("aint", "ain't"), ("youre", "you're"), ("youve", "you've"), ("youll", "you'll"),
        ("theyre", "they're"), ("theyve", "they've"), ("theyll", "they'll"), ("weve", "we've"), ("thats", "that's"),
        ("whats", "what's"), ("theres", "there's"), ("heres", "here's"), ("whos", "who's"), ("shes", "she's"), ("hes", "he's"),
        ("itll", "it'll"), ("thatll", "that'll"), ("alot", "a lot"),
    ];
    match lang {
        "en" => EN.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(),
        _ => HashMap::new(),
    }
}

/// What the text before the word says about it: the word just before,
/// and the words of the paragraph so far.
#[derive(Default)]
pub struct Context {
    pub prev: String,
    /// Lower case.
    pub near: HashSet<String>,
}

impl Context {
    /// `before`: the text before the cursor, the word being typed
    /// (`typed`) at its end.
    pub fn of(before: &str, typed: &str) -> Self {
        let chars: Vec<char> = before.chars().collect();
        let keep = chars.len().saturating_sub(typed.chars().count());
        let head: String = chars[..keep].iter().collect();
        let prev = crate::text::word_before(head.clone(), 0);
        let para = head.rsplit(['\n', '\r', '\u{2029}']).next().unwrap_or("");
        let near = para.split(|c: char| !crate::text::is_word_char(c)).filter(|w| w.chars().count() >= 3).map(str::to_lowercase).collect();
        Context { prev, near }
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
    suggest_in(lang, others, typed, &Context::default(), keys, autocorrect)
}

/// A word the context makes likelier counts as this much commoner (e^x):
/// typed after the word before by the owner, usual after it, or already
/// in the paragraph. Only the order changes: corrections stay on the
/// spelling alone.
const AFTER_MINE: f32 = 2.0;
const AFTER_USUAL: f32 = 1.2;
const IN_PARAGRAPH: f32 = 1.0;
const CONTEXT_MAX: f32 = 2.5;
/// The order shown, tuned on the benchmark's `shown` and `half`.
const SHOW_EDIT: f32 = 4.5;
const SHOW_FIRST: f32 = 2.5;
const SHOW_COMPLETION: f32 = 0.8;

fn context_bonus(lang: &Lang, ctx: &Context, word: &str) -> f32 {
    let lower = word.to_lowercase();
    let mut b: f32 = 0.0;
    if !ctx.prev.is_empty() {
        let key = fold(&ctx.prev);
        if lang.user.next.get(&key).is_some_and(|n| n.keys().any(|w| w.to_lowercase() == lower)) {
            b += AFTER_MINE;
        } else if lang.next.get(&key).is_some_and(|n| n.iter().any(|w| w.to_lowercase() == lower)) {
            b += AFTER_USUAL;
        }
    }
    if ctx.near.contains(&lower) {
        b += IN_PARAGRAPH;
    }
    b.min(CONTEXT_MAX)
}

/// The words the context offers that go on from `typed` ("Kat" after
/// "schwarze": Kater, typed there before), as the lists spell them.
fn context_words<'a>(lang: &'a Lang, ctx: &Context, typed: &str) -> Vec<&'a crate::words::Entry> {
    let t = fold(typed);
    let mut out: Vec<&crate::words::Entry> = Vec::new();
    let key = fold(&ctx.prev);
    let after = lang.user.next.get(&key).into_iter().flat_map(|n| n.keys()).chain(lang.next.get(&key).into_iter().flatten());
    for w in after.chain(ctx.near.iter()) {
        let f = fold(w);
        if f.len() <= t.len() || !f.starts_with(&t) {
            continue;
        }
        let lower = w.to_lowercase();
        if let Some(e) = lang.lex.same_folded(w).find(|e| e.word.to_lowercase() == lower)
            && !out.iter().any(|x| x.word == e.word)
        {
            out.push(e);
        }
    }
    out
}

/// Suggestions for `typed`, ordered with what came before it in mind.
pub fn suggest_in(lang: &Lang, others: &[&Lang], typed: &str, ctx: &Context, keys: &Neighbours, autocorrect: bool) -> Suggestions {
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
            // Each edit past the first counts far more: one slip from a rarer word beats
            // two from a very common one ("foood" → food, not for; "κάτιι" → κάτι, not και).
            let score = e.logf - EDIT_WEIGHT * c.cost - FAR_WEIGHT * (c.cost - 1.0).max(0.0) - if c.completion { COMPLETION_PENALTY } else { 0.0 };
            Scored { score, logf: e.logf, cost: c.cost, completion: c.completion, word: &e.word }
        })
        .filter(|c| !lang.removed(c.word))
        .collect();
    scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    // The order shown: slips count more than for corrections, and more
    // again in the first letter, so a word being typed meets its own
    // endings before a common word a slip or two away ("rete": retenir,
    // not tête).
    let first = fold(typed).chars().next();
    let mut ranked: Vec<(f32, &str)> = scored
        .iter()
        .map(|c| {
            let differs = fold(c.word).chars().next() != first;
            let shown = c.logf - SHOW_EDIT * c.cost - FAR_WEIGHT * (c.cost - 1.0).max(0.0) - if c.completion { SHOW_COMPLETION } else { 0.0 } - if differs { SHOW_FIRST } else { 0.0 };
            (shown + context_bonus(lang, ctx, c.word), c.word)
        })
        .collect();
    for e in context_words(lang, ctx, typed) {
        if !lang.removed(&e.word) && !ranked.iter().any(|(_, w)| *w == &*e.word) {
            ranked.push((e.logf - SHOW_COMPLETION + context_bonus(lang, ctx, &e.word), &e.word));
        }
    }
    ranked.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    // A fix taken back (backspace after it) teaches the word as typed: trusted from then on.
    let taught = lang.user.words.get(typed).or_else(|| lang.user.used.get(typed)).is_some_and(|n| *n >= ACCEPT);
    let fix = lang.fixes.get(&typed.to_lowercase()).filter(|_| !taught).map(|f| with_case(typed, f));
    let mut words: Vec<String> = fix.iter().cloned().collect();
    for (_, word) in &ranked {
        if words.len() == 4 {
            break;
        }
        let w = with_case(typed, word);
        if !words.iter().any(|x| x.to_lowercase() == w.to_lowercase()) {
            words.push(w);
        }
    }
    // A word taught by taking a fix back is left as typed, by the fix table and autocorrect alike.
    let correction = if !autocorrect || leave_alone(typed) || taught {
        None
    } else if let Some(f) = fix {
        Some(f).filter(|f| f != typed)
    } else {
        correct(typed, n, own, other, &scored)
    };
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

/// Among the language's 3000 commonest words, as written (case aside).
pub fn very_common(lang: &Lang, word: &str) -> bool {
    own_logf(lang, word).is_some_and(|f| f > -(3_000f32).ln())
}

/// Whether `w` is a word here, as written (case aside).
pub fn is_word(lang: &Lang, w: &str) -> bool {
    own_logf(lang, w).is_some()
}

/// How common the typed word is here as written (case aside), if it's a word.
fn own_logf(lang: &Lang, typed: &str) -> Option<f32> {
    if lang.removed(typed) {
        return None;
    }
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
    // Mid-word: the best is a longer word the typed letters start ("bonj").
    // One letter longer is a missed letter, a fix like any other ("guessin"
    // → guessing, "prioriy" → priority).
    let mid_word = scored.first().is_some_and(|c| c.completion && c.word.chars().count() >= n + 2);
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
            // An accent typed is meant (a long press): never taken away
            // ("mesuré" stays, though "mesure" is commoner).
            let marks = |w: &str| {
                use unicode_normalization::UnicodeNormalization;
                w.nfd().filter(|c| ('\u{300}'..='\u{36f}').contains(c)).count()
            };
            if marks(near.word) < marks(typed) {
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
            // Ahead of the next other word ("Einer" is "einer" again, not a rival).
            // Rivals too far off to be picked themselves don't count ("diesw": not "die").
            let clear = scored
                .iter()
                .filter(|c| c.word.to_lowercase() != best.word.to_lowercase() && c.cost <= limit)
                .find(|c| !c.completion)
                .is_none_or(|c2| best.score - c2.score >= 0.5);
            if best.cost <= limit && clear { pick(best.word) } else { None }
        }
    }
}

/// Drops `word` from the owner's words, and from what followed other words.
pub fn forget(user: &mut UserWords, word: &str) -> bool {
    let had = user.words.remove(word).is_some();
    user.next.remove(&fold(word));
    for next in user.next.values_mut() {
        next.remove(word);
    }
    user.next.retain(|_, n| !n.is_empty());
    had
}

/// Takes `word` out of the suggestions (a word saved by mistake,
/// "ccezkeys" for "ccez-keys"): forgotten if typing taught it, and never
/// offered again, until it's typed and held to save it.
pub fn remove(lang: &mut Lang, word: &str) {
    forget(&mut lang.user, word);
    let lower = word.to_lowercase();
    lang.user.used.retain(|w, _| w.to_lowercase() != lower);
    lang.user.removed.insert(lower);
}

/// A word the owner kept. `strong` when they undid a correction to keep
/// it, or held it to save it: trusted at once (and back in the
/// suggestions if it was taken out).
pub fn learn(lang: &mut Lang, prev: &str, word: &str, strong: bool) {
    if !word.chars().any(|c| c.is_alphabetic()) || word.chars().count() > 40 {
        return;
    }
    let lower = word.to_lowercase();
    if lang.user.removed.contains(&lower) {
        if !strong {
            return;
        }
        lang.user.removed.remove(&lower);
    }
    if !lang.lex.knows(word) || lang.user.words.contains_key(word) {
        let n = lang.user.words.entry(word.to_string()).or_insert(0);
        *n = if strong { (*n).max(ACCEPT) } else { *n + 1 };
        let n = *n;
        if n >= ACCEPT {
            lang.lex.insert(word, user_logf(n));
        }
    } else {
        // A word of the lists, in their spelling ("bonjour" for "Bonjour"
        // at a sentence's start): used often, it's offered sooner.
        let spelled = {
            let mut same = lang.lex.same_folded(word).filter(|e| e.word.to_lowercase() == lower).map(|e| e.word.to_string()).peekable();
            let first = same.peek().cloned();
            same.find(|w| w == word).or(first).unwrap_or_else(|| word.to_string())
        };
        let n = lang.user.used.entry(spelled.clone()).or_insert(0);
        // Strong (a fix taken back): trusted at once, as a new word is.
        *n = if strong { (*n + 1).max(ACCEPT) } else { *n + 1 };
        let n = *n;
        if n >= ACCEPT {
            lang.lex.insert(&spelled, user_logf(n));
        }
        if lang.user.used.len() > USED_MAX {
            let mut v: Vec<(String, u32)> = lang.user.used.drain().collect();
            v.sort_by(|a, b| b.1.cmp(&a.1));
            v.truncate(USED_KEEP);
            lang.user.used.extend(v);
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
/// The words likely after `prev`: the owner's own first (what followed it
/// before), then the language's usual ones.
pub fn predict(lang: &Lang, prev: &str) -> Vec<String> {
    let key = fold(prev);
    let mut mine: Vec<(&String, &u32)> = lang.user.next.get(&key).map(|n| n.iter().collect()).unwrap_or_default();
    mine.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    // Typed twice or more before the usual ones; once, after them.
    let (often, once): (Vec<_>, Vec<_>) = mine.into_iter().partition(|(_, n)| **n >= 2);
    let mut out: Vec<String> = often.into_iter().map(|(w, _)| w.clone()).collect();
    if let Some(base) = lang.next.get(&key) {
        out.extend(base.iter().cloned());
    }
    out.extend(once.into_iter().map(|(w, _)| w.clone()));
    let mut seen = std::collections::HashSet::new();
    out.retain(|w| !lang.removed(w) && seen.insert(w.to_lowercase()));
    out.truncate(3);
    out
}

/// Reads data/next/<lang>.txt (`previous \t next next…`).
pub fn parse_next(text: &str) -> HashMap<String, Vec<String>> {
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.split_once('\t'))
        .map(|(prev, next)| (fold(prev), next.split(' ').filter(|w| !w.is_empty()).map(String::from).collect()))
        .collect()
}
