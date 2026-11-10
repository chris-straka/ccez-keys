//! Mozilla's Firefox Translations models through fxtranslate (pure Rust),
//! the translator on Linux and Windows: English into each language, offline.
//! The models (MPL-2.0, ~35-50 MB a language) come from this repo's
//! models-firefox-1 release, checked against its SHA256SUMS, on first use.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use fxtranslate::engine::Engine;

use crate::worker::{Translated, Translator};

const BASE: &str = "https://github.com/chris-straka/ccez-keys/releases/download/models-firefox-1/";

/// The models' name for a language ("zh" is Simplified Chinese).
fn model_lang(lang: &str) -> &str {
    match lang {
        "zh" | "zh-CN" => "zh-Hans",
        l => l,
    }
}

pub struct Firefox {
    dir: PathBuf,
    engines: HashMap<String, Engine>,
    sums: Option<Vec<(String, String)>>,
}

impl Firefox {
    pub fn new(dir: &Path) -> Firefox {
        let _ = std::fs::create_dir_all(dir);
        Firefox { dir: dir.to_path_buf(), engines: HashMap::new(), sums: None }
    }

    fn sums(&mut self) -> Result<&Vec<(String, String)>, String> {
        if self.sums.is_none() {
            let file = self.dir.join("SHA256SUMS");
            if !file.exists() && !crate::learn::fetch(&format!("{BASE}SHA256SUMS"), &file) {
                return Err("couldn't reach the models".into());
            }
            let text = std::fs::read_to_string(&file).map_err(|e| e.to_string())?;
            self.sums = Some(text.lines().filter_map(|l| l.split_once("  ").map(|(h, n)| (n.trim().to_string(), h.to_string()))).collect());
        }
        Ok(self.sums.as_ref().unwrap())
    }

    /// `name` here and whole (fetched and checked if not).
    fn file(&mut self, name: &str) -> Result<PathBuf, String> {
        let path = self.dir.join(name);
        let want = self.sums()?.iter().find(|(n, _)| n == name).map(|(_, h)| h.clone()).ok_or(format!("no model {name}"))?;
        if path.exists() && crate::learn::sha256(&path).as_deref() == Some(want.as_str()) {
            return Ok(path);
        }
        let tmp = path.with_extension("part");
        if !crate::learn::fetch(&format!("{BASE}{name}"), &tmp) || crate::learn::sha256(&tmp).as_deref() != Some(want.as_str()) {
            let _ = std::fs::remove_file(&tmp);
            return Err(format!("couldn't fetch {name}"));
        }
        std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
        Ok(path)
    }

    fn engine(&mut self, lang: &str) -> Result<&Engine, String> {
        let l = model_lang(lang).to_string();
        if !self.engines.contains_key(&l) {
            let model = self.file(&format!("en-{l}.model.bin"))?;
            let src = self.file(&format!("en-{l}.src.spm"))?;
            let trg = self.file(&format!("en-{l}.trg.spm"))?;
            let e = Engine::load(&model, &src, &trg)?;
            self.engines.insert(l.clone(), e);
        }
        Ok(&self.engines[&l])
    }
}

/// A question typed without its "?" ("Where is the station"): these models
/// drop the verb without it (Korean "기차역은 어디"), so it's added.
pub fn as_typed(text: &str) -> String {
    const ASKING: &[&str] = &[
        "who", "what", "where", "when", "why", "how", "which", "whose", "is", "are", "am", "was", "were", "can", "could", "do", "does", "did",
        "will", "would", "should", "shall", "may", "might", "have", "has",
    ];
    let t = text.trim_end();
    let first = t.split_whitespace().next().unwrap_or("").to_lowercase();
    if ASKING.contains(&first.as_str()) && !t.ends_with(['.', '!', '?', ',', ';', ':']) && t.split_whitespace().count() >= 3 { format!("{t}?") } else { t.to_string() }
}

impl Translator for Firefox {
    fn translate(&mut self, from: &str, to: &str, text: &str) -> Result<Translated, String> {
        if from != "en" {
            return Err(format!("only from English ({from})"));
        }
        let e = self.engine(to)?;
        // Readings for Japanese and Chinese come from the core's dictionaries (worker::toks).
        Ok((e.translate(&as_typed(text)), Vec::new()))
    }
}

#[cfg(test)]
mod tests {
    use super::as_typed;

    #[test]
    fn questions_get_their_mark() {
        assert_eq!(as_typed("Where is the train station"), "Where is the train station?");
        assert_eq!(as_typed("Can you help me"), "Can you help me?");
        assert_eq!(as_typed("Where is it."), "Where is it.");
        assert_eq!(as_typed("I am going home"), "I am going home");
        assert_eq!(as_typed("Is it"), "Is it");
    }
}
