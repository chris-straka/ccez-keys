//! When each learning word comes back on screen: FSRS, the scheduler Anki
//! uses (FSRS-5, its published default weights; owner, 2026-11-11: "is it
//! using the anki algorithm anywhere?"). Each word keeps a stability (days
//! until recall falls to 90%) and a difficulty, updated by every answer;
//! the next review is set for 90% recall. Again sends a word to a short
//! relearning step (2 min); Good on a new word to a 10 min learning step,
//! then it graduates. Kept in screen-words.json beside the settings, on the
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

/// FSRS-5's default weights (as Anki ships them).
const W: [f64; 19] = [
    0.40255, 1.18385, 3.173, 15.69105, 7.1949, 0.5345, 1.4604, 0.0046, 1.54575, 0.1192, 1.01925, 1.9395, 0.11, 0.29605, 2.2698, 0.2315, 2.9898,
    0.51655, 0.6621,
];
const DECAY: f64 = -0.5;
const FACTOR: f64 = 19.0 / 81.0;
/// Reviews are set for this chance of still knowing the word.
const RETENTION: f64 = 0.9;
const DAY: u64 = 86_400;
/// Good on a new word: back once more after this, then it graduates.
const LEARN_STEP: u64 = 600;
/// Again: back after this.
const RELEARN: u64 = 120;
/// Never put off longer than this, in days.
const MAX_DAYS: f64 = 365.0;

#[derive(Clone, Copy, Debug, PartialEq, uniffi::Enum)]
pub enum Grade {
    Good,
    Easy,
    Again,
}

