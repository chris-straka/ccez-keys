//! Reading a line or a word aloud with the Mac's own voices: the best one
//! installed for the language (Premium, then Enhanced, then any), one at a
//! time (a new one stops the last).
//!
//! A word on screen has its sound made when it comes up (`prepare`, a `say -o`
//! in the background, kept in ~/Library/Caches), so a click only plays a
//! file: heard at once (owner, 2026-11-13: "it should be almost
//! instantaneous"). Anything not made ahead is spoken by one
//! AVSpeechSynthesizer kept for the app's life, and made for next time.
//! A `say` per click took about 0.9 s before a word was heard.

use objc2::rc::Retained;
use objc2_app_kit::NSSound;
use objc2_avf_audio::{AVSpeechBoundary, AVSpeechSynthesisVoice, AVSpeechSynthesizer, AVSpeechUtterance};
use objc2_foundation::NSString;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

/// Off from the menu: nothing said, nothing made.
static ON: AtomicBool = AtomicBool::new(true);

pub fn set_on(on: bool) {
    ON.store(on, Ordering::Relaxed);
    if !on {
        stop();
    }
}

/// (identifier, locale as fr_FR, quality 1 default / 2 enhanced / 3 premium).
type Voice = (String, String, isize);

fn installed() -> Vec<Voice> {
    unsafe {
        AVSpeechSynthesisVoice::speechVoices()
            .iter()
            .map(|v| (v.identifier().to_string(), v.language().to_string().replace('-', "_"), v.quality().0))
            .collect()
    }
}

/// The voice for a language code (fr, zh, zh-Hans, pt-BR…).
pub fn voice_for(lang: &str, voices: &[Voice]) -> Option<String> {
    let lang = lang.replace('-', "_");
    let base = lang.split('_').next().unwrap_or(&lang).to_lowercase();
    // Chinese: Mandarin (China) before Cantonese (Hong Kong).
    let region = match lang.as_str() {
        "zh" | "zh_Hans" => Some("zh_CN"),
        "zh_Hant" => Some("zh_TW"),
        _ if lang.contains('_') => Some(lang.as_str()),
        _ => None,
    };
    let rank = |(_, locale, quality): &Voice| {
        let place = if region.is_some_and(|r| r.eq_ignore_ascii_case(locale)) { 0 } else { 1 };
        (place, -quality)
    };
    voices
        .iter()
        .filter(|(_, l, _)| l.split('_').next().is_some_and(|b| b.eq_ignore_ascii_case(&base)))
        .min_by_key(|v| rank(v))
        .map(|(id, _, _)| id.clone())
}

struct Speaker {
    synth: Retained<AVSpeechSynthesizer>,
    voices: Vec<Voice>,
    /// Voice per language, looked up once.
    chosen: HashMap<String, Option<Retained<AVSpeechSynthesisVoice>>>,
}

thread_local! {
    static SPEAKER: RefCell<Option<Speaker>> = const { RefCell::new(None) };
}

fn with_speaker<R>(f: impl FnOnce(&mut Speaker) -> R) -> R {
    SPEAKER.with(|s| {
        let mut s = s.borrow_mut();
        let sp = s.get_or_insert_with(|| Speaker { synth: unsafe { AVSpeechSynthesizer::new() }, voices: installed(), chosen: HashMap::new() });
        f(sp)
    })
}

fn voice(sp: &mut Speaker, lang: &str) -> Option<Retained<AVSpeechSynthesisVoice>> {
    let voices = &sp.voices;
    sp.chosen
        .entry(lang.to_string())
        .or_insert_with(|| voice_for(lang, voices).and_then(|id| unsafe { AVSpeechSynthesisVoice::voiceWithIdentifier(&NSString::from_str(&id)) }))
        .clone()
}

/// The voice as `say -v` names it ("Audrey (Premium)").
fn say_name(v: &AVSpeechSynthesisVoice) -> String {
    let (name, quality) = unsafe { (v.name().to_string(), v.quality().0) };
    match quality {
        _ if name.contains('(') => name,
        3 => format!("{name} (Premium)"),
        2 => format!("{name} (Enhanced)"),
        _ => name,
    }
}

fn cache_dir() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("Library/Caches/uk.ccez.keys/speech")
}

/// Where a word's sound in a voice is kept.
fn file_for(voice: &str, text: &str) -> PathBuf {
    use sha2::Digest;
    let h = sha2::Sha256::digest(format!("{voice}\0{text}").as_bytes());
    let name: String = h.iter().take(12).map(|b| format!("{b:02x}")).collect();
    cache_dir().join(format!("{name}.aiff"))
}

/// The file `text` would be kept in for `lang`'s voice.
fn sound_path(lang: &str, text: &str) -> Option<(String, PathBuf)> {
    let name = with_speaker(|sp| voice(sp, lang).map(|v| say_name(&v)))?;
    let path = file_for(&name, text);
    Some((name, path))
}

