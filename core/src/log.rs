//! Events and errors, as JSON lines in the app's files, for agents to
//! read with `keys log`. Never typed text: callers pass counts, lengths,
//! timings and names, not what was written.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_BYTES: u64 = 512 * 1024;

pub struct Log {
    path: PathBuf,
}

impl Log {
    pub fn new(dir: &Path) -> Self {
        Log { path: dir.join("log.jsonl") }
    }

    pub fn write(&self, level: &str, event: &str, ctx: &str) {
        let ctx: serde_json::Value = serde_json::from_str(ctx).unwrap_or(serde_json::Value::Null);
        let t = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
        let line = serde_json::json!({"t": t, "level": level, "event": event, "ctx": ctx}).to_string();
        if fs::metadata(&self.path).map(|m| m.len() > MAX_BYTES).unwrap_or(false) {
            let _ = fs::rename(&self.path, self.path.with_extension("1.jsonl"));
        }
        // One write per line, so lines from several threads never interleave.
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&self.path) {
            let _ = f.write_all(format!("{line}\n").as_bytes());
        }
    }

    /// The last `n` lines, oldest first (the rotated file too when needed).
    pub fn tail(&self, n: usize) -> String {
        let mut lines: Vec<String> = Vec::new();
        for p in [self.path.with_extension("1.jsonl"), self.path.clone()] {
            if let Ok(t) = fs::read_to_string(&p) {
                lines.extend(t.lines().map(String::from));
            }
        }
        let skip = lines.len().saturating_sub(n);
        lines[skip..].join("\n")
    }
}
