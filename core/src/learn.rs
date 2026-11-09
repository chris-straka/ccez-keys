//! The learning side, over ccez-lang-core: a sentence's words with their
//! readings and the owner's status for each (the strip), a word's card,
//! statuses set and synced with ccez-words. Never logs words.

use std::path::{Path, PathBuf};

use ccez_lang_core::status::{Status, Statuses};
use ccez_lang_core::words::Words;

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum WordStatus {
    New,
    Learning,
    Known,
}

impl From<Status> for WordStatus {
    fn from(s: Status) -> Self {
        match s {
            Status::New => WordStatus::New,
            Status::Learning => WordStatus::Learning,
            Status::Known => WordStatus::Known,
        }
    }
}

impl From<WordStatus> for Status {
    fn from(s: WordStatus) -> Self {
        match s {
            WordStatus::New => Status::New,
            WordStatus::Learning => Status::Learning,
            WordStatus::Known => Status::Known,
        }
    }
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Ruby {
    pub text: String,
    /// Furigana or pinyin over `text`; empty when none.
    pub rt: String,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct StripWord {
    pub text: String,
    /// A word (not a space or a mark).
    pub word: bool,
    /// The dictionary form, when it differs.
    pub lemma: String,
    pub ruby: Vec<Ruby>,
    pub status: WordStatus,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Card {
    pub word: String,
    pub lemma: String,
    pub reading: String,
    pub gender: String,
    pub article: String,
    /// Parts of speech, in order, without repeats.
    pub pos: Vec<String>,
    /// The first few meanings.
    pub meanings: Vec<String>,
    pub credit: String,
    pub status: WordStatus,
}

pub struct Learn {
    pub words: Words,
    pub statuses: Statuses,
    rt: tokio::runtime::Runtime,
}

impl Learn {
    pub fn new(dir: &Path) -> Self {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
        Learn { words: Words::new(dir.join("dict")), statuses: Statuses::new(dir.join("statuses")), rt }
    }

    pub fn dict_dir(dir: &Path) -> PathBuf {
        dir.join("dict")
    }

    fn status_of(&self, lang: &str, word: &str, lemma: &str) -> WordStatus {
        let by_lemma = if lemma.is_empty() { Status::New } else { self.statuses.status(lang, lemma) };
        if by_lemma != Status::New { by_lemma.into() } else { self.statuses.status(lang, word).into() }
    }

    pub fn tokens(&self, lang: &str, text: &str) -> Vec<StripWord> {
        self.words
            .tokens(lang, text)
            .into_iter()
            .map(|t| {
                let lemma = t.lemma.clone().unwrap_or_default();
                let status = if t.word { self.status_of(lang, &t.text, &lemma) } else { WordStatus::New };
                let ruby = if t.ruby.is_empty() {
                    vec![Ruby { text: t.text.clone(), rt: String::new() }]
                } else {
                    t.ruby.iter().map(|r| Ruby { text: r.text.clone(), rt: r.rt.clone().unwrap_or_default() }).collect()
                };
                StripWord { text: t.text, word: t.word, lemma, ruby, status }
            })
            .collect()
    }

    pub fn card(&self, lang: &str, word: &str, lemma: &str) -> Option<Card> {
        let e = self.words.lookup(lang, word, if lemma.is_empty() { None } else { Some(lemma) })?;
        let mut pos: Vec<String> = Vec::new();
        for s in &e.senses {
            if !s.pos.is_empty() && !pos.contains(&s.pos) {
                pos.push(s.pos.clone());
            }
        }
        // One meaning per sense first ("est": east, then "third-person singular
        // of être"), no repeats, then the rest.
        let mut meanings: Vec<String> = Vec::new();
        // A form's note carries its word's meaning along: "… of être (to be)".
        let first = |s: &ccez_lang_core::words::Sense| -> Option<String> {
            let m = s.meanings.first()?;
            match s.meanings.get(1) {
                Some(next) if m.contains(" of ") && m.split_whitespace().count() <= 8 => {
                    // Without a leading label: "(copulative) to be" → "to be".
                    let next = next.strip_prefix('(').and_then(|r| r.split_once(") ")).map_or(next.as_str(), |(_, rest)| rest);
                    Some(format!("{m} ({next})"))
                }
                _ => Some(m.clone()),
            }
        };
        let firsts: Vec<String> = e.senses.iter().filter_map(first).collect();
        let rest = e.senses.iter().flat_map(|s| s.meanings.iter().skip(1).cloned());
        for m in firsts.into_iter().chain(rest) {
            let m = &m;
            if meanings.len() == 4 {
                break;
            }
            if !meanings.iter().any(|x| x.eq_ignore_ascii_case(m)) {
                meanings.push(m.clone());
            }
        }
        let key_lemma = e.lemma.clone().unwrap_or_default();
        Some(Card {
            status: self.status_of(lang, word, &key_lemma),
            word: e.word,
            lemma: key_lemma,
            reading: e.reading.unwrap_or_default(),
            gender: e.gender.unwrap_or_default(),
            article: e.article.unwrap_or_default(),
            pos,
            meanings,
            credit: e.source,
        })
    }

    pub fn block_on<F: std::future::Future>(&self, f: F) -> F::Output {
        self.rt.block_on(f)
    }

    /// Pulls and pushes statuses; returns how many came down.
    pub fn sync(&self, server: &str, from: &str) -> Result<u32, String> {
        self.rt.block_on(self.statuses.sync(server, from)).map(|v| v.len() as u32).map_err(|e| e.to_string())
    }
}
