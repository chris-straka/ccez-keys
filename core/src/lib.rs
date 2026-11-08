//! ccez-keys' core, for the Android keyboard through UniFFI: words and
//! autocorrect (`words`, `typing`), layouts from data files (`layout`),
//! every setting in one table (`settings`), and a log agents can read
//! (`log`). The keyboard service draws and handles touch; everything it
//! decides about text, it asks here.

pub mod layout;
pub mod learn;
pub mod log;
pub mod settings;
pub mod typing;
pub mod words;

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

pub use layout::{Key, KeyKind, Layout, Page, Row};
pub use learn::{Card, Ruby, StripWord, WordStatus};
pub use typing::Suggestions;

uniffi::setup_scaffolding!();

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum KeysError {
    #[error("{0}")]
    Bad(String),
}

struct State {
    langs: HashMap<String, typing::Lang>,
    keys: HashMap<String, words::Neighbours>,
    settings: settings::Settings,
    current: String,
}

#[derive(uniffi::Object)]
pub struct Engine {
    dir: PathBuf,
    layouts: layout::Layouts,
    log: log::Log,
    learn: learn::Learn,
    state: Mutex<State>,
}

impl Engine {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn user_path(&self, lang: &str) -> PathBuf {
        self.dir.join(format!("user-{lang}.json"))
    }

    fn typing_languages_of(&self, s: &settings::Settings) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let default = s.get("languages.default").and_then(|v| v.as_str()).unwrap_or("en").to_string();
        for l in std::iter::once(default).chain(s.strings("languages.learning")) {
            if self.layouts.has(&l) && !out.contains(&l) {
                out.push(l);
            }
        }
        if out.is_empty() {
            out.push("en".into());
        }
        out
    }

    fn save_settings(&self, s: &settings::Settings) {
        let _ = fs::write(self.dir.join("settings.json"), s.to_json());
    }
}

#[uniffi::export]
impl Engine {
    /// `dir` holds settings, the owner's own words and the log.
    #[uniffi::constructor]
    pub fn new(dir: String) -> Arc<Self> {
        let dir = PathBuf::from(dir);
        let _ = fs::create_dir_all(&dir);
        let settings = settings::Settings::from_json(&fs::read_to_string(dir.join("settings.json")).unwrap_or_default());
        let current = fs::read_to_string(dir.join("current-language")).unwrap_or_default().trim().to_string();
        let engine = Engine {
            log: log::Log::new(&dir),
            learn: learn::Learn::new(&dir),
            layouts: layout::Layouts::load(),
            dir,
            state: Mutex::new(State { langs: HashMap::new(), keys: HashMap::new(), settings, current }),
        };
        {
            let mut s = engine.lock();
            let langs = engine.typing_languages_of(&s.settings);
            if !langs.contains(&s.current) {
                s.current = langs[0].clone();
            }
        }
        Arc::new(engine)
    }

    pub fn version(&self) -> String {
        env!("CARGO_PKG_VERSION").into()
    }

    /// A language's word list (data/words/<lang>.txt). Returns how many words.
    pub fn load_words(&self, lang: String, text: String) -> u32 {
        let lex = words::Lexicon::from_ranked(&text);
        let user: typing::UserWords = fs::read_to_string(self.user_path(&lang)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        let n = lex.len() as u32;
        let keys = self.layouts.neighbours(&lang);
        let mut s = self.lock();
        s.langs.insert(lang.clone(), typing::Lang::new(lex, user));
        s.keys.insert(lang, keys);
        n
    }

    pub fn loaded(&self, lang: String) -> bool {
        self.lock().langs.contains_key(&lang)
    }

    /// The languages the space bar cycles through: the default language,
    /// then the ones being learned, those with a layout.
    pub fn typing_languages(&self) -> Vec<String> {
        let s = self.lock();
        self.typing_languages_of(&s.settings)
    }

    pub fn current_language(&self) -> String {
        self.lock().current.clone()
    }

    pub fn set_current_language(&self, lang: String) {
        let _ = fs::write(self.dir.join("current-language"), &lang);
        self.lock().current = lang;
    }

    /// One step along the space bar's languages (1 forward, -1 back).
    pub fn step_language(&self, step: i32) -> String {
        let langs = self.typing_languages();
        let cur = self.current_language();
        let i = langs.iter().position(|l| *l == cur).unwrap_or(0) as i32;
        let n = langs.len() as i32;
        let next = langs[((i + step) % n + n) as usize % n as usize].clone();
        self.set_current_language(next.clone());
        next
    }

    pub fn layout(&self, lang: String, page: Page) -> Layout {
        self.layouts.layout(&lang, page)
    }

    /// Suggestions for the word being typed, in `lang`.
    pub fn suggest(&self, lang: String, typed: String) -> Suggestions {
        let s = self.lock();
        let others_langs = self.typing_languages_of(&s.settings);
        let auto = s.settings.bool("typing.autocorrect");
        let Some(l) = s.langs.get(&lang) else {
            return Suggestions { words: vec![], correction: None, typed_known: false };
        };
        let others: Vec<&typing::Lang> = others_langs.iter().filter(|o| **o != lang).filter_map(|o| s.langs.get(o)).collect();
        let empty = words::Neighbours::default();
        let keys = s.keys.get(&lang).unwrap_or(&empty);
        typing::suggest(l, &others, &typed, keys, auto)
    }

    /// The language the last words were written in, when it isn't
    /// `current`: all of them common there, and together far likelier
    /// there than here ("ich bin": German, though "bin" is English too).
    pub fn detect_language(&self, current: String, words: Vec<String>) -> Option<String> {
        if words.len() < 2 {
            return None;
        }
        let s = self.lock();
        let here = typing::fit(s.langs.get(&current)?, &words);
        self.typing_languages_of(&s.settings)
            .into_iter()
            .filter(|l| *l != current)
            .filter_map(|l| {
                let x = s.langs.get(&l)?;
                let all_common = words.iter().all(|w| typing::common(x, w));
                let there = typing::fit(x, &words);
                (all_common && there - here >= 4.0).then_some((there, l))
            })
            .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(_, l)| l)
    }

