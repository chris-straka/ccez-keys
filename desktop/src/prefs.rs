//! The menu's choices, kept in ~/Library/Application Support/ccez-keys/mac.json.

use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Prefs {
    /// How many languages the preview shows (0: all of them).
    pub shown: usize,
    /// Furigana and pinyin over Japanese and Chinese.
    pub readings: bool,
    /// Apps turned off from the menu, beyond the quiet ones by default.
    pub off: Vec<String>,
    /// Quiet-by-default apps turned back on.
    pub on: Vec<String>,
    /// Paused from the menu or settings (Linux and Windows keep it here).
    pub paused: bool,
    /// "Open at login" was set on the first start (on, as decided); after
    /// that it's the owner's, from the menu.
    pub login_set: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs { shown: 6, readings: true, off: vec![], on: vec![], paused: false, login_set: false }
    }
}

pub fn path(dir: &std::path::Path) -> PathBuf {
    dir.join(if cfg!(target_os = "macos") { "mac.json" } else { "desktop.json" })
}

impl Prefs {
    pub fn load(dir: &std::path::Path) -> Prefs {
        let d = Prefs::default();
        let Ok(t) = std::fs::read_to_string(path(dir)) else { return d };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&t) else { return d };
        let list = |k: &str| v[k].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
        Prefs {
            shown: v["shown"].as_u64().map(|n| n as usize).unwrap_or(d.shown),
            readings: v["readings"].as_bool().unwrap_or(d.readings),
            off: list("off"),
            on: list("on"),
            paused: v["paused"].as_bool().unwrap_or(false),
            login_set: v["login_set"].as_bool().unwrap_or(false),
        }
    }

    pub fn save(&self, dir: &std::path::Path) {
        let v = serde_json::json!({"shown": self.shown, "readings": self.readings, "off": self.off, "on": self.on, "paused": self.paused, "login_set": self.login_set});
        let tmp = path(dir).with_extension("tmp");
        if std::fs::write(&tmp, v.to_string()).is_ok() {
            let _ = std::fs::rename(&tmp, path(dir));
        }
    }
}
