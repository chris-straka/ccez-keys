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
    /// Selected text gets a preview too.
    pub selection: bool,
    /// A word clicked on the preview is copied (with a toast).
    pub copy_click: bool,
    /// A word clicked (on screen or in the preview) is said aloud.
    pub speak: bool,
    /// Learning words on screen at once, in its empty spots (0: none).
    pub words_on_screen: usize,
    /// How many of the learning languages (in the owner's order) those words come from (0: all).
    pub words_langs: usize,
    /// Words come while keys are being typed, and while a video plays.
    pub words_typing: bool,
    pub words_video: bool,
    /// Words leave for screenshots (and stay out of captures).
    pub words_hide_shots: bool,
    /// ⌥⏎ in ChatGPT and Claude puts the message box into the first learning language.
    pub chat_swap: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            shown: 6,
            readings: true,
            off: vec![],
            on: vec![],
            paused: false,
            login_set: false,
            selection: true,
            copy_click: true,
            speak: true,
            words_on_screen: 4,
            words_langs: 0,
            words_typing: true,
            words_video: true,
            words_hide_shots: true,
            chat_swap: true,
        }
    }
}

pub fn path(dir: &std::path::Path) -> PathBuf {
    dir.join(if cfg!(target_os = "macos") { "mac.json" } else { "desktop.json" })
}

impl Prefs {
    pub fn load(dir: &std::path::Path) -> Prefs {
        let d = Prefs::default();
        let Ok(t) = std::fs::read_to_string(path(dir)) else {
            return d;
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&t) else {
            return d;
        };
        let list = |k: &str| v[k].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
        Prefs {
            shown: v["shown"].as_u64().map(|n| n as usize).unwrap_or(d.shown),
            readings: v["readings"].as_bool().unwrap_or(d.readings),
            off: list("off"),
            on: list("on"),
            paused: v["paused"].as_bool().unwrap_or(false),
            login_set: v["login_set"].as_bool().unwrap_or(false),
            selection: v["selection"].as_bool().unwrap_or(d.selection),
            copy_click: v["copy_click"].as_bool().unwrap_or(d.copy_click),
            speak: v["speak"].as_bool().unwrap_or(d.speak),
            words_on_screen: v["words_on_screen"].as_u64().map(|n| n as usize).unwrap_or(if v["idle_words"].as_bool() == Some(false) { 0 } else { d.words_on_screen }),
            words_langs: v["words_langs"].as_u64().map(|n| n as usize).unwrap_or(d.words_langs),
            words_typing: v["words_typing"].as_bool().unwrap_or(d.words_typing),
            words_video: v["words_video"].as_bool().unwrap_or(d.words_video),
            words_hide_shots: v["words_hide_shots"].as_bool().unwrap_or(d.words_hide_shots),
            chat_swap: v["chat_swap"].as_bool().unwrap_or(d.chat_swap),
        }
    }

    pub fn save(&self, dir: &std::path::Path) {
        let v = serde_json::json!({"shown": self.shown, "readings": self.readings, "off": self.off, "on": self.on, "paused": self.paused, "login_set": self.login_set, "selection": self.selection, "copy_click": self.copy_click, "speak": self.speak, "words_on_screen": self.words_on_screen, "words_langs": self.words_langs, "words_typing": self.words_typing, "words_video": self.words_video, "words_hide_shots": self.words_hide_shots, "chat_swap": self.chat_swap});
        let tmp = path(dir).with_extension("tmp");
        if std::fs::write(&tmp, v.to_string()).is_ok() {
            let _ = std::fs::rename(&tmp, path(dir));
        }
    }
}
