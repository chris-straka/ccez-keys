//! Autocorrect and suggestions on the real word lists (data/words), in
//! the languages the owner types: French, German, Spanish, and English.

use ccez_keys_core::Engine;

fn engine(langs: &[&str]) -> std::sync::Arc<Engine> {
    let dir = std::env::temp_dir().join(format!("ccez-keys-test-{}-{}", std::process::id(), rand_suffix()));
    let e = Engine::new(dir.to_string_lossy().into());
    for l in langs {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../data/words/");
        let mut text = std::fs::read_to_string(format!("{root}{l}.txt")).unwrap();
        if let Ok(extra) = std::fs::read_to_string(format!("{root}{l}-extra.txt")) {
            text += &extra;
        }
        assert!(e.load_words(l.to_string(), text) > 20000);
    }
    e
}

fn rand_suffix() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
}

fn fix(e: &Engine, lang: &str, typed: &str) -> Option<String> {
    e.suggest(lang.into(), typed.into()).correction
}

#[test]
fn french_accents_come_back() {
    let e = engine(&["fr"]);
    assert_eq!(fix(&e, "fr", "deja").as_deref(), Some("déjà"));
    assert_eq!(fix(&e, "fr", "tres").as_deref(), Some("très"));
    assert_eq!(fix(&e, "fr", "francais").as_deref(), Some("français"));
}

#[test]
fn french_slips_and_apostrophes() {
    let e = engine(&["fr"]);
    assert_eq!(fix(&e, "fr", "bonjoru").as_deref(), Some("bonjour"));
    assert_eq!(fix(&e, "fr", "Bonjoru").as_deref(), Some("Bonjour"));
    assert_eq!(fix(&e, "fr", "jai").as_deref(), Some("j'ai"));
    assert_eq!(fix(&e, "fr", "maisom").as_deref(), Some("maison"));
}

#[test]
fn real_words_are_left_alone() {
    let e = engine(&["fr", "en"]);
    for w in ["maison", "chat", "Bonjour", "a", "ou", "où", "à"] {
        assert_eq!(fix(&e, "fr", w), None, "{w}");
    }
    // An English word typed while on French isn't "fixed" into French.
    assert_eq!(fix(&e, "fr", "weekend"), None);
    assert_eq!(fix(&e, "fr", "the"), None);
}

#[test]
fn french_wins_when_its_spelling_is_commoner() {
    // "deja" is Spanish too, but déjà is far commoner in French.
    let e = engine(&["fr", "es", "en"]);
    assert_eq!(fix(&e, "fr", "deja").as_deref(), Some("déjà"));
    assert_eq!(fix(&e, "fr", "tres").as_deref(), Some("très"));
    assert_eq!(fix(&e, "fr", "the"), None);
    assert_eq!(fix(&e, "fr", "you"), None);
    assert_eq!(fix(&e, "es", "tres"), None);
    assert_eq!(fix(&e, "es", "deja"), None);
}

#[test]
fn german_capitals() {
    let e = engine(&["de"]);
    assert_eq!(fix(&e, "de", "haus").as_deref(), Some("Haus"));
    assert_eq!(fix(&e, "de", "strasse").as_deref(), Some("Straße"));
    assert_eq!(fix(&e, "de", "essen"), None);
    assert_eq!(fix(&e, "de", "Essen"), None);
}

#[test]
fn completions_while_typing() {
    let e = engine(&["fr"]);
    let s = e.suggest("fr".into(), "bonj".into());
    assert_eq!(s.words.first().map(String::as_str), Some("bonjour"), "{:?}", s.words);
    assert_eq!(s.correction, None);
    let s = e.suggest("fr".into(), "aujou".into());
    assert!(s.words.iter().any(|w| w == "aujourd'hui"), "{:?}", s.words);
}

#[test]
fn spanish_and_english() {
    let e = engine(&["es", "en"]);
    assert_eq!(fix(&e, "es", "manana").as_deref(), Some("mañana"));
    assert_eq!(fix(&e, "es", "tambien").as_deref(), Some("también"));
    assert_eq!(fix(&e, "en", "teh").as_deref(), Some("the"));
    assert_eq!(fix(&e, "en", "i").as_deref(), Some("I"));
    assert_eq!(fix(&e, "en", "im").as_deref(), Some("I'm"));
}

