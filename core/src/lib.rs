//! ccez-keys' core, for the Android keyboard through UniFFI: words and
//! autocorrect (`words`, `typing`), layouts from data files (`layout`),
//! every setting in one table (`settings`), and a log agents can read
//! (`log`). The keyboard service draws and handles touch; everything it
//! decides about text, it asks here.

pub mod ai;
pub mod convert;
pub mod gloss;
pub mod layout;
pub mod learn;
pub mod log;
pub mod review;
pub mod settings;
pub mod swipe;
pub mod telex;
pub mod text;
pub mod typing;
pub mod words;

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

pub use ai::{AnswerSink, ChatMessage, Fix, FixKind, FixPart};
pub use convert::Conversion;
pub use layout::{Key, KeyKind, Layout, Page, Row};
pub use learn::{Card, Ruby, StripWord, WordStatus};
pub use review::Grade;
pub use typing::Suggestions;

uniffi::setup_scaffolding!();

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum KeysError {
    #[error("{0}")]
    Bad(String),
}

/// Written whole or not at all: a kill mid-write (an update installing)
/// would otherwise leave a cut file, read back as empty settings or words.
fn write_safely(path: &std::path::Path, text: impl AsRef<[u8]>) {
    let tmp = path.with_extension("tmp");
    if fs::write(&tmp, text).is_ok() {
        let _ = fs::rename(&tmp, path);
    }
}

struct State {
    langs: HashMap<String, typing::Lang>,
    keys: HashMap<String, words::Neighbours>,
    settings: settings::Settings,
    current: String,
}

/// A word saved as Learning, and its language.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct LearningWord {
    pub lang: String,
    pub word: String,
}

/// A word to put on screen: as it shows ("parvenir à"), its short meaning
/// and its tags ("tr") as small pills.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ScreenWord {
    pub lang: String,
    pub word: String,
    pub shown: String,
    pub meaning: String,
    pub tags: Vec<String>,
}

#[derive(uniffi::Object)]
pub struct Engine {
    dir: PathBuf,
    layouts: layout::Layouts,
    log: log::Log,
    learn: learn::Learn,
    /// When each screen word comes back (screen-words.json in `dir`).
    review: Mutex<review::Review>,
    /// Where the next screen words carry on from in the list.
    screen_seed: std::sync::atomic::AtomicU64,
    converters: Mutex<HashMap<String, convert::Index>>,
    /// Converters being read from disk now.
    loading: Mutex<std::collections::HashSet<String>>,
    state: Mutex<State>,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Romaji {
    pub kana: String,
    /// Letters still waiting for a vowel.
    pub pending: String,
}

impl Engine {
    /// A saved word as a learner should see it: (shown, meaning, tags), the
    /// meaning empty when the dictionary can't place it. Saved forms are
    /// mended first: a German noun saved in lower case ("urteil") is the noun
    /// ("das Urteil"), not the verb form the lower case also spells; an
    /// elided article or "de" goes ("l'ardoise" → "une ardoise",
    /// "d'adoucir" → "adoucir"); a word cut off when saved ("rechtskräft")
    /// is the whole word when one common ending makes one; a phrase
    /// ("découler de") takes its first word's meaning. A noun in its own
    /// form gets its article ("le cap", "das Wasser"); a verb never does.
    pub fn word_face(&self, lang: &str, word: &str) -> (String, String, Vec<String>) {
        let card = |w: &str| self.learn.card(lang, w, "");
        let meanings = |w: &str| card(w).map(|c| c.meanings).unwrap_or_default();
        let base = lang.split(['-', '_']).next().unwrap_or(lang);
        let mut w = word.trim().to_string();
        if matches!(base, "fr" | "it") {
            for lead in ["l'", "d'", "qu'", "j'", "m'", "n'", "t'", "c'", "l’", "d’", "qu’", "j’", "m’", "n’", "t’", "c’"] {
                if let Some(rest) = w.strip_prefix(lead)
                    && !rest.is_empty()
                    && !meanings(rest).is_empty()
                {
                    w = rest.to_string();
                    break;
                }
            }
        }
        if base == "de" && w.chars().next().is_some_and(char::is_lowercase) {
            let mut cs = w.chars();
            let cap: String = cs.next().map(|c| c.to_uppercase().chain(cs).collect()).unwrap_or_default();
            let noun = card(&cap).filter(|c| !c.article.is_empty() && c.pos.iter().any(|p| p == "noun") && (c.lemma.is_empty() || c.lemma == cap));
            let low = card(&w);
            // The lower case is a word of its own ("fressen") unless it's
            // only a form of another ("urteil" of urteilen) or the
            // dictionary already took it for the noun ("spur" → Spur).
            let own = low.as_ref().is_some_and(|c| (c.lemma.is_empty() || c.lemma == w) && c.word == w && !c.pos.iter().all(|p| p == "noun"));
            if noun.is_some() && !own {
                w = cap;
            }
        }
        // The dictionary's own capitals ("Eau" saved from the start of a
        // sentence is "eau"; "spur" is "Spur").
        if let Some(c) = card(&w)
            && c.word != w
            && c.word.to_lowercase() == w.to_lowercase()
        {
            w = c.word;
        }
        if meanings(&w).is_empty() && w.chars().count() >= 6 && !w.contains(' ') {
            const ENDINGS: [&str; 8] = ["ig", "e", "en", "er", "lich", "ung", "es", "s"];
            if let Some(whole) = ENDINGS.iter().map(|e| format!("{w}{e}")).find(|x| !meanings(x).is_empty()) {
                w = whole;
            }
        }
        if meanings(&w).is_empty()
            && let Some((head, _)) = w.split_once(' ')
            && !meanings(head).is_empty()
        {
            let (_, meaning, tags) = gloss::short_gloss(head, &meanings(head), meanings);
            return (w, meaning, tags);
        }
        let (shown, meaning, tags) = gloss::short_gloss(&w, &meanings(&w), meanings);
        if shown != w || meaning.is_empty() {
            return (shown, meaning, tags);
        }
        let head = match card(&w) {
            Some(c) if !c.article.is_empty() && c.pos.first().is_some_and(|p| p == "noun") && (c.lemma.is_empty() || c.lemma.eq_ignore_ascii_case(&w)) => {
                let shown = if base == "de" { c.word.clone() } else { w.clone() };
                if c.article.ends_with('\'') || c.article.ends_with('’') { format!("{}{shown}", c.article) } else { format!("{} {shown}", c.article) }
            }
            _ => w,
        };
        (head, meaning, tags)
    }

