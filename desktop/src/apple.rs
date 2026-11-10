//! Apple's on-device translator, through the bundled Swift helper
//! (ccez-keys-translate, JSON lines over stdin/stdout), for the worker.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use crate::worker::{Translated, Translator};

pub struct Helper {
    _child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    id: u64,
}

impl Helper {
    pub fn start() -> Option<Helper> {
        let exe = std::env::current_exe().ok()?;
        let path = exe.with_file_name("ccez-keys-translate");
        let mut child = Command::new(path).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
        let stdin = child.stdin.take()?;
        let stdout = BufReader::new(child.stdout.take()?);
        Some(Helper { _child: child, stdin, stdout, id: 0 })
    }

}

impl Translator for Helper {
    fn translate(&mut self, from: &str, to: &str, text: &str) -> Result<Translated, String> {
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
            .map(|a| {
                a.iter()
                    .filter_map(|p| {
                        let whole = p.get(2).and_then(|w| w.as_str()).unwrap_or("").to_string();
                        Some((p.get(0)?.as_str()?.to_string(), p.get(1)?.as_str()?.to_string(), whole))
                    })
                    .collect()
            })
            .unwrap_or_default();
        match v.get("text").and_then(|t| t.as_str()) {
            Some(t) => Ok((t.to_string(), ruby)),
            None => Err(v.get("error").and_then(|e| e.as_str()).unwrap_or("no answer").to_string()),
        }
    }
}

