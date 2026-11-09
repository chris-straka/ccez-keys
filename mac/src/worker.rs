//! Translations off the main thread: Apple's on-device translator through
//! the bundled Swift helper (ccez-keys-translate), one sentence at a time,
//! always the newest asked for, answers kept for the session.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

/// A sentence to show in `langs`, in order.
pub struct Ask {
    pub seq: u64,
    pub text: String,
    pub from: String,
    pub langs: Vec<String>,
    /// How many lines the preview shows.
    pub max: usize,
}

/// What came back: each language's translation, in the order asked.
/// One language's line: its code and its words (with readings for Japanese
/// and Chinese, and each word's status).
#[derive(Clone)]
pub struct Line {
    pub lang: String,
    pub toks: Vec<crate::view::Tok>,
}

fn status(s: ccez_keys_core::WordStatus) -> u8 {
    match s {
        ccez_keys_core::WordStatus::Learning => 1,
        ccez_keys_core::WordStatus::Known => 2,
        _ => 0,
    }
}

/// The translation as words with statuses: Japanese and Chinese by macOS's
/// words (readings over them), the rest through the dictionary (a form counts
/// as its word).
fn toks(engine: &ccez_keys_core::Engine, lang: &str, text: &str, ruby: &[(String, String)]) -> Vec<crate::view::Tok> {
    use crate::view::Tok;
    if !ruby.is_empty() {
        return ruby
            .iter()
            .map(|(t, r)| {
                let word = t.chars().any(char::is_alphanumeric);
                let s = if word { status(engine.word_status(lang.into(), t.clone())) } else { 0 };
                Tok { text: t.clone(), reading: r.clone(), word, key: t.clone(), status: s }
            })
            .collect();
    }
    engine
        .strip_words(lang.into(), text.into())
        .into_iter()
        .map(|w| Tok {
            key: if w.lemma.is_empty() { w.text.clone() } else { w.lemma.clone() },
            text: w.text,
            reading: String::new(),
            word: w.word,
            status: status(w.status),
        })
        .collect()
}

#[derive(Clone)]
pub struct Shown {
    pub seq: u64,
    pub lines: Vec<Line>,
}

pub struct Worker {
    tx: Sender<Ask>,
    pub out: Arc<Mutex<Option<Shown>>>,
}

struct Helper {
    _child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    id: u64,
}

impl Helper {
    fn start() -> Option<Helper> {
        let exe = std::env::current_exe().ok()?;
        let path = exe.with_file_name("ccez-keys-translate");
        let mut child = Command::new(path).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
        let stdin = child.stdin.take()?;
        let stdout = BufReader::new(child.stdout.take()?);
        Some(Helper { _child: child, stdin, stdout, id: 0 })
    }

    fn translate(&mut self, from: &str, to: &str, text: &str) -> Result<(String, Vec<(String, String)>), String> {
        self.id += 1;
        let ask = serde_json::json!({"id": self.id, "from": from, "to": to, "text": text, "readings": true});
        writeln!(self.stdin, "{ask}").map_err(|e| e.to_string())?;
        self.stdin.flush().map_err(|e| e.to_string())?;
        let mut line = String::new();
        self.stdout.read_line(&mut line).map_err(|e| e.to_string())?;
        let v: serde_json::Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        let ruby = v
            .get("ruby")
            .and_then(|r| r.as_array())
            .map(|a| a.iter().filter_map(|p| Some((p.get(0)?.as_str()?.to_string(), p.get(1)?.as_str()?.to_string()))).collect())
            .unwrap_or_default();
        match v.get("text").and_then(|t| t.as_str()) {
            Some(t) => Ok((t.to_string(), ruby)),
            None => Err(v.get("error").and_then(|e| e.as_str()).unwrap_or("no answer").to_string()),
        }
    }
}

impl Worker {
    pub fn start(engine: std::sync::Arc<ccez_keys_core::Engine>, log: impl Fn(&str, &str) + Send + 'static) -> Worker {
        let (tx, rx): (Sender<Ask>, Receiver<Ask>) = channel();
        let out: Arc<Mutex<Option<Shown>>> = Arc::default();
        let put = out.clone();
        std::thread::spawn(move || {
            let mut helper = Helper::start();
            if helper.is_none() {
                log("error", "translator helper didn't start");
            }
            let mut cache: HashMap<(String, String), (String, Vec<(String, String)>)> = HashMap::new();
            let mut missing: Vec<String> = Vec::new();
            let mut next: Option<Ask> = None;
            loop {
                let Some(mut ask) = next.take().or_else(|| rx.recv().ok()) else { break };
                // Only the newest: typing moved on past the others.
                while let Ok(newer) = rx.try_recv() {
                    ask = newer;
                }
                let mut lines = Vec::new();
                'langs: for lang in &ask.langs {
                    // A newer sentence: this one's done with.
                    if let Ok(newer) = rx.try_recv() {
                        let mut n = newer;
                        while let Ok(more) = rx.try_recv() {
                            n = more;
                        }
                        next = Some(n);
                        break 'langs;
                    }
                    let key = (lang.clone(), ask.text.clone());
                    let got = if let Some(t) = cache.get(&key) {
                        Some(t.clone())
                    } else if let Some(h) = helper.as_mut() {
                        match h.translate(&ask.from, lang, &ask.text) {
                            Ok(t) => {
                                cache.insert(key, t.clone());
                                Some(t)
                            }
                            Err(e) => {
                                if !missing.contains(lang) {
                                    missing.push(lang.clone());
                                    log("warn", &format!("{lang}: {e}"));
                                }
                                if e.contains("Broken pipe") {
                                    helper = Helper::start();
                                }
                                None
                            }
                        }
                    } else {
                        None
                    };
                    if let Some((t, ruby)) = got.filter(|(t, _)| !t.trim().eq_ignore_ascii_case(ask.text.trim())) {
                        let toks = toks(&engine, lang, t.trim(), &ruby);
                        // Every word known: that language has nothing to teach here (as on the phone).
                        let words: Vec<_> = toks.iter().filter(|t| t.word).collect();
                        if words.is_empty() || !words.iter().all(|t| t.status == 2) {
                            lines.push(Line { lang: lang.clone(), toks });
                        }
                    }
                    // Each language as it comes (Apple's translator does one at a time, ~0.3 s each).
                    *put.lock().unwrap() = Some(Shown { seq: ask.seq, lines: lines.clone() });
                    if lines.len() >= ask.max {
                        break;
                    }
                }
                *put.lock().unwrap() = Some(Shown { seq: ask.seq, lines });
                if cache.len() > 2000 {
                    cache.clear();
                }
            }
        });
        Worker { tx, out }
    }

    pub fn ask(&self, a: Ask) {
        let _ = self.tx.send(a);
    }
}