    /// The screen words' schedule, for Rust callers (the Mac app): the one
    /// copy, shared with its sync, so neither overwrites the other's answers.
    pub fn with_review<T>(&self, f: impl FnOnce(&mut review::Review) -> T) -> T {
        f(&mut self.review.lock().unwrap_or_else(|e| e.into_inner()))
    }

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

    fn provider(&self) -> Result<ai::Provider, KeysError> {
        let id = self.lock().settings.get("ai.provider").and_then(|v| v.as_str()).unwrap_or("muse").to_string();
        ai::provider(&id).ok_or_else(|| KeysError::Bad(format!("no AI provider called {id}")))
    }

    fn converter_path(&self, lang: &str) -> PathBuf {
        // v2: each word carries its frequency (for cutting input into words).
        self.dir.join(format!("convert-{lang}.v2.tsv"))
    }

    fn save_settings(&self, s: &settings::Settings) {
        write_safely(&self.dir.join("settings.json"), s.to_json());
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
            review: Mutex::new(review::Review::load(&dir)),
            screen_seed: std::sync::atomic::AtomicU64::new(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() % 997).unwrap_or(0)),
            converters: Mutex::new(HashMap::new()),
            loading: Mutex::new(std::collections::HashSet::new()),
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
        let mut l = typing::Lang::new(lex, user);
        l.fixes = typing::fixes(&lang);
        let mut s = self.lock();
        s.langs.insert(lang.clone(), l);
        s.keys.insert(lang, keys);
        n
    }

    /// Frees a language's words (saving the owner's own first).
    pub fn unload(&self, lang: String) {
        let mut s = self.lock();
        if let Some(l) = s.langs.remove(&lang) {
            if let Ok(t) = serde_json::to_string(&l.user) {
                write_safely(&self.user_path(&lang), t);
            }
        }
        s.keys.remove(&lang);
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
        write_safely(&self.dir.join("current-language"), &lang);
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
        self.suggest_in(lang, typed, String::new())
    }

