//! When each learning word comes back on screen: FSRS, the scheduler Anki
//! uses (FSRS-5, its published default weights; owner, 2026-11-11: "is it
//! using the anki algorithm anywhere?"). Each word keeps a stability (days
//! until recall falls to 90%) and a difficulty, updated by every answer;
//! the next review is set for 90% recall. Again sends a word to a short
//! relearning step (2 min); Good on a new word to a 10 min learning step,
//! then it graduates (the maths is ccez-lang-core's `fsrs`, shared with
//! ccez-words, which grades what the owner watches). Kept in
//! screen-words.json beside the settings, on the
//! Mac and the phone alike.
//!
//! Synced through ccez-words with the word statuses ([`sync`]): `GET
//! /review` answers `{"cards": {"<lang>\t<word>": {…card…}}}`, `POST
//! /review` takes the same shape; the later answer (`at`) wins per word, so
//! a word answered on one device isn't asked on another. The server keeps
//! each card as it comes; only `at` matters to it. A server without
//! `/review` (404) leaves each device its own schedule.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ccez_lang_core::fsrs::{self, Card};
#[cfg(test)]
use ccez_lang_core::fsrs::DAY;

/// An answer on a screen word (the shared FSRS's, for Kotlin).
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Enum)]
pub enum Grade {
    Good,
    Easy,
    Again,
}

impl From<Grade> for fsrs::Grade {
    fn from(g: Grade) -> fsrs::Grade {
        match g {
            Grade::Good => fsrs::Grade::Good,
            Grade::Easy => fsrs::Grade::Easy,
            Grade::Again => fsrs::Grade::Again,
        }
    }
}

/// A word's schedule kept for Undo (see [Review::snapshot]).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Snapshot(Option<Card>);

#[derive(Default)]
pub struct Review {
    cards: HashMap<String, Card>,
    path: Option<PathBuf>,
}

fn key(lang: &str, word: &str) -> String {
    format!("{lang}\t{word}")
}

fn cards_of(v: &serde_json::Value) -> HashMap<String, Card> {
    v.as_object().map(|o| o.iter().map(|(k, c)| (k.clone(), Card::from_json(c))).collect()).unwrap_or_default()
}