static MAKING: Mutex<Option<HashSet<PathBuf>>> = Mutex::new(None);
/// One `say` at a time, so a screenful of new words doesn't take every core.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// Makes `text`'s sound in the background, if it isn't kept already. Run
/// on the main thread (the voice is looked up there).
pub fn prepare(lang: &str, text: &str) {
    if !ON.load(Ordering::Relaxed) || text.trim().is_empty() {
        return;
    }
    let Some((name, path)) = sound_path(lang, text) else { return };
    if path.exists() || !MAKING.lock().unwrap().get_or_insert_with(HashSet::new).insert(path.clone()) {
        return;
    }
    static PRUNED: std::sync::Once = std::sync::Once::new();
    let text = text.to_string();
    std::thread::spawn(move || {
        PRUNED.call_once(prune);
        {
            let _one = ONE_AT_A_TIME.lock().unwrap();
            let _ = std::fs::create_dir_all(cache_dir());
            let part = path.with_extension("part.aiff");
            let made = std::process::Command::new("say").args(["-v", &name, "-o"]).arg(&part).arg("--").arg(&text).status().is_ok_and(|s| s.success());
            if made {
                let _ = std::fs::rename(&part, &path);
            } else {
                let _ = std::fs::remove_file(&part);
            }
        }
        if let Some(m) = MAKING.lock().unwrap().as_mut() {
            m.remove(&path);
        }
    });
}

/// Sounds not used in two weeks go.
fn prune() {
    let Ok(dir) = std::fs::read_dir(cache_dir()) else { return };
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(14 * 86_400);
    for e in dir.flatten() {
        if e.metadata().and_then(|m| m.accessed().or(m.modified())).is_ok_and(|t| t < old) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

thread_local! {
    static PLAYING: RefCell<Option<Retained<NSSound>>> = const { RefCell::new(None) };
}

fn stop() {
    if let Some(s) = PLAYING.with(|p| p.borrow_mut().take()) {
        s.stop();
    }
    SPEAKER.with(|s| {
        if let Some(sp) = s.borrow().as_ref() {
            unsafe { sp.synth.stopSpeakingAtBoundary(AVSpeechBoundary::Immediate) };
        }
    });
}

fn utterance(sp: &mut Speaker, lang: &str, text: &str) -> Retained<AVSpeechUtterance> {
    let voice = voice(sp, lang);
    unsafe {
        let u = AVSpeechUtterance::speechUtteranceWithString(&NSString::from_str(text));
        u.setVoice(voice.as_deref());
        u
    }
}

/// Says `text` in `lang`, stopping whatever was being said: its kept
/// sound if made, else spoken now and made for next time.
pub fn say(lang: &str, text: &str) {
    if !ON.load(Ordering::Relaxed) {
        return;
    }
    stop();
    if let Some(sound) = kept(lang, text) {
        sound.play();
        PLAYING.with(|p| *p.borrow_mut() = Some(sound));
        return;
    }
    // Not kept: only screen words (learning words) have their sound made
    // ahead. A preview line or word is a translation of what was typed, and
    // was saved as audio for two weeks (review, 2026-11-13).
    say_at(lang, text, 1.0);
}

fn kept(lang: &str, text: &str) -> Option<Retained<NSSound>> {
    use objc2::AllocAnyThread;
    let (_, path) = sound_path(lang, text).filter(|(_, p)| p.exists())?;
    NSSound::initWithContentsOfFile_byReference(NSSound::alloc(), &NSString::from_str(&path.to_string_lossy()), true)
}

/// For checks: makes `text`'s sound, waits for it, and says where it is.
pub fn made(lang: &str, text: &str) -> Option<PathBuf> {
    prepare(lang, text);
    let (_, path) = sound_path(lang, text)?;
    let t = std::time::Instant::now();
    while !path.exists() && t.elapsed().as_secs() < 20 {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    path.exists().then_some(path)
}

fn say_at(lang: &str, text: &str, volume: f32) {
    with_speaker(|sp| {
        let u = utterance(sp, lang, text);
        unsafe {
            u.setVolume(volume);
            sp.synth.stopSpeakingAtBoundary(AVSpeechBoundary::Immediate);
            sp.synth.speakUtterance(&u);
        }
    });
}

/// For checks: how long a word takes to start, spoken at volume 0
/// (tests never play sound on the Mac). Spins the run loop meanwhile.
pub fn time_to_start(lang: &str, text: &str) -> std::time::Duration {
    let t = std::time::Instant::now();
    say_at(lang, text, 0.0);
    let speaking = || with_speaker(|sp| unsafe { sp.synth.isSpeaking() });
    while !speaking() && t.elapsed().as_secs() < 5 {
        objc2_foundation::NSRunLoop::currentRunLoop().runUntilDate(&objc2_foundation::NSDate::dateWithTimeIntervalSinceNow(0.005));
    }
    let took = t.elapsed();
    while speaking() && t.elapsed().as_secs() < 10 {
        objc2_foundation::NSRunLoop::currentRunLoop().runUntilDate(&objc2_foundation::NSDate::dateWithTimeIntervalSinceNow(0.01));
    }
    took
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list() -> Vec<Voice> {
        [("thomas", "fr_FR", 1), ("audrey", "fr_FR", 3), ("amelie", "fr_CA", 2), ("fung", "zh_HK", 3), ("tingting", "zh_CN", 1), ("han", "zh_CN", 3), ("kyoko", "ja_JP", 2)]
            .iter()
            .map(|(a, b, c)| (a.to_string(), b.to_string(), *c))
            .collect()
    }

    #[test]
    fn the_best_voice_for_each_language() {
        let v = list();
        assert_eq!(voice_for("fr", &v).as_deref(), Some("audrey"));
        assert_eq!(voice_for("fr-CA", &v).as_deref(), Some("amelie"));
        assert_eq!(voice_for("zh", &v).as_deref(), Some("han"));
        assert_eq!(voice_for("ja", &v).as_deref(), Some("kyoko"));
        assert_eq!(voice_for("ko", &v), None);
    }
}