    /// Suggestions for the word being typed, ordered with the text before
    /// it in mind (`before`: up to the cursor, the word at its end): the
    /// word before, and the paragraph's words.
    pub fn suggest_in(&self, lang: String, typed: String, before: String) -> Suggestions {
        let ctx = typing::Context::of(&before, &typed);
        let s = self.lock();
        let others_langs = self.typing_languages_of(&s.settings);
        // Thai is written without spaces between words: suggestions, no corrections.
        let auto = s.settings.bool("typing.autocorrect") && lang != "th";
        let Some(l) = s.langs.get(&lang) else {
            return Suggestions { words: vec![], correction: None, typed_known: false };
        };
        let others: Vec<&typing::Lang> = others_langs.iter().filter(|o| **o != lang).filter_map(|o| s.langs.get(o)).collect();
        let empty = words::Neighbours::default();
        let keys = s.keys.get(&lang).unwrap_or(&empty);
        typing::suggest_in(l, &others, &typed, &ctx, keys, auto)
    }

    /// Whether `word` is a word of `lang` as written (the lists or the
    /// owner's own), and not taken out of the suggestions.
    pub fn knows_word(&self, lang: String, word: String) -> bool {
        self.lock().langs.get(&lang).is_some_and(|l| l.knows(&word))
    }

    /// Takes `word` out of `lang`'s suggestions (and what typing taught of it).
    pub fn remove_word(&self, lang: String, word: String) {
        let mut s = self.lock();
        if let Some(l) = s.langs.get_mut(&lang) {
            typing::remove(l, &word);
        }
    }