fn json_of(cards: &HashMap<String, Card>) -> serde_json::Value {
    serde_json::Value::Object(cards.iter().map(|(k, c)| (k.clone(), c.to_json())).collect())
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
        self.cards.entry(key(lang, word)).or_default().answer(g.into(), now);
        self.save();
    }

    /// A word's schedule as it is now, to put back after a mistaken answer.
    pub fn snapshot(&self, lang: &str, word: &str) -> Snapshot {
        Snapshot(self.cards.get(&key(lang, word)).copied())
    }

    /// Puts a word's schedule back as [Review::snapshot] took it (Undo).
    /// Stamped now, so the putting back wins the next sync.
    pub fn restore(&mut self, lang: &str, word: &str, snap: Snapshot, now: u64) {
        match snap.0 {
            Some(c) => {
                self.cards.insert(key(lang, word), Card { at: now, ..c });
            }
            None => {
                self.cards.remove(&key(lang, word));
            }
        }
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
    /// already. The language with the fewest words up goes first (owner,
    /// 2026-11-13: one language's due cards filled the screen; the screen
    /// should mix them), and within it the overdue first (longest waiting),
    /// then the never seen, in list order from `seed`.
    pub fn next(&self, words: &[(String, String)], up: &[(String, String)], n: usize, now: u64, seed: usize) -> Vec<(String, String)> {
        let len = words.len().max(1);
        let mut ready: Vec<(u64, usize, &(String, String))> = words
            .iter()
            .enumerate()
            .filter(|(_, w)| !up.contains(w) && self.due(&w.0, &w.1, now))
            .map(|(i, w)| (self.cards.get(&key(&w.0, &w.1)).map_or(u64::MAX, |c| c.due), (i + len - seed % len) % len, w))
            .collect();
        ready.sort();
        let mut count: HashMap<String, usize> = HashMap::new();
        for (l, _) in up {
            *count.entry(l.clone()).or_default() += 1;
        }
        let mut out = Vec::new();
        while out.len() < n && !ready.is_empty() {
            // Ready is in order, so the first with the fewest up is its language's best.
            let at = (0..ready.len()).min_by_key(|&i| (count.get(&ready[i].2.0).copied().unwrap_or(0), i)).unwrap_or(0);
            let (_, _, w) = ready.remove(at);
            *count.entry(w.0.clone()).or_default() += 1;
            out.push(w.clone());
        }
        out
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

    #[test]
    fn the_screen_mixes_languages_even_when_one_has_all_the_due_cards() {
        let mut r = Review::default();
        let w = |l: &str, x: &str| (l.to_string(), x.to_string());
        let words = vec![w("zh", "过去"), w("zh", "谈话"), w("zh", "关于"), w("fr", "balayer"), w("de", "Urteil")];
        // The Chinese seen before and due again; the others never seen.
        for (_, x) in &words[..3] {
            r.grade("zh", x, Grade::Again, 0);
        }
        let now = 10_000_000;
        let first = r.next(&words, &[], 3, now, 0);
        assert_eq!(first.iter().map(|w| w.0.as_str()).collect::<Vec<_>>(), ["zh", "fr", "de"]);
        // Chinese already up: the next is another language.
        assert_eq!(r.next(&words, &[w("zh", "过去")], 1, now, 0)[0].0, "fr");
        // Only Chinese left to show: Chinese.
        assert_eq!(r.next(&words[..3], &[w("fr", "balayer")], 1, now, 0)[0].0, "zh");
    }

    #[test]
    fn undo_puts_the_schedule_back() {
        let mut r = Review::default();
        let before = r.snapshot("fr", "cap");
        r.grade("fr", "cap", Grade::Good, 1000);
        assert!(!r.due("fr", "cap", 1100));
        r.restore("fr", "cap", before, 1200);
        assert!(r.due("fr", "cap", 1200), "never graded again");
        r.grade("fr", "cap", Grade::Good, 2000);
        let graded = r.snapshot("fr", "cap");
        r.grade("fr", "cap", Grade::Again, 2100);
        r.restore("fr", "cap", graded, 2200);
        assert_eq!(r.due_at("fr", "cap"), Some(2000 + 600));
    }

    use super::*;

    fn w(s: &str) -> (String, String) {
        ("fr".into(), s.into())
    }

    #[test]
    fn fsrs_a_new_word_learns_graduates_then_waits_longer_each_time() {
        let mut r = Review::default();
        // New, Good: one learning step (10 min).
        r.grade("fr", "lugubre", Grade::Good, 0);
        assert_eq!(r.due_at("fr", "lugubre"), Some(600));
        // Good again: it graduates, a day or more off.
        r.grade("fr", "lugubre", Grade::Good, 600);
        let first = r.due_at("fr", "lugubre").unwrap() - 600;
        assert!(first >= DAY, "{first}");
        // Good when due: put off longer than the time before.
        let at = 600 + first;
        r.grade("fr", "lugubre", Grade::Good, at);
        let second = r.due_at("fr", "lugubre").unwrap() - at;
        assert!(second > first, "{second} <= {first}");
    }

    #[test]
    fn fsrs_again_comes_back_in_two_minutes_and_shortens_what_follows() {
        let mut steady = Review::default();
        let mut slipped = Review::default();
        for r in [&mut steady, &mut slipped] {
            r.grade("fr", "x", Grade::Good, 0);
            r.grade("fr", "x", Grade::Good, 600);
        }
        let due = steady.due_at("fr", "x").unwrap();
        slipped.grade("fr", "x", Grade::Again, due);
        assert_eq!(slipped.due_at("fr", "x"), Some(due + 120));
        slipped.grade("fr", "x", Grade::Good, due + 120);
        steady.grade("fr", "x", Grade::Good, due + 120);
        let after_slip = slipped.due_at("fr", "x").unwrap();
        let after_good = steady.due_at("fr", "x").unwrap();
        assert!(after_slip < after_good, "a lapse shortens the next wait");
    }

    #[test]
    fn fsrs_easy_on_a_new_word_skips_ahead_and_languages_take_turns() {
        let mut r = Review::default();
        r.grade("fr", "x", Grade::Easy, 0);
        assert!(r.due_at("fr", "x").unwrap() >= 7 * DAY);
        let m = mixed(vec![w("a"), w("b"), ("de".into(), "c".into()), ("de".into(), "d".into())]);
        assert_eq!(m.iter().map(|x| x.1.as_str()).collect::<Vec<_>>(), ["a", "c", "b", "d"]);
    }

    #[test]
    fn a_card_round_trips_through_its_json() {
        let mut r = Review::default();
        r.grade("fr", "x", Grade::Good, 0);
        r.grade("fr", "x", Grade::Good, 600);
        let mut other = Review::default();
        other.merge(&r.export());
        assert_eq!(other.cards.get("fr\tx"), r.cards.get("fr\tx"));
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