impl Grade {
    /// FSRS's rating: 1 Again, 3 Good, 4 Easy (Hard isn't asked).
    fn rating(self) -> f64 {
        match self {
            Grade::Again => 1.0,
            Grade::Good => 3.0,
            Grade::Easy => 4.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum State {
    #[default]
    New,
    Learning,
    Review,
    Relearning,
}

impl State {
    fn code(self) -> u64 {
        match self {
            State::New => 0,
            State::Learning => 1,
            State::Review => 2,
            State::Relearning => 3,
        }
    }
    fn of(n: u64) -> State {
        match n {
            1 => State::Learning,
            2 => State::Review,
            3 => State::Relearning,
            _ => State::New,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Card {
    /// Unix seconds it may show again.
    due: u64,
    /// Unix seconds it was last answered (the newest answer wins a sync).
    at: u64,
    /// Days until recall falls to 90%.
    s: f64,
    /// 1 (easy) to 10 (hard).
    d: f64,
    state: State,
    reps: u32,
    lapses: u32,
    /// Unix seconds of the last answer, for how long it's been.
    last: u64,
}

fn init_s(g: f64) -> f64 {
    W[(g as usize) - 1]
}
fn init_d(g: f64) -> f64 {
    (W[4] - (W[5] * (g - 1.0)).exp() + 1.0).clamp(1.0, 10.0)
}
fn recall(elapsed_days: f64, s: f64) -> f64 {
    (1.0 + FACTOR * elapsed_days / s.max(0.01)).powf(DECAY)
}
fn next_d(d: f64, g: f64) -> f64 {
    let d1 = d - W[6] * (g - 3.0) * (10.0 - d) / 9.0;
    (W[7] * init_d(4.0) + (1.0 - W[7]) * d1).clamp(1.0, 10.0)
}
fn recall_s(d: f64, s: f64, r: f64, g: f64) -> f64 {
    let easy = if g == 4.0 { W[16] } else { 1.0 };
    s * (1.0 + W[8].exp() * (11.0 - d) * s.powf(-W[9]) * ((W[10] * (1.0 - r)).exp() - 1.0) * easy)
}
fn forget_s(d: f64, s: f64, r: f64) -> f64 {
    (W[11] * d.powf(-W[12]) * ((s + 1.0).powf(W[13]) - 1.0) * (W[14] * (1.0 - r)).exp()).min(s)
}
/// Answered again within a day (learning steps).
fn short_s(s: f64, g: f64) -> f64 {
    s * (W[17] * (g - 3.0 + W[18])).exp()
}
/// Days until recall falls to [RETENTION] (whole days, 1 to [MAX_DAYS]).
fn interval(s: f64) -> u64 {
    let days = s / FACTOR * (RETENTION.powf(1.0 / DECAY) - 1.0);
    (days.round().clamp(1.0, MAX_DAYS) as u64) * DAY
}

impl Card {
    /// FSRS on one answer at `now`.
    fn answer(&mut self, g: Grade, now: u64) {
        let r = g.rating();
        match self.state {
            State::New => {
                self.s = init_s(r);
                self.d = init_d(r);
                (self.state, self.due) = match g {
                    Grade::Again => (State::Learning, now + RELEARN),
                    Grade::Good => (State::Learning, now + LEARN_STEP),
                    Grade::Easy => (State::Review, now + interval(self.s)),
                };
            }
            State::Learning | State::Relearning => {
                self.s = short_s(self.s.max(0.01), r);
                self.d = next_d(self.d, r);
                (self.state, self.due) = match g {
                    Grade::Again => (self.state, now + RELEARN),
                    _ => (State::Review, now + interval(self.s)),
                };
            }
            State::Review => {
                let elapsed = now.saturating_sub(self.last) as f64 / DAY as f64;
                let rec = recall(elapsed, self.s);
                self.d = next_d(self.d, r);
                if g == Grade::Again {
                    self.s = forget_s(self.d, self.s, rec);
                    self.lapses += 1;
                    self.state = State::Relearning;
                    self.due = now + RELEARN;
                } else {
                    self.s = if elapsed < 1.0 { short_s(self.s, r) } else { recall_s(self.d, self.s, rec, r) };
                    self.due = now + interval(self.s);
                }
            }
        }
        self.reps += 1;
        self.last = now;
        self.at = now;
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

/// The ladder this replaced (Good steps, in seconds): an old card's step
/// becomes a stability of that many days.
const OLD_STEPS: [u64; 7] = [600, 3_600, 86_400, 3 * 86_400, 7 * 86_400, 21 * 86_400, 60 * 86_400];

fn cards_of(v: &serde_json::Value) -> HashMap<String, Card> {
    let mut cards = HashMap::new();
    if let Some(o) = v.as_object() {
        for (k, c) in o {
            let due = c["due"].as_u64().unwrap_or(0);
            let at = c["at"].as_u64().unwrap_or(0);
            let card = if let Some(s) = c["s"].as_f64() {
                Card {
                    due,
                    at,
                    s,
                    d: c["d"].as_f64().unwrap_or(5.0),
                    state: State::of(c["state"].as_u64().unwrap_or(2)),
                    reps: c["reps"].as_u64().unwrap_or(1) as u32,
                    lapses: c["lapses"].as_u64().unwrap_or(0) as u32,
                    last: c["last"].as_u64().unwrap_or(at),
                }
            } else {
                // A card from the step ladder: carried over as it stood.
                let step = c["step"].as_u64().unwrap_or(0) as usize;
                let s = if step == 0 { init_s(1.0) } else { (OLD_STEPS[(step - 1).min(OLD_STEPS.len() - 1)] as f64 / DAY as f64).max(0.1) };
                let state = if step == 0 { State::Learning } else { State::Review };
                Card { due, at, s, d: 5.0, state, reps: step as u32, lapses: 0, last: at }
            };
            cards.insert(k.clone(), card);
        }
    }
    cards
}

fn json_of(cards: &HashMap<String, Card>) -> serde_json::Value {
    let o: serde_json::Map<String, serde_json::Value> = cards
        .iter()
        .map(|(k, c)| {
            (
                k.clone(),
                serde_json::json!({
                    "due": c.due, "at": c.at, "s": c.s, "d": c.d, "state": c.state.code(),
                    "reps": c.reps, "lapses": c.lapses, "last": c.last,
                    // For apps still reading the old ladder.
                    "step": c.reps.min(255),
                }),
            )
        })
        .collect();
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
        self.cards.entry(key(lang, word)).or_default().answer(g, now);
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