    /// The language the last words were written in, when it isn't
    /// `current`: all of them common there, and together far likelier
    /// there than here ("ich bin": German, though "bin" is English too).
    pub fn detect_language(&self, current: String, words: Vec<String>) -> Option<String> {
        if words.is_empty() {
            return None;
        }
        let s = self.lock();
        // One word is enough when it's no word here and common there
        // ("chez", "merci", "gracias" on English).
        if let [w] = words.as_slice() {
            let here = s.langs.get(&current)?;
            if w.chars().count() < 3 || typing::is_word(here, w) {
                return None;
            }
            let found: Vec<String> = self.typing_languages_of(&s.settings).into_iter().filter(|l| *l != current).filter(|l| s.langs.get(l).is_some_and(|x| typing::very_common(x, w))).collect();
            // Only when one language claims it.
            return if found.len() == 1 { found.into_iter().next() } else { None };
        }
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

    /// `typed` on another keyboard, when it's Japanese in romaji
    /// ("konnichiha"): its kana (こんにちは). Only for a word of four letters
    /// or more that is no word in any other language loaded, and whose kana
    /// is a Japanese word. Needs Japanese among those typed, its words loaded.
    pub fn romaji_word(&self, current: String, typed: String) -> Option<String> {
        if current == "ja" || typed.chars().count() < 4 || !typed.chars().all(|c| c.is_ascii_alphabetic()) {
            return None;
        }
        let s = self.lock();
        if !self.typing_languages_of(&s.settings).iter().any(|l| l == "ja") {
            return None;
        }
        let ja = s.langs.get("ja")?;
        if s.langs.iter().any(|(l, x)| l != "ja" && typing::is_word(x, &typed)) {
            return None;
        }
        let (kana, _) = convert::romaji(&typed, true);
        (kana.chars().count() >= 3 && !kana.chars().any(|c| c.is_ascii()) && typing::is_word(ja, &kana)).then_some(kana)
    }

    /// Whether `word` is among the commonest 10,000 of `lang` (true when its words aren't loaded).
    pub fn common_word(&self, lang: String, word: String) -> bool {
        self.lock().langs.get(&lang).map(|l| typing::common(l, &word)).unwrap_or(true)
    }

    /// The words likely next (data/next/<lang>.txt), for a language whose words are loaded.
    pub fn load_next(&self, lang: String, text: String) -> u32 {
        let next = typing::parse_next(&text);
        let n = next.len() as u32;
        if let Some(l) = self.lock().langs.get_mut(&lang) {
            l.next = next;
        }
        n
    }

    /// Words for a swipe over `path` (the letters passed, in order).
    pub fn swipe(&self, lang: String, path: String) -> Vec<String> {
        let s = self.lock();
        s.langs.get(&lang).map(|l| swipe::decode(&l.lex, &path, 4)).unwrap_or_default()
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

    /// Forgets a word the owner's typing taught `lang` (and what followed it).
    /// The list is unloaded: load it again for the change to show. True when it was there.
    pub fn forget(&self, lang: String, word: String) -> bool {
        let mut s = self.lock();
        let Some(mut l) = s.langs.remove(&lang) else {
            let mut user: typing::UserWords = fs::read_to_string(self.user_path(&lang)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
            let had = typing::forget(&mut user, &word);
            if had {
                if let Ok(t) = serde_json::to_string(&user) {
                    write_safely(&self.user_path(&lang), t);
                }
            }
            return had;
        };
        let had = typing::forget(&mut l.user, &word);
        if let Ok(t) = serde_json::to_string(&l.user) {
            write_safely(&self.user_path(&lang), t);
        }
        had
    }

    /// Writes the owner's words to disk (the keyboard calls it when a field closes).
    pub fn save(&self) {
        let s = self.lock();
        for (lang, l) in &s.langs {
            if let Ok(t) = serde_json::to_string(&l.user) {
                write_safely(&self.user_path(lang), t);
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

    /// The words saved as Learning, in the learning languages (in the owner's
    /// order), for the home-screen widget.
    pub fn learning_words(&self) -> Vec<LearningWord> {
        let langs = self.lock().settings.strings("languages.learning");
        langs
            .into_iter()
            .flat_map(|lang| {
                let mut words = self.learn.statuses.words(&lang, ccez_lang_core::status::Status::Learning);
                words.sort();
                words.into_iter().map(move |word| LearningWord { lang: lang.clone(), word })
            })
            .collect()
    }

    /// By the word's headword, the form every app saves it under
    /// (`Words::headword`): "freiner" known covers "freinent".
    pub fn word_status(&self, lang: String, word: String) -> WordStatus {
        self.learn.status_of(&lang, &word, "")
    }

    /// Saved under its headword; a word with none (a number, a name, a
    /// fragment) saves nothing.
    pub fn set_word_status(&self, lang: String, word: String, status: WordStatus) {
        self.learn.set_status(&lang, &word, status.into());
    }

    /// Statuses to and from ccez-words (`words.server`), and with them the
    /// screen words' schedule (best effort: a server without `/review`
    /// keeps each device its own). Blocks: call off the main thread.
    pub fn sync_words(&self, from: String) -> Result<u32, KeysError> {
        let server = self.lock().settings.get("words.server").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if server.is_empty() {
            return Err(KeysError::Bad("no words.server set".into()));
        }
        let came = self.learn.sync(&server, &from).map_err(KeysError::Bad)?;
        match self.learn.block_on(review::sync(&server, &self.review)) {
            Ok(n) => self.log.write("info", "review-sync", &serde_json::json!({"came": n}).to_string()),
            Err(e) => self.log.write("warn", "review-sync-failed", &serde_json::json!({"error": e}).to_string()),
        }
        Ok(came)
    }

    // --- words on screen ---

    /// Words to put on screen now (up to `learning.screen` of them, from
    /// the first `learning.screen_langs` learning languages, the languages
    /// taking turns), never one in `up` already or not yet due. Each with
    /// its short meaning. Dictionary lookups: call off the main thread.
    pub fn screen_words(&self, up: Vec<LearningWord>) -> Vec<ScreenWord> {
        let (count, langs_limit, langs) = {
            let s = self.lock();
            let count = s.settings.get("learning.screen").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let limit = s.settings.get("learning.screen_langs").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            (count, limit, s.settings.strings("languages.learning"))
        };
        if count == 0 {
            return Vec::new();
        }
        let keep: Vec<&String> = if langs_limit == 0 { langs.iter().collect() } else { langs.iter().take(langs_limit).collect() };
        let words: Vec<(String, String)> = self.learning_words().into_iter().filter(|w| keep.iter().any(|l| **l == w.lang)).map(|w| (w.lang, w.word)).collect();
        let up: Vec<(String, String)> = up.into_iter().map(|w| (w.lang, w.word)).collect();
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let seed = self.screen_seed.fetch_add(1, std::sync::atomic::Ordering::Relaxed) as usize;
        let picked = {
            let r = self.review.lock().unwrap_or_else(|e| e.into_inner());
            r.next(&review::mixed(words), &up, count.saturating_sub(up.len()), now, seed)
        };
        picked
            .into_iter()
            .map(|(lang, word)| {
                let (shown, meaning, tags) = self.word_face(&lang, &word);
                ScreenWord { lang, word, shown, meaning, tags }
            })
            .collect()
    }

    /// A screen word graded: Good later and later, Easy two steps, Again
    /// in two minutes. Syncs with the word statuses (`sync_words`).
    pub fn grade_screen_word(&self, lang: String, word: String, grade: Grade) {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        self.review.lock().unwrap_or_else(|e| e.into_inner()).grade(&lang, &word, grade, now);
    }

    // --- Japanese and Chinese ---

    pub fn has_converter(&self, lang: String) -> bool {
        self.converter_path(&lang).exists()
    }

    /// Builds `lang`'s converter (ja from JMdict's ja.tsv.gz, zh from
    /// cedict.txt.gz) with wordfreq's list (`freq`) for the order.
    pub fn build_converter(&self, lang: String, dict_path: String, freq: String) -> Result<u32, KeysError> {
        let out = self.converter_path(&lang);
        // Built beside it and renamed, so a build cut short never leaves half a file.
        let tmp = out.with_extension("tmp");
        let n = match lang.as_str() {
            "ja" => convert::build_japanese(std::path::Path::new(&dict_path), &freq, &tmp),
            "zh" => convert::build_chinese(std::path::Path::new(&dict_path), &freq, &tmp),
            _ => return Err(KeysError::Bad(format!("no converter for {lang}"))),
        }
        .map_err(|e| KeysError::Bad(e.to_string()))?;
        fs::rename(&tmp, &out).map_err(|e| KeysError::Bad(e.to_string()))?;
        self.converters.lock().unwrap_or_else(|e| e.into_inner()).remove(&lang);
        self.log.write("info", "converter", &serde_json::json!({"lang": lang, "keys": n}).to_string());
        Ok(n as u32)
    }

    /// Vietnamese typed with Telex ("Vieetj" → Việt).
    pub fn telex(&self, raw: String) -> String {
        telex::telex(&raw)
    }

    /// Romaji typed so far, as kana (`finish`: a last "n" counts as ん).
    pub fn romaji(&self, input: String, finish: bool) -> Romaji {
        let (kana, pending) = convert::romaji(&input, finish);
        Romaji { kana, pending }
    }

    /// Words for kana (Japanese) or pinyin letters (Chinese).
    pub fn convert(&self, lang: String, input: String) -> Vec<Conversion> {
        // The kana as typed, while there's no converter (yet).
        let plain = || if lang == "ja" && !input.is_empty() { vec![Conversion { text: input.clone(), reading: String::new(), consumed: input.chars().count() as u32 }] } else { vec![] };
        {
            let c = self.converters.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(i) = c.get(&lang) {
                return i.convert(&input, lang == "ja");
            }
        }
        // Loaded (several MB) outside the lock, by one caller at a time: a key
        // typed meanwhile gets the kana as typed rather than waiting.
        {
            let mut loading = self.loading.lock().unwrap_or_else(|e| e.into_inner());
            if !loading.insert(lang.clone()) {
                return plain();
            }
        }
        let index = convert::Index::load(&self.converter_path(&lang));
        self.loading.lock().unwrap_or_else(|e| e.into_inner()).remove(&lang);
        let Some(index) = index else { return plain() };
        let out = index.convert(&input, lang == "ja");
        self.converters.lock().unwrap_or_else(|e| e.into_inner()).insert(lang, index);
        out
    }

    // --- AI ---

    /// `text`, written in the learning language `lang`, corrected; None when
    /// it needs nothing. Blocks: call off the main thread.
    pub fn correct(&self, lang: String, text: String, key: String) -> Result<Option<Fix>, KeysError> {
        let p = self.provider()?;
        let explain = self.lock().settings.get("languages.default").and_then(|v| v.as_str()).unwrap_or("en").to_string();
        let messages = ai::correction_messages(&lang, &text, &explain);
        let started = std::time::Instant::now();
        let answer = self.learn.block_on(ccez_lang_core::ai::ask(&p.url, &key, &p.model, &messages, p.thinking.as_deref(), |_| {})).map_err(|e| KeysError::Bad(e.to_string()))?;
        let fix = ai::to_fix(&text, &answer);
        self.log.write("info", "correction", &serde_json::json!({"lang": lang, "changed": fix.is_some(), "ms": started.elapsed().as_millis() as u64}).to_string());
        Ok(fix)
    }

    /// `text` (written in `from`) said naturally in `to`, casual or polite.
    /// Blocks: call off the main thread.
    pub fn translate_ai(&self, text: String, from: String, to: String, polite: bool, key: String) -> Result<String, KeysError> {
        let p = self.provider()?;
        let name = |l: &str| ccez_lang_core::correction::language_name(l).to_string();
        let system = format!(
            "Translate the user's message from {} into {}, the way a native speaker would text it, {}. Reply with the translation only: no quotes, no notes.",
            name(&from),
            name(&to),
            if polite { "politely (vous, Sie, usted, です/ます)" } else { "casually, as to a friend (tu, du, tú, plain form)" }
        );
        let m = vec![ccez_lang_core::ai::Message { role: "system".into(), content: system }, ccez_lang_core::ai::Message { role: "user".into(), content: text }];
        let r = self.learn.block_on(ccez_lang_core::ai::ask(&p.url, &key, &p.model, &m, p.thinking.as_deref(), |_| {}));
        r.map(|t| t.trim().trim_matches('"').to_string()).map_err(|e| KeysError::Bad(e.to_string()))
    }

    /// What `word` means in `context` (the strip's sentence), as ccez-lang
    /// glosses a word: its English equivalent for this sense on the first
    /// line, at most one short note on the next. Blocks: off the main thread.
    pub fn gloss(&self, context: String, word: String, key: String) -> Result<String, KeysError> {
        let p = self.provider()?;
        let m = ccez_lang_core::ai::ask_messages(&context, &word, None, false);
        let started = std::time::Instant::now();
        // The least thinking the provider allows: a card wants its line quickly.
        let r = self.learn.block_on(ccez_lang_core::ai::ask(&p.url, &key, &p.model, &m, p.thinking.as_deref(), |_| {}));
        self.log.write("info", "gloss", &serde_json::json!({"ok": r.is_ok(), "ms": started.elapsed().as_millis() as u64}).to_string());
        r.map(|t| t.trim().to_string()).map_err(|e| KeysError::Bad(e.to_string()))
    }

    /// A correction the owner took (for review later, in ccez-learn).
    pub fn note_mistake(&self, lang: String, written: String, fixed: String, why: String) {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
        let line = serde_json::json!({"t": t, "lang": lang, "written": written, "fixed": fixed, "why": why}).to_string();
        if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(self.dir.join("mistakes.jsonl")) {
            use std::io::Write;
            let _ = writeln!(f, "{line}");
        }
    }

    pub fn mistakes(&self) -> String {
        fs::read_to_string(self.dir.join("mistakes.jsonl")).unwrap_or_default()
    }

    /// The first messages of an Ask chat about what was typed.
    pub fn ask_start(&self, typed: String, question: String) -> Vec<ChatMessage> {
        ai::ask_start(&typed, &question)
    }

    /// The assistant's next answer to `messages`, streamed into `sink`;
    /// returns the whole answer. Blocks: call off the main thread.
    pub fn ask(&self, messages: Vec<ChatMessage>, key: String, sink: Box<dyn AnswerSink>) -> Result<String, KeysError> {
        let mut p = self.provider()?;
        // Explanations get the provider's usual thinking (Muse: medium);
        // corrections and the strip stay on the least, for speed.
        let id = self.lock().settings.get("ai.provider").and_then(|v| v.as_str()).unwrap_or("muse").to_string();
        p.thinking = ccez_lang_core::ai::provider(&id).and_then(|x| x.default_thinking).map(String::from).or(p.thinking);
        let m = ai::to_messages(&messages);
        // The crate hands over the whole answer so far each time.
        let r = self.learn.block_on(ccez_lang_core::ai::ask(&p.url, &key, &p.model, &m, p.thinking.as_deref(), |t| {
            sink.text(t.to_string());
        }));
        self.log.write("info", "ask", &serde_json::json!({"ok": r.is_ok(), "turns": messages.len()}).to_string());
        r.map_err(|e| KeysError::Bad(e.to_string()))
    }

    /// `ctx` is JSON. Never typed text.
    pub fn log(&self, level: String, event: String, ctx: String) {
        self.log.write(&level, &event, &ctx);
    }

    pub fn log_tail(&self, lines: u32) -> String {
        self.log.tail(lines as usize)
    }
}
