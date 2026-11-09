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
}

/// What came back: each language's translation, in the order asked.
#[derive(Clone)]
pub struct Shown {
    pub seq: u64,
    pub lines: Vec<(String, String)>,
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

    fn translate(&mut self, from: &str, to: &str, text: &str) -> Result<String, String> {
        self.id += 1;
        let ask = serde_json::json!({"id": self.id, "from": from, "to": to, "text": text});
        writeln!(self.stdin, "{ask}").map_err(|e| e.to_string())?;
        self.stdin.flush().map_err(|e| e.to_string())?;
        let mut line = String::new();
        self.stdout.read_line(&mut line).map_err(|e| e.to_string())?;
        let v: serde_json::Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        match v.get("text").and_then(|t| t.as_str()) {
            Some(t) => Ok(t.to_string()),
            None => Err(v.get("error").and_then(|e| e.as_str()).unwrap_or("no answer").to_string()),
        }
    }
}

impl Worker {
    pub fn start(log: impl Fn(&str, &str) + Send + 'static) -> Worker {
        let (tx, rx): (Sender<Ask>, Receiver<Ask>) = channel();
        let out: Arc<Mutex<Option<Shown>>> = Arc::default();
        let put = out.clone();
        std::thread::spawn(move || {
            let mut helper = Helper::start();
            if helper.is_none() {
                log("error", "translator helper didn't start");
            }
            let mut cache: HashMap<(String, String), String> = HashMap::new();
            let mut missing: Vec<String> = Vec::new();
            while let Ok(mut ask) = rx.recv() {
                // Only the newest: typing moved on past the others.
                while let Ok(newer) = rx.try_recv() {
                    ask = newer;
                }
                let mut lines = Vec::new();
                for lang in &ask.langs {
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
                    if let Some(t) = got.filter(|t| !t.trim().eq_ignore_ascii_case(ask.text.trim())) {
                        lines.push((lang.clone(), t));
                    }
                    // Enough for one line: the rest wouldn't show.
                    if lines.iter().map(|(_, t)| t.chars().count() + 4).sum::<usize>() > 90 {
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