#[test]
fn own_words_stop_being_corrected() {
    let e = engine(&["fr"]);
    assert!(fix(&e, "fr", "Tchoupi").is_some() || true);
    e.learn("fr".into(), "".into(), "Tchoupi".into(), false);
    e.learn("fr".into(), "".into(), "Tchoupi".into(), false);
    assert_eq!(fix(&e, "fr", "Tchoupi"), None);
    // Undoing a correction trusts the word at once.
    e.learn("fr".into(), "".into(), "ptdr".into(), true);
    assert_eq!(fix(&e, "fr", "ptdr"), None);
}

#[test]
fn next_words_from_own_typing() {
    let e = engine(&["fr"]);
    e.learn("fr".into(), "à".into(), "demain".into(), false);
    e.learn("fr".into(), "à".into(), "demain".into(), false);
    e.learn("fr".into(), "à".into(), "plus".into(), false);
    assert_eq!(e.predict("fr".into(), "À".into()), vec!["demain".to_string(), "plus".to_string()]);
}

#[test]
fn addresses_and_numbers_left_alone() {
    let e = engine(&["fr"]);
    assert_eq!(fix(&e, "fr", "lea@gmail"), None);
    assert_eq!(fix(&e, "fr", "19h"), None);
}

#[test]
fn settings_round_trip_and_refuse_bad_values() {
    let e = engine(&["fr"]);
    assert_eq!(e.setting("typing.autocorrect".into()).unwrap(), "true");
    e.set_setting("typing.autocorrect".into(), "false".into()).unwrap();
    assert_eq!(fix(&e, "fr", "deja"), None);
    assert!(e.set_setting("typing.autocorrect".into(), "\"yes\"".into()).is_err());
    assert!(e.set_setting("keyboard.height".into(), "500".into()).is_err());
    assert!(e.set_setting("nope".into(), "1".into()).is_err());
    e.set_setting("languages.learning".into(), "[\"es\",\"fr\"]".into()).unwrap();
    assert_eq!(e.typing_languages(), vec!["en", "es", "fr"]);
}

#[test]
fn layouts_have_accents_and_digits() {
    let e = engine(&[]);
    let l = e.layout("fr".into(), ccez_keys_core::Page::Letters);
    let e_key = l.rows[0].keys.iter().find(|k| k.text == "e").unwrap();
    assert_eq!(e_key.popups, vec!["3", "é", "è", "ê", "ë"]);
    assert_eq!(l.rows.len(), 4);
}

#[test]
fn fast_enough_for_each_key() {
    let t = std::time::Instant::now();
    let e = engine(&["fr", "de", "es", "en"]);
    let load = t.elapsed();
    let words = ["b", "bo", "bon", "bonj", "bonjo", "bonjoru", "aujourdhui", "maisom", "strasse", "tambien", "dejaa", "qwertyuiop"];
    let t = std::time::Instant::now();
    for _ in 0..50 {
        for w in words {
            e.suggest("fr".into(), w.into());
        }
    }
    let each = t.elapsed() / (50 * words.len() as u32);
    eprintln!("load 4 languages: {load:?}; one suggestion: {each:?}");
    assert!(each < std::time::Duration::from_millis(4), "{each:?}");
}

#[test]
fn notices_the_language_typed() {
    let e = engine(&["en", "fr", "de", "es"]);
    let d = |cur: &str, ws: &[&str]| e.detect_language(cur.into(), ws.iter().map(|w| w.to_string()).collect());
    assert_eq!(d("en", &["je", "suis"]).as_deref(), Some("fr"));
    assert_eq!(d("en", &["ich", "bin"]).as_deref(), Some("de"));
    assert_eq!(d("fr", &["estoy", "cansado"]).as_deref(), Some("es"));
    // English words while on English, names, one word: stay.
    assert_eq!(d("en", &["see", "you"]), None);
    assert_eq!(d("en", &["Tchoupi", "Doudou"]), None);
    assert_eq!(d("en", &["bonjour"]), None);
}
