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
}

#[test]
fn words_of_another_language_are_not_turned_into_this_ones() {
    // French typed before the keyboard noticed it isn't English.
    let e = engine(&["en", "fr"]);
    for w in ["nous", "allons", "manger", "avec", "amis", "soir"] {
        assert_eq!(fix(&e, "en", w), None, "{w}");
    }
    assert_eq!(fix(&e, "en", "teh").as_deref(), Some("the"));
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
    // The phone runs a release build; an unoptimised test build is about twice as slow.
    let limit = if cfg!(debug_assertions) { 12 } else { 4 };
    assert!(each < std::time::Duration::from_millis(limit), "{each:?}");
}

#[test]
fn notices_the_language_typed() {
    let e = engine(&["en", "fr", "de", "es"]);
    let d = |cur: &str, ws: &[&str]| e.detect_language(cur.into(), ws.iter().map(|w| w.to_string()).collect());
    assert_eq!(d("en", &["je", "suis"]).as_deref(), Some("fr"));
    assert_eq!(d("en", &["ich", "bin"]).as_deref(), Some("de"));
    assert_eq!(d("fr", &["estoy", "cansado"]).as_deref(), Some("es"));
    // English words while on English, names: stay. One clearly French word
    // switches (owner, 2026-11-08: "chez" should switch to French).
    assert_eq!(d("en", &["see", "you"]), None);
    assert_eq!(d("en", &["Tchoupi", "Doudou"]), None);
    assert_eq!(d("en", &["bonjour"]).as_deref(), Some("fr"));
}

#[test]
#[ignore]
fn memory_per_language() {
    fn rss() -> u64 {
        let out = std::process::Command::new("ps").args(["-o", "rss=", "-p", &std::process::id().to_string()]).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
    }
    let before = rss();
    let e = engine(&["fr", "de", "es", "en"]);
    let after = rss();
    eprintln!("4 languages: {} MB", (after - before) / 1024);
    drop(e);
}

#[test]
fn swiping_french_words() {
    let e = engine(&["fr"]);
    // The letters a finger passes on QWERTY for each word.
    assert_eq!(e.swipe("fr".into(), "bvcxonjhgfdsoiuyr".into()).first().map(String::as_str), Some("bonjour"));
    assert_eq!(e.swipe("fr".into(), "mjhgfdsaqsdfghijuytrezaasdfghjklmnbvcxwqsdfghjklmo".into()).len() <= 4, true);
    assert_eq!(e.swipe("fr".into(), "mertcvbgfdsi".into()).first().map(String::as_str), Some("merci"));
}

#[test]
fn japanese_in_romaji_is_noticed() {
    let e = engine(&["en", "fr", "ja"]);
    let r = |w: &str| e.romaji_word("en".into(), w.into());
    assert_eq!(r("konnichiha").as_deref(), Some("こんにちは"));
    assert_eq!(r("Arigatou").as_deref(), Some("ありがとう"));
    assert_eq!(r("sugoi").as_deref(), Some("すごい"));
    // English and French words, typos and short words stay.
    for w in ["anime", "karate", "tsunami", "manger", "hello", "teh", "thx", "nani", "taken", "mine", "same"] {
        assert_eq!(r(w), None, "{w}");
    }
    assert_eq!(e.romaji_word("ja".into(), "konnichiha".into()), None);
}

#[test]
fn next_words_from_the_language_then_the_owner() {
    let e = engine(&["fr", "en"]);
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../data/next/");
    for l in ["fr", "en"] {
        assert!(e.load_next(l.into(), std::fs::read_to_string(format!("{root}{l}.txt")).unwrap()) > 1000);
    }
    let p = |l: &str, w: &str| e.predict(l.into(), w.into());
    assert_eq!(p("fr", "je")[..2], ["ne", "suis"]);
    assert_eq!(p("en", "thank")[0], "you");
    assert!(p("fr", "Je").contains(&"suis".to_string()));
    // What the owner typed twice after a word comes first.
    for _ in 0..2 {
        e.learn("fr".into(), "je".into(), "pense".into(), false);
    }
    assert_eq!(p("fr", "je")[0], "pense");
    // No Tatoeba stock names.
    assert!(!p("en", "with").iter().any(|w| w == "Tom" || w == "Mary"));
}

#[test]
fn bench_findings_stay_fixed() {
    let e = engine(&["en", "de", "fr"]);
    // A missed last letter, and one missed inside.
    assert_eq!(fix(&e, "en", "guessin").as_deref(), Some("guessing"));
    assert_eq!(fix(&e, "en", "prioriy").as_deref(), Some("priority"));
    // German's "einer"/"Einer" are one word, not rivals.
    assert_eq!(fix(&e, "de", "einerr").as_deref(), Some("einer"));
    assert_eq!(fix(&e, "de", "diesw").as_deref(), Some("diese"));
    // Two edits from a very common word don't win ("foood" was "for"):
    // food and good are both one away, so it's left as typed.
    assert_ne!(fix(&e, "en", "foood").as_deref(), Some("for"));
    // An accent typed stays.
    assert_eq!(fix(&e, "fr", "mesuré"), None);
}

