//! When each learning word comes back on screen, as Anki would have it
//! (owner, 2026-11-11): Good puts it off for longer each time, Easy longer
//! still (two steps at once), Again brings it back in a couple of minutes.
//! Kept in screen-words.json beside the settings, on the Mac and the phone
//! alike (moved here from the Mac app so both share it, 2026-11-11).
//!
//! Synced through ccez-words with the word statuses ([`sync`]): `GET
//! /review` answers `{"cards": {"<lang>\t<word>": {"due", "step", "at"}}}`,
//! `POST /review` takes the same shape; the later grade (`at`) wins per
//! word, so a word graded on one device isn't asked on another. A server
//! without `/review` (404) leaves each device its own schedule.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Seconds until it's back after Good, by how many Goods in a row.
const GOOD: [u64; 7] = [600, 3_600, 86_400, 3 * 86_400, 7 * 86_400, 21 * 86_400, 60 * 86_400];
/// After Again.
const AGAIN: u64 = 120;

#[derive(Clone, Copy, Debug, PartialEq, uniffi::Enum)]
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
    /// Unix seconds it was last graded (the newest grade wins a sync).
    at: u64,
}

#[derive(Default)]
pub struct Review {
    cards: HashMap<String, Card>,
    path: Option<PathBuf>,
}

fn key(lang: &str, word: &str) -> String {
    format!("{lang}\t{word}")
}

fn cards_of(v: &serde_json::Value) -> HashMap<String, Card> {
    let mut cards = HashMap::new();
    if let Some(o) = v.as_object() {
        for (k, c) in o {
            let due = c["due"].as_u64().unwrap_or(0);
            let step = c["step"].as_u64().unwrap_or(0).min(255) as u8;
            let at = c["at"].as_u64().unwrap_or(0);
            cards.insert(k.clone(), Card { due, step, at });
        }
    }
    cards
}

fn json_of(cards: &HashMap<String, Card>) -> serde_json::Value {
    let o: serde_json::Map<String, serde_json::Value> =
        cards.iter().map(|(k, c)| (k.clone(), serde_json::json!({"due": c.due, "step": c.step, "at": c.at}))).collect();
    serde_json::Value::Object(o)
}

impl Review {
    pub fn load(dir: &Path) -> Review {
        let path = dir.join("screen-words.json");
        let cards = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .map(|v| cards_of(&v))
            .unwrap_or_default();
        Review { cards, path: Some(path) }
    }

    fn save(&self) {
        let Some(path) = &self.path else { return };
        let tmp = path.with_extension("tmp");
        if std::fs::write(&tmp, json_of(&self.cards).to_string()).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }

    /// The whole schedule as kept on disk (and as sent to ccez-words).
    pub fn export(&self) -> String {
        json_of(&self.cards).to_string()
    }

    /// Takes in another device's schedule: a word graded there later than
    /// here keeps its later card. Returns how many cards changed.
    pub fn merge(&mut self, json: &str) -> usize {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else { return 0 };
        // The server wraps it as {"cards": …}; a bare map is taken too.
        let incoming = cards_of(v.get("cards").unwrap_or(&v));
        let mut took = 0;
        for (k, c) in incoming {
            if self.cards.get(&k).is_none_or(|l| c.at > l.at) {
                self.cards.insert(k, c);
                took += 1;
            }
        }
        if took > 0 {
            self.save();
        }
        took
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
        c.at = now;
        self.save();
    }

    /// When it may show again (Unix seconds), once graded.
    pub fn due_at(&self, lang: &str, word: &str) -> Option<u64> {
        self.cards.get(&key(lang, word)).map(|c| c.due)
    }

    /// A wait as a toast says it: "2 min", "1 h", "3 days".
    pub fn wait_words(secs: u64) -> String {
        match secs {
            s if s < 3_600 => format!("{} min", (s / 60).max(1)),
            s if s < 86_400 => format!("{} h", s / 3_600),
            s if s < 2 * 86_400 => "1 day".to_string(),
            s => format!("{} days", s / 86_400),
        }
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

/// One round with ccez-words at `server`: what was graded elsewhere comes
/// down (newest grade per word wins), then the merged schedule goes up.
/// Returns how many cards changed here. A server without `/review` is not
/// an error: nothing syncs.
pub async fn sync(server: &str, review: &std::sync::Mutex<Review>) -> Result<u32, String> {
    let base = server.trim_end_matches('/');
    let http = reqwest::Client::builder().timeout(std::time::Duration::from_secs(20)).build().unwrap_or_default();
    let res = http.get(format!("{base}/review")).send().await.map_err(|e| e.to_string())?;
    if res.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(0);
    }
    if !res.status().is_success() {
        return Err(format!("review: {}", res.status()));
    }
    let down = res.text().await.map_err(|e| e.to_string())?;
    let (took, up) = {
        let mut r = review.lock().unwrap_or_else(|e| e.into_inner());
        let took = r.merge(&down);
        (took, serde_json::json!({"cards": json_of(&r.cards)}).to_string())
    };
    let posted = http.post(format!("{base}/review")).header("content-type", "application/json").body(up).send().await;
    match posted {
        Ok(r) if r.status().is_success() || r.status() == reqwest::StatusCode::NOT_FOUND => Ok(took as u32),
        Ok(r) => Err(format!("review: {}", r.status())),
        Err(e) => Err(e.to_string()),
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

    #[test]
    fn a_grade_elsewhere_puts_the_word_off_here_too() {
        let mut mac = Review::default();
        mac.grade("fr", "lugubre", Grade::Good, 1000);
        let mut phone = Review::default();
        assert!(phone.due("fr", "lugubre", 1100));
        assert_eq!(phone.merge(&format!("{{\"cards\": {}}}", mac.export())), 1);
        assert!(!phone.due("fr", "lugubre", 1100));
        // An older grade coming back down doesn't undo a newer one.
        phone.grade("fr", "lugubre", Grade::Again, 2000);
        assert_eq!(phone.merge(&mac.export()), 0);
        assert!(phone.due("fr", "lugubre", 2200));
        // The old file shape (no "at") still reads.
        assert_eq!(phone.merge(r#"{"fr\tvieux": {"due": 5, "step": 2}}"#), 1);
    }
}
