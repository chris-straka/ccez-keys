//! Swipe typing (a setting, off by default): the letters a finger passed
//! over, in order, read as the likeliest word that starts on the first,
//! ends on the last, and whose letters all lie along the way.

use crate::words::{Lexicon, fold};

/// Up to `n` words for a swipe that passed over `path` (letters, in order).
pub fn decode(lex: &Lexicon, path: &str, n: usize) -> Vec<String> {
    let p: Vec<char> = path.chars().collect();
    if p.len() < 2 {
        return vec![];
    }
    let (first, last) = (p[0], p[p.len() - 1]);
    let mut scored: Vec<(f32, &str)> = Vec::new();
    for e in &lex.entries {
        let w: Vec<char> = fold(&e.word).chars().collect();
        if w.len() < 2 || w[0] != first || w[w.len() - 1] != last {
            continue;
        }
        // Each letter of the word, in order, somewhere along the path
        // (a doubled letter needs passing only once).
        let mut at = 0;
        let mut ok = true;
        for (i, c) in w.iter().enumerate() {
            if i > 0 && w[i - 1] == *c {
                continue;
            }
            match p[at..].iter().position(|x| x == c) {
                Some(k) => at += k,
                None => {
                    ok = false;
                    break;
                }
            }
        }
        if !ok {
            continue;
        }
        // A short word on a long path skipped a lot: likelier a longer one.
        let skipped = p.len().saturating_sub(w.len()) as f32;
        scored.push((e.logf - 0.35 * skipped, &e.word));
    }
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored.into_iter().take(n).map(|(_, w)| w.to_string()).collect()
}
