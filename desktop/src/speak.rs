//! Reading a line or a word aloud with the Mac's own voices: the best one
//! installed for the language (Premium, then Enhanced, then any), one at a
//! time (a new one stops the last).

use std::process::{Child, Command};
use std::sync::{Mutex, OnceLock};

/// `say -v ?` once: (name, locale) for each voice.
fn voices() -> &'static Vec<(String, String)> {
    static V: OnceLock<Vec<(String, String)>> = OnceLock::new();
    V.get_or_init(|| {
        let out = Command::new("say").args(["-v", "?"]).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
        out.lines().filter_map(parse).collect()
    })
}

/// "Audrey (Premium)    fr_FR    # Bonjour…" → ("Audrey (Premium)", "fr_FR").
fn parse(line: &str) -> Option<(String, String)> {
    let head = line.split('#').next()?.trim_end();
    let (name, locale) = head.rsplit_once(char::is_whitespace)?;
    Some((name.trim().to_string(), locale.to_string()))
}

/// The voice for a language code (fr, zh, zh-Hans, pt-BR…).
pub fn voice_for(lang: &str, voices: &[(String, String)]) -> Option<String> {
    let lang = lang.replace('-', "_");
    let base = lang.split('_').next().unwrap_or(&lang).to_lowercase();
    // Chinese: Mandarin (China) before Cantonese (Hong Kong).
    let region = match lang.as_str() {
        "zh" | "zh_Hans" => Some("zh_CN"),
        "zh_Hant" => Some("zh_TW"),
        _ if lang.contains('_') => Some(lang.as_str()),
        _ => None,
    };
    let rank = |(name, locale): &(String, String)| {
        let quality = if name.contains("(Premium)") { 0 } else if name.contains("(Enhanced)") { 1 } else { 2 };
        let place = if region.is_some_and(|r| r.eq_ignore_ascii_case(locale)) { 0 } else { 1 };
        (place, quality)
    };
    voices
        .iter()
        .filter(|(_, l)| l.split('_').next().is_some_and(|b| b.eq_ignore_ascii_case(&base)))
        .min_by_key(|v| rank(v))
        .map(|(n, _)| n.clone())
}

static LAST: Mutex<Option<Child>> = Mutex::new(None);

/// Says `text` in `lang`, stopping whatever was being said.
pub fn say(lang: &str, text: &str) {
    let mut last = LAST.lock().unwrap();
    if let Some(mut c) = last.take() {
        let _ = c.kill();
        let _ = c.wait();
    }
    let mut cmd = Command::new("say");
    if let Some(v) = voice_for(lang, voices()) {
        cmd.args(["-v", &v]);
    }
    *last = cmd.arg("--").arg(text).spawn().ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list() -> Vec<(String, String)> {
        ["Thomas              fr_FR    # Bonjour", "Audrey (Premium)    fr_FR    # Bonjour", "Amélie (Enhanced)   fr_CA    # Bonjour", "Fung (Premium)      zh_HK    # 你好", "Tingting            zh_CN    # 你好", "Han (Premium)       zh_CN    # 你好", "Kyoko (Enhanced)    ja_JP    # こんにちは"]
            .iter()
            .filter_map(|l| parse(l))
            .collect()
    }

    #[test]
    fn the_best_voice_for_each_language() {
        let v = list();
        assert_eq!(voice_for("fr", &v).as_deref(), Some("Audrey (Premium)"));
        assert_eq!(voice_for("fr-CA", &v).as_deref(), Some("Amélie (Enhanced)"));
        assert_eq!(voice_for("zh", &v).as_deref(), Some("Han (Premium)"));
        assert_eq!(voice_for("ja", &v).as_deref(), Some("Kyoko (Enhanced)"));
        assert_eq!(voice_for("ko", &v), None);
    }
}
