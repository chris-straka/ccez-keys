//! Reading a line or a word aloud with the Mac's own voices: the best one
//! installed for the language (Premium, then Enhanced, then any), one at a
//! time (a new one stops the last).
//!
//! Spoken in the app by one AVSpeechSynthesizer kept for its life, so a voice
//! loads once: a `say` process per click loaded it every time, nearly a
//! second before a word was heard (owner, 2026-11-13: "takes a while for the
//! TTS to kick in").

use objc2::rc::Retained;
use objc2_avf_audio::{AVSpeechBoundary, AVSpeechSynthesisVoice, AVSpeechSynthesizer, AVSpeechUtterance};
use objc2_foundation::NSString;
use std::cell::RefCell;
use std::collections::HashMap;

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

fn utterance(sp: &mut Speaker, lang: &str, text: &str) -> Retained<AVSpeechUtterance> {
    let voices = &sp.voices;
    let voice = sp
        .chosen
        .entry(lang.to_string())
        .or_insert_with(|| voice_for(lang, voices).and_then(|id| unsafe { AVSpeechSynthesisVoice::voiceWithIdentifier(&NSString::from_str(&id)) }))
        .clone();
    unsafe {
        let u = AVSpeechUtterance::speechUtteranceWithString(&NSString::from_str(text));
        u.setVoice(voice.as_deref());
        u
    }
}

/// Says `text` in `lang`, stopping whatever was being said.
pub fn say(lang: &str, text: &str) {
    say_at(lang, text, 1.0);
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
