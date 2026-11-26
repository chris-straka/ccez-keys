//! The learning strip's previews (the sentence typed, shown in the owner's
//! learning languages) follow `learning.strip_hidden`: the languages left
//! out of them. Its default (none) keeps today's previews; typing
//! languages and the learning order stay as they are.
//! gates/preview-languages/goal.md; protected: the model may not edit it.

use ccez_keys_core::Engine;
use std::sync::Arc;

fn new_dir() -> String {
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("ccez-keys-preview-{}-{n}", std::process::id()));
    dir.to_string_lossy().into()
}

fn engine() -> Arc<Engine> {
    Engine::new(new_dir())
}

fn langs(l: &[&str]) -> Vec<String> {
    l.iter().map(|s| s.to_string()).collect()
}

fn set(e: &Engine, key: &str, json: &str) {
    e.set_setting(key.into(), json.into()).unwrap_or_else(|err| panic!("set {key} {json}: {err:?}"));
}

/// The strip's rule before this setting (Strip.kt's targets() at main, less
/// ML Kit's own list of languages): typing a learning language that isn't
/// the default, the default; else every learning language but the one
/// typed, in the owner's order.
fn today(e: &Engine, typing: &str) -> Vec<String> {
    let def = e.setting("languages.default".into()).unwrap().trim_matches('"').to_string();
    let learning = e.setting_strings("languages.learning".into());
    if typing != def && learning.iter().any(|l| l == typing) {
        vec![def]
    } else {
        learning.into_iter().filter(|l| l != typing).collect()
    }
}

/// The owner's ccez-tube order, the default learning list (core/src/settings.rs at main).
const DEFAULT_LEARNING: &[&str] = &["fr", "de", "es", "zh", "ja", "ko", "pt", "ar", "hi", "ru", "pl", "id", "tr", "it", "no", "cs", "el", "fa", "th", "vi", "ro", "bg", "hu", "hy"];

#[test]
fn by_default_every_learning_language_previews_as_today() {
    let e = engine();
    assert_eq!(e.setting_strings("learning.strip_hidden".into()), Vec::<String>::new());
    // Typing English (the default language): all 24, in the owner's order.
    assert_eq!(e.strip_languages("en".into()), langs(DEFAULT_LEARNING));
    for typing in ["en", "fr", "de", "ja", "zh", "hy", "nl", "sv"] {
        assert_eq!(e.strip_languages(typing.into()), today(&e, typing), "typing {typing}");
    }
}

