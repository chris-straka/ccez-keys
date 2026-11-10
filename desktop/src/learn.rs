//! Your words on the Mac: statuses synced with ccez-words (the same server as
//! the phone and the other apps), and the dictionaries the phone uses (from
//! ccez-lang-core's release, checked against its SHA256SUMS) so a form counts
//! as its word ("aime" is "aimer").

use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use ccez_keys_core::Engine;

const BASE: &str = "https://github.com/chris-straka/ccez-lang-core/releases/download/dictionaries/";
/// Where a fresh install finds the ccez-words server: one line in this file
/// (kept out of the repo), or `keys set words.server` as on the phone.
const SERVER_FILE: &str = ".config/ccez-keys/words-server";

fn files(lang: &str) -> Vec<&'static str> {
    match lang {
        "zh" => vec!["cedict.txt.gz"],
        "ja" => vec!["ja.tsv.gz", "ja-pitch.tsv.gz", "ipadic.zip"],
        _ => vec![],
    }
}

pub fn fetch(url: &str, to: &Path) -> bool {
    Command::new("curl").args(["-sSfL", "--max-time", "300", "-o"]).arg(to).arg(url).status().is_ok_and(|s| s.success())
}

pub fn sha256(path: &Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(path).ok()?;
    Some(Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect())
}

/// A zip's files, flat (no folders), into `into`.
fn unzip_flat(file: &Path, into: &Path) -> Result<(), String> {
    let mut z = zip::ZipArchive::new(std::fs::File::open(file).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    for i in 0..z.len() {
        let mut f = z.by_index(i).map_err(|e| e.to_string())?;
        if f.is_dir() {
            continue;
        }
        // Only the file's own name: nothing lands outside `into`.
        let Some(name) = f.enclosed_name().and_then(|p| p.file_name().map(|n| n.to_owned())) else { continue };
        let mut out = std::fs::File::create(into.join(name)).map_err(|e| e.to_string())?;
        std::io::copy(&mut f, &mut out).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Fetches and installs `lang`'s dictionary unless it's there.
fn ensure(engine: &Engine, lang: &str, sums: &[(String, String)], tmp: &Path, log: &dyn Fn(&str, &str)) {
    if engine.dictionary_ready(lang.to_string()) {
        return;
    }
    let names: Vec<String> = {
        let f = files(lang);
        if f.is_empty() { vec![format!("{lang}.tsv.gz")] } else { f.into_iter().map(str::to_string).collect() }
    };
    for name in names {
        let Some((_, want)) = sums.iter().find(|(n, _)| *n == name) else { continue };
        let file = tmp.join(&name);
        if !fetch(&format!("{BASE}{name}"), &file) || sha256(&file).as_deref() != Some(want.as_str()) {
            log("warn", &format!("dictionary {name}: download or checksum failed"));
            let _ = std::fs::remove_file(&file);
            return;
        }
        let p = file.to_string_lossy().into_owned();
        let r = if name == "ipadic.zip" {
            let into = Path::new(&engine.dictionary_dir()).join("ipadic");
            let _ = std::fs::create_dir_all(&into);
            unzip_flat(&file, &into)
        } else if name == "ja-pitch.tsv.gz" {
            engine.install_pitch(p).map_err(|e| e.to_string())
        } else {
            engine.install_dictionary(lang.to_string(), p).map_err(|e| e.to_string())
        };
        let _ = std::fs::remove_file(&file);
        match r {
            Ok(()) => log("info", &format!("dictionary {lang}: {name}")),
            Err(e) => log("warn", &format!("dictionary {lang}: {e}")),
        }
    }
}

/// In the background: the dictionaries once, then the word statuses every
/// five minutes (and at once).
pub fn start(engine: Arc<Engine>, dir: std::path::PathBuf, log: impl Fn(&str, &str) + Send + 'static) {
    if engine.setting("words.server".into()).map(|v| v.trim_matches('"').is_empty()).unwrap_or(true) {
        let file = crate::paths::home().join(SERVER_FILE);
        if let Some(url) = std::fs::read_to_string(file).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
            let _ = engine.set_setting("words.server".into(), serde_json::Value::String(url).to_string());
        }
    }
    std::thread::spawn(move || {
        let tmp = dir.join("dict-tmp");
        let _ = std::fs::create_dir_all(&tmp);
        let sums_file = tmp.join("SHA256SUMS");
        let sums: Vec<(String, String)> = if fetch(&format!("{BASE}SHA256SUMS"), &sums_file) {
            std::fs::read_to_string(&sums_file)
                .unwrap_or_default()
                .lines()
                .filter_map(|l| {
                    let mut it = l.split_whitespace();
                    let sum = it.next()?.to_string();
                    let name = it.next()?.trim_start_matches('*').to_string();
                    Some((name, sum))
                })
                .collect()
        } else {
            vec![]
        };
        for lang in engine.setting_strings("languages.learning".into()) {
            ensure(&engine, &lang, &sums, &tmp, &log);
        }
        loop {
            match engine.sync_words("ccez-keys-mac".into()) {
                Ok(n) => log("info", &format!("words synced ({n} came)")),
                Err(e) => log("warn", &format!("words sync: {e}")),
            }
            std::thread::sleep(Duration::from_secs(300));
        }
    });
}
