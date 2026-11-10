//! ccez-keys on Linux and Windows: the same preview as on the Mac (preview.rs
//! decides when it shows and what it says), drawn with egui, translated by
//! Mozilla's Firefox Translations models (firefox.rs), the field read through
//! each system's accessibility API (AT-SPI on Linux, UI Automation on Windows).

mod app;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
use linux as sys;
#[cfg(windows)]
use windows as sys;

use crate::model::Field;

/// What the system tells about the field being typed in, and does to it.
pub trait Access {
    /// The field with the keyboard now (None: not a text field).
    fn focused(&mut self) -> Option<Field>;
    /// The app in front: an id (for the off list) and a name (for the menu).
    fn front_app(&mut self) -> Option<(String, String)>;
    /// `sentence` (just before the cursor, then `trailing` spaces) replaced by
    /// `text`, only if the field still ends that way; how, or why not.
    fn swap(&mut self, sentence: &str, trailing: &str, text: &str) -> Result<&'static str, String>;
}

/// Says `text` in `lang` with the system's voices (stopping what was being said).
pub fn say(lang: &str, text: &str) {
    sys::say(lang, text);
}

/// Puts `text` on the clipboard.
pub fn copy(text: &str) {
    if let Ok(mut c) = arboard::Clipboard::new() {
        let _ = c.set_text(text.to_string());
    }
}

pub fn log(level: &str, what: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(crate::paths::data().join("desktop.log")) {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let _ = writeln!(f, "{t} {level} {what}");
    }
}

pub fn run() {
    let args: Vec<String> = std::env::args().collect();
    // `--translate <lang> "text"`: the translator alone, for checks.
    if let Some(i) = args.iter().position(|a| a == "--translate") {
        use crate::worker::Translator;
        let lang = args.get(i + 1).cloned().unwrap_or_else(|| "fr".into());
        let mut f = crate::firefox::Firefox::new(&crate::paths::data().join("models"));
        for text in &args[i + 2..] {
            let t = std::time::Instant::now();
            match f.translate("en", &lang, text) {
                Ok((out, _)) => println!("{:>5} ms  {out}", t.elapsed().as_millis()),
                Err(e) => println!("error: {e}"),
            }
        }
        return;
    }
    // `--probe`: what the field with the keyboard reports, every second for 20 s.
    if args.iter().any(|a| a == "--probe") {
        let Some(mut a) = sys::Access::start() else {
            println!("no accessibility");
            return;
        };
        for _ in 0..20 {
            let f = a.focused();
            println!("{:?} {:?}", a.front_app(), f.map(|f| (f.secure, f.before.chars().count(), f.caret, f.frame)));
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
        return;
    }
    app::run();
}