#[test]
fn by_default_the_owners_own_lists_preview_as_today() {
    let e = engine();
    set(&e, "languages.learning", r#"["fr","de","es","ja","zh","ar"]"#);
    for typing in ["en", "fr", "es", "ja", "ar", "it"] {
        assert_eq!(e.strip_languages(typing.into()), today(&e, typing), "typing {typing}");
    }
    set(&e, "languages.default", r#""fr""#);
    set(&e, "languages.learning", r#"["de","es","ja"]"#);
    for typing in ["fr", "de", "en", "ja"] {
        assert_eq!(e.strip_languages(typing.into()), today(&e, typing), "default fr, typing {typing}");
    }
    assert_eq!(e.strip_languages("fr".into()), langs(&["de", "es", "ja"]));
    assert_eq!(e.strip_languages("de".into()), langs(&["fr"]));
}

#[test]
fn hidden_languages_leave_the_previews_and_the_order_stays() {
    let e = engine();
    set(&e, "languages.learning", r#"["fr","de","es","ja","zh"]"#);
    set(&e, "learning.strip_hidden", r#"["de","ja"]"#);
    assert_eq!(e.strip_languages("en".into()), langs(&["fr", "es", "zh"]));
    // The hidden list's own order means nothing.
    set(&e, "learning.strip_hidden", r#"["zh","fr"]"#);
    assert_eq!(e.strip_languages("en".into()), langs(&["de", "es", "ja"]));
    // Moving a language in the learning order moves it in the previews.
    set(&e, "languages.learning", r#"["ja","fr","de","es","zh"]"#);
    assert_eq!(e.strip_languages("en".into()), langs(&["ja", "de", "es"]));
    // A code that isn't a learning language changes nothing.
    set(&e, "learning.strip_hidden", r#"["ko"]"#);
    assert_eq!(e.strip_languages("en".into()), langs(&["ja", "fr", "de", "es", "zh"]));
    // Shown again: back in its place.
    set(&e, "learning.strip_hidden", "[]");
    assert_eq!(e.strip_languages("en".into()), langs(&["ja", "fr", "de", "es", "zh"]));
}

#[test]
fn the_owners_case_a_few_of_many() {
    let e = engine();
    // Keep French, Japanese and Chinese; hide the other 21.
    let keep = ["fr", "ja", "zh"];
    let hide: Vec<&str> = DEFAULT_LEARNING.iter().copied().filter(|l| !keep.contains(l)).collect();
    set(&e, "learning.strip_hidden", &serde_json::to_string(&hide).unwrap());
    assert_eq!(e.strip_languages("en".into()), langs(&["fr", "zh", "ja"]));
    assert_eq!(e.strip_languages("nl".into()), langs(&["fr", "zh", "ja"]));
}

#[test]
fn typing_a_learning_language_still_previews_in_the_default() {
    let e = engine();
    set(&e, "languages.learning", r#"["fr","de","es"]"#);
    set(&e, "learning.strip_hidden", r#"["fr","de"]"#);
    assert_eq!(e.strip_languages("fr".into()), langs(&["en"]));
    assert_eq!(e.strip_languages("de".into()), langs(&["en"]));
    assert_eq!(e.strip_languages("es".into()), langs(&["en"]));
    assert_eq!(e.strip_languages("en".into()), langs(&["es"]));
}

#[test]
fn hiding_every_language_leaves_no_previews() {
    let e = engine();
    set(&e, "languages.learning", r#"["fr","de"]"#);
    set(&e, "learning.strip_hidden", r#"["fr","de"]"#);
    assert_eq!(e.strip_languages("en".into()), Vec::<String>::new());
    assert_eq!(e.strip_languages("fr".into()), langs(&["en"]));
}

#[test]
fn typing_languages_and_the_learning_list_stay_as_they_are() {
    let e = engine();
    set(&e, "languages.learning", r#"["fr","de","es","ja","zh"]"#);
    let typing = e.typing_languages();
    let current = e.current_language();
    set(&e, "learning.strip_hidden", r#"["de","ja","zh"]"#);
    assert_eq!(e.typing_languages(), typing);
    assert_eq!(e.current_language(), current);
    assert_eq!(e.setting_strings("languages.learning".into()), langs(&["fr", "de", "es", "ja", "zh"]));
    assert!(e.typing_languages().iter().any(|l| l == "de"), "a hidden language is still typed in");
}

#[test]
fn the_setting_survives_a_restart() {
    let dir = new_dir();
    {
        let e = Engine::new(dir.clone());
        set(&e, "languages.learning", r#"["fr","de","es","ja"]"#);
        set(&e, "learning.strip_hidden", r#"["de","ja"]"#);
        e.save();
    }
    let e = Engine::new(dir.clone());
    assert_eq!(e.setting_strings("learning.strip_hidden".into()), langs(&["de", "ja"]));
    assert_eq!(e.strip_languages("en".into()), langs(&["fr", "es"]));
    // And the settings file holds it under its name.
    let saved = std::fs::read_to_string(std::path::Path::new(&dir).join("settings.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&saved).unwrap();
    assert_eq!(v["learning.strip_hidden"], serde_json::json!(["de", "ja"]));
}

#[test]
fn an_older_settings_file_without_it_previews_every_language() {
    let dir = new_dir();
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(std::path::Path::new(&dir).join("settings.json"), r#"{"languages.learning": ["fr","ja","zh"], "learning.strip": true}"#).unwrap();
    let e = Engine::new(dir);
    assert_eq!(e.setting_strings("learning.strip_hidden".into()), Vec::<String>::new());
    assert_eq!(e.strip_languages("en".into()), langs(&["fr", "ja", "zh"]));
}

#[test]
fn it_is_in_the_schema_takes_language_codes_only() {
    let e = engine();
    let schema: serde_json::Value = serde_json::from_str(&e.settings_schema()).unwrap();
    let def = schema.as_array().unwrap().iter().find(|d| d["key"] == "learning.strip_hidden").expect("learning.strip_hidden in keys schema");
    assert_eq!(def["kind"], serde_json::json!("languages"));
    assert_eq!(def["default"], serde_json::json!([]));
    assert!(def["about"].as_str().is_some_and(|a| !a.is_empty()));
    assert!(e.set_setting("learning.strip_hidden".into(), r#"["French"]"#.into()).is_err());
    assert!(e.set_setting("learning.strip_hidden".into(), "true".into()).is_err());
    assert!(e.set_setting("learning.strip_hidden".into(), "3".into()).is_err());
    assert_eq!(e.setting_strings("learning.strip_hidden".into()), Vec::<String>::new());
}