    /// Words that followed `prev` before.
    pub fn predict(&self, lang: String, prev: String) -> Vec<String> {
        let s = self.lock();
        s.langs.get(&lang).map(|l| typing::predict(l, &prev)).unwrap_or_default()
    }

    /// A word kept after `prev`. `strong` when a correction was undone to keep it.
    pub fn learn(&self, lang: String, prev: String, word: String, strong: bool) {
        let mut s = self.lock();
        if let Some(l) = s.langs.get_mut(&lang) {
            typing::learn(l, &prev, &word, strong);
        }
    }

    /// Writes the owner's words to disk (the keyboard calls it when a field closes).
    pub fn save(&self) {
        let s = self.lock();
        for (lang, l) in &s.langs {
            if let Ok(t) = serde_json::to_string(&l.user) {
                let _ = fs::write(self.user_path(lang), t);
            }
        }
    }

    pub fn settings_json(&self) -> String {
        self.lock().settings.to_json()
    }

    pub fn settings_schema(&self) -> String {
        settings::schema()
    }

    /// One setting as JSON.
    pub fn setting(&self, key: String) -> Result<String, KeysError> {
        self.lock().settings.get(&key).map(|v| v.to_string()).ok_or_else(|| KeysError::Bad(format!("no setting called {key}; `keys schema` lists them")))
    }

    pub fn setting_bool(&self, key: String) -> bool {
        self.lock().settings.bool(&key)
    }

    pub fn setting_strings(&self, key: String) -> Vec<String> {
        self.lock().settings.strings(&key)
    }

    /// Sets one setting from JSON (`true`, `105`, `["fr","ja"]`); a bare
    /// word is taken as a string. Returns the stored value.
    pub fn set_setting(&self, key: String, value: String) -> Result<String, KeysError> {
        let v: serde_json::Value = serde_json::from_str(&value).unwrap_or(serde_json::Value::String(value));
        let mut s = self.lock();
        s.settings.set(&key, v.clone()).map_err(KeysError::Bad)?;
        self.save_settings(&s.settings);
        let langs = self.typing_languages_of(&s.settings);
        if !langs.contains(&s.current) {
            s.current = langs[0].clone();
        }
        drop(s);
        self.log.write("info", "setting", &serde_json::json!({"key": key, "value": v}).to_string());
        Ok(v.to_string())
    }

    // --- learning ---

    /// The folder dictionaries are installed into (IPAdic goes in `ipadic/` there).
    pub fn dictionary_dir(&self) -> String {
        learn::Learn::dict_dir(&self.dir).to_string_lossy().into()
    }

    pub fn dictionary_ready(&self, lang: String) -> bool {
        self.learn.words.ready(&lang)
    }

    /// Packs a downloaded dictionary file (the release's `<lang>.tsv.gz`, or `cedict.txt.gz`).
    pub fn install_dictionary(&self, lang: String, path: String) -> Result<(), KeysError> {
        self.learn.words.install(&lang, std::path::Path::new(&path)).map_err(|e| KeysError::Bad(e.to_string()))
    }

    /// Japanese pitch accents (`ja-pitch.tsv.gz`).
    pub fn install_pitch(&self, path: String) -> Result<(), KeysError> {
        self.learn.words.install_pitch(std::path::Path::new(&path)).map_err(|e| KeysError::Bad(e.to_string()))
    }

    /// A sentence's words with readings and the owner's status for each.
    pub fn strip_words(&self, lang: String, text: String) -> Vec<StripWord> {
        self.learn.tokens(&lang, &text)
    }

    pub fn word_card(&self, lang: String, word: String, lemma: String) -> Option<Card> {
        self.learn.card(&lang, &word, &lemma)
    }

    pub fn word_status(&self, lang: String, word: String) -> WordStatus {
        self.learn.statuses.status(&lang, &word).into()
    }

    pub fn set_word_status(&self, lang: String, word: String, status: WordStatus) {
        self.learn.statuses.set(&lang, &word, status.into());
    }

    /// Statuses to and from ccez-words (`words.server`). Blocks: call off the main thread.
    pub fn sync_words(&self, from: String) -> Result<u32, KeysError> {
        let server = self.lock().settings.get("words.server").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if server.is_empty() {
            return Err(KeysError::Bad("no words.server set".into()));
        }
        self.learn.sync(&server, &from).map_err(KeysError::Bad)
    }

    /// `ctx` is JSON. Never typed text.
    pub fn log(&self, level: String, event: String, ctx: String) {
        self.log.write(&level, &event, &ctx);
    }

    pub fn log_tail(&self, lines: u32) -> String {
        self.log.tail(lines as usize)
    }
}