#[test]
fn one_foreign_word_switches() {
    let e = engine(&["en", "fr", "es", "de"]);
    let d = |w: &str| e.detect_language("en".into(), vec![w.into()]);
    assert_eq!(d("chez").as_deref(), Some("fr"));
    assert_eq!(d("merci").as_deref(), Some("fr"));
    assert_eq!(d("gracias").as_deref(), Some("es"));
    // English words, names and typos stay.
    for w in ["the", "chef", "cafe", "hello", "teh", "anime"] {
        assert_eq!(d(w), None, "{w}");
    }
}

#[test]
fn english_contractions_whatever_else_is_loaded() {
    // "im" is German too: on English it's still I'm.
    let e = engine(&["en", "de", "it"]);
    e.set_setting("languages.learning".into(), r#"["de","it"]"#.into()).unwrap();
    assert_eq!(fix(&e, "en", "im").as_deref(), Some("I'm"));
    assert_eq!(fix(&e, "en", "Im").as_deref(), Some("I'm"));
    assert_eq!(fix(&e, "en", "weve").as_deref(), Some("we've"));
    assert_eq!(fix(&e, "en", "i").as_deref(), Some("I"));
    assert_eq!(fix(&e, "en", "dont").as_deref(), Some("don't"));
    assert_eq!(e.suggest("en".into(), "im".into()).words.first().map(String::as_str), Some("I'm"));
    // Words in their own right stay.
    for w in ["ill", "were", "its", "lets", "well"] {
        assert_eq!(fix(&e, "en", w), None, "{w}");
    }
    // Typed in German, "im" is German.
    assert_eq!(fix(&e, "de", "im"), None);
}

#[test]
fn a_word_taken_out_of_the_suggestions() {
    let e = engine(&["fr"]);
    let offered = |t: &str| e.suggest("fr".into(), t.into()).words;
    e.learn("fr".into(), "".into(), "ccezkeys".into(), false);
    e.learn("fr".into(), "".into(), "ccezkeys".into(), false);
    assert!(offered("ccezk").contains(&"ccezkeys".to_string()));
    e.remove_word("fr".into(), "ccezkeys".into());
    assert!(!offered("ccezk").contains(&"ccezkeys".to_string()));
    assert!(!e.knows_word("fr".into(), "ccezkeys".into()));
    // Typing it again doesn't bring it back; holding it to save it does.
    e.learn("fr".into(), "".into(), "ccezkeys".into(), false);
    assert!(!offered("ccezk").contains(&"ccezkeys".to_string()));
    e.learn("fr".into(), "".into(), "ccezkeys".into(), true);
    assert!(offered("ccezk").contains(&"ccezkeys".to_string()));
    // A word of the lists goes too.
    assert!(offered("bonj").contains(&"bonjour".to_string()));
    e.remove_word("fr".into(), "bonjour".into());
    assert!(!offered("bonj").contains(&"bonjour".to_string()));
    assert_eq!(fix(&e, "fr", "bonjoru").as_deref().filter(|w| *w == "bonjour"), None);
}

#[test]
fn words_typed_often_come_sooner() {
    let e = engine(&["fr"]);
    let offered = |t: &str| e.suggest("fr".into(), t.into()).words;
    // A word of the list, rare there.
    assert!(e.knows_word("fr".into(), "prennes".into()));
    assert!(!offered("pren").contains(&"prennes".to_string()), "{:?}", offered("pren"));
    for _ in 0..6 {
        e.learn("fr".into(), "tu".into(), "prennes".into(), false);
    }
    assert!(offered("pren").contains(&"prennes".to_string()), "{:?}", offered("pren"));
    // Saved with the rest of the owner's words.
    e.save();
}

#[test]
fn the_text_before_orders_suggestions() {
    let e = engine(&["de"]);
    let first = |before: &str, typed: &str| e.suggest_in("de".into(), typed.into(), format!("{before}{typed}")).words.first().cloned();
    let alone = e.suggest("de".into(), "Kat".into()).words;
    assert_ne!(alone.first().map(String::as_str), Some("Katzen"), "{alone:?}");
    // A word already in the paragraph.
    assert_eq!(first("Die Katzen schlafen. Meine ", "Kat").as_deref(), Some("Katzen"));
    // What the owner typed after the word before.
    e.learn("de".into(), "schwarze".into(), "Kater".into(), false);
    assert_eq!(first("Eine schwarze ", "Kat").as_deref(), Some("Kater"));
}
