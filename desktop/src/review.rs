//! When each learning word comes back on screen, as Anki would have it
//! (owner, 2026-11-11): Good puts it off for longer each time, Easy longer
//! still (two steps at once), Again brings it back in a couple of minutes.
//! Kept in screen-words.json.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Seconds until it's back after Good, by how many Goods in a row.
const GOOD: [u64; 7] = [600, 3_600, 86_400, 3 * 86_400, 7 * 86_400, 21 * 86_400, 60 * 86_400];
/// After Again.
const AGAIN: u64 = 120;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Grade {
    Good,
    Easy,
    Again,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Card {
    /// Unix seconds it may show again.
    due: u64,
    /// Goods in a row.
    step: u8,
}

#[derive(Default)]
pub struct Review {
    cards: HashMap<String, Card>,
    path: Option<PathBuf>,
}

fn key(lang: &str, word: &str) -> String {
    format!("{lang}\t{word}")
}

impl Review {
    pub fn load(dir: &Path) -> Review {
        let path = dir.join("screen-words.json");
        let mut cards = HashMap::new();
        if let Ok(t) = std::fs::read_to_string(&path)
            && let Ok(v) = serde_json::from_str::<serde_json::Value>(&t)
            && let Some(o) = v.as_object()
        {
            for (k, c) in o {
                let due = c["due"].as_u64().unwrap_or(0);
                let step = c["step"].as_u64().unwrap_or(0).min(255) as u8;
                cards.insert(k.clone(), Card { due, step });
            }
        }
        Review { cards, path: Some(path) }
    }

    fn save(&self) {
        let Some(path) = &self.path else { return };
        let o: serde_json::Map<String, serde_json::Value> = self.cards.iter().map(|(k, c)| (k.clone(), serde_json::json!({"due": c.due, "step": c.step}))).collect();
        let tmp = path.with_extension("tmp");
        if std::fs::write(&tmp, serde_json::Value::Object(o).to_string()).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }

    pub fn grade(&mut self, lang: &str, word: &str, g: Grade, now: u64) {
        let c = self.cards.entry(key(lang, word)).or_default();
        match g {
            Grade::Good => {
                c.due = now + GOOD[(c.step as usize).min(GOOD.len() - 1)];
                c.step = c.step.saturating_add(1);
            }
            Grade::Easy => {
                let step = (c.step as usize + 1).min(GOOD.len() - 1);
                c.due = now + GOOD[step];
                c.step = c.step.saturating_add(2);
            }
            Grade::Again => {
                c.due = now + AGAIN;
                c.step = 0;
            }
        }
        self.save();
    }

    /// May it show now (never graded, or its time has come)?
    pub fn due(&self, lang: &str, word: &str, now: u64) -> bool {
        self.cards.get(&key(lang, word)).is_none_or(|c| c.due <= now)
    }

    /// Up to `n` of `words` (lang, word) that may show now and aren't up
    /// already: the overdue first (longest waiting), then the never seen, in
    /// list order from `seed` (the list takes the languages in turn: see
    /// [`mixed`]).
    pub fn next(&self, words: &[(String, String)], up: &[(String, String)], n: usize, now: u64, seed: usize) -> Vec<(String, String)> {
        let len = words.len().max(1);
        let mut ready: Vec<(u64, usize, &(String, String))> = words
            .iter()
            .enumerate()
            .filter(|(_, w)| !up.contains(w) && self.due(&w.0, &w.1, now))
            .map(|(i, w)| (self.cards.get(&key(&w.0, &w.1)).map_or(u64::MAX, |c| c.due), (i + len - seed % len) % len, w))
            .collect();
        ready.sort();
        ready.into_iter().take(n).map(|(_, _, w)| w.clone()).collect()
    }
}

/// The words with the languages taking turns (fr, de, ja, fr, de, …), each
/// language's own order kept: a few on screen show more than one language.
pub fn mixed(words: Vec<(String, String)>) -> Vec<(String, String)> {
    let mut langs: Vec<String> = Vec::new();
    let mut by: std::collections::HashMap<String, std::collections::VecDeque<(String, String)>> = Default::default();
    for w in words {
        if !langs.contains(&w.0) {
            langs.push(w.0.clone());
        }
        by.entry(w.0.clone()).or_default().push_back(w);
    }
    let mut out = Vec::new();
    loop {
        let before = out.len();
        for l in &langs {
            if let Some(w) = by.get_mut(l).and_then(|q| q.pop_front()) {
                out.push(w);
            }
        }
        if out.len() == before {
            return out;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: &str) -> (String, String) {
        ("fr".into(), s.into())
    }

    #[test]
    fn good_waits_longer_each_time_again_comes_back_soon() {
        let mut r = Review::default();
        r.grade("fr", "lugubre", Grade::Good, 1000);
        assert!(!r.due("fr", "lugubre", 1000 + 599));
        assert!(r.due("fr", "lugubre", 1000 + 600));
        r.grade("fr", "lugubre", Grade::Good, 2000);
        assert!(!r.due("fr", "lugubre", 2000 + 3_599));
        r.grade("fr", "lugubre", Grade::Again, 3000);
        assert!(r.due("fr", "lugubre", 3000 + 120));
        r.grade("fr", "lugubre", Grade::Good, 4000);
        assert!(r.due("fr", "lugubre", 4000 + 600), "Again starts the steps over");
    }

    #[test]
    fn easy_waits_two_steps_and_languages_take_turns() {
        let mut r = Review::default();
        r.grade("fr", "x", Grade::Easy, 0);
        assert!(!r.due("fr", "x", 3_599));
        assert!(r.due("fr", "x", 3_600));
        let m = mixed(vec![w("a"), w("b"), ("de".into(), "c".into()), ("de".into(), "d".into())]);
        assert_eq!(m.iter().map(|x| x.1.as_str()).collect::<Vec<_>>(), ["a", "c", "b", "d"]);
    }

    #[test]
    fn next_skips_words_up_and_not_yet_due_overdue_first() {
        let mut r = Review::default();
        let words = vec![w("a"), w("b"), w("c"), w("d")];
        r.grade("fr", "b", Grade::Good, 0); // due at 600
        r.grade("fr", "c", Grade::Again, 0); // due at 120
        assert_eq!(r.next(&words, &[w("a")], 3, 300, 0), vec![w("c"), w("d")]);
        assert_eq!(r.next(&words, &[], 2, 700, 0), vec![w("c"), w("b")]);
    }
}
