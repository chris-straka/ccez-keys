//! A word's meaning in its sentence, for its card: Apple's on-device model
//! through the Swift helper (free, offline), asked once per word and
//! sentence and kept on disk. Nothing comes back while the model isn't
//! there (Apple Intelligence off, or still downloading): the card keeps the
//! dictionary's meaning.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};

/// Answers kept (the oldest go past this).
const KEEP: usize = 500;

struct Helper {
    _child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

pub struct Meanings {
    file: PathBuf,
    cache: Mutex<Vec<(String, String)>>,
    helper: Mutex<Option<Helper>>,
    /// The newest answer: (cache key, meaning), taken by the app.
    pub out: Arc<Mutex<Option<(String, String)>>>,
}

pub fn key(lang: &str, word: &str, sentence: &str) -> String {
    format!("{lang}\u{1f}{word}\u{1f}{sentence}")
}

impl Meanings {
    pub fn new(dir: &std::path::Path) -> Arc<Meanings> {
        let file = dir.join("meanings.json");
        let cache: Vec<(String, String)> = std::fs::read_to_string(&file).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        Arc::new(Meanings { file, cache: Mutex::new(cache), helper: Mutex::new(None), out: Arc::default() })
    }

    /// The meaning if it's been asked before.
    pub fn known(&self, key: &str) -> Option<String> {
        self.cache.lock().unwrap().iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    }

    /// Asks in the background; the answer lands in `out`.
    pub fn ask(self: &Arc<Self>, lang: String, word: String, sentence: String) {
        let me = self.clone();
        std::thread::spawn(move || {
            let k = key(&lang, &word, &sentence);
            let Some(text) = me.request(&lang, &word, &sentence) else { return };
            {
                let mut c = me.cache.lock().unwrap();
                c.retain(|(old, _)| *old != k);
                c.push((k.clone(), text.clone()));
                let over = c.len().saturating_sub(KEEP);
                c.drain(..over);
                // Written aside, then moved into place: never half a file.
                let tmp = me.file.with_extension("tmp");
                if serde_json::to_vec(&*c).ok().and_then(|b| std::fs::write(&tmp, b).ok()).is_some() {
                    let _ = std::fs::rename(&tmp, &me.file);
                }
            }
            *me.out.lock().unwrap() = Some((k, text));
        });
    }

    fn request(&self, lang: &str, word: &str, sentence: &str) -> Option<String> {
        let mut h = self.helper.lock().unwrap();
        if h.is_none() {
            let path = std::env::current_exe().ok()?.with_file_name("ccez-keys-translate");
            let mut child = Command::new(path).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
            let stdin = child.stdin.take()?;
            let stdout = BufReader::new(child.stdout.take()?);
            *h = Some(Helper { _child: child, stdin, stdout });
        }
        let helper = h.as_mut()?;
        let ask = serde_json::json!({"id": 1, "meaning": {"lang": lang, "word": word, "sentence": sentence}});
        let mut line = String::new();
        let ok = writeln!(helper.stdin, "{ask}").is_ok() && helper.stdin.flush().is_ok() && helper.stdout.read_line(&mut line).is_ok_and(|n| n > 0);
        if !ok {
            *h = None;
            return None;
        }
        let v: serde_json::Value = serde_json::from_str(&line).ok()?;
        v.get("text").and_then(|t| t.as_str()).map(str::to_string).filter(|t| !t.is_empty())
    }
}

