//! Every setting, in one table: the keyboard's settings panel and the
//! `keys` CLI both read it, so a new setting shows up in both.

use serde_json::{Map, Value, json};

pub enum Kind {
    Bool,
    /// One language code.
    Lang,
    Int { min: i64, max: i64 },
    /// Language codes, in order.
    Langs,
    /// Plain strings (app package names).
    List,
    /// One string (a server address).
    Text,
}

pub struct Def {
    pub key: &'static str,
    pub kind: Kind,
    pub default: fn() -> Value,
    /// For people and agents reading `keys schema`.
    pub about: &'static str,
}

pub const CHAT_APPS: &[&str] = &[
    "com.google.android.apps.messaging",
    "com.samsung.android.messaging",
    "com.enflick.android.TextNow",
    "com.discord",
    "org.telegram.messenger",
    "com.tencent.mm",
    "com.whatsapp",
    "com.facebook.orca",
    "com.instagram.android",
    "com.facebook.katana",
    "com.instagram.barcelona",
    "jp.naver.line.android",
    "org.thoughtcrime.securesms",
];

pub const DEFS: &[Def] = &[
    Def { key: "buttons.clipboard", kind: Kind::Bool, default: || json!(true), about: "The clipboard chip in the button row." },
    Def { key: "buttons.face", kind: Kind::Bool, default: || json!(true), about: "The face key (emoji and kaomoji)." },
    Def { key: "typing.autocorrect", kind: Kind::Bool, default: || json!(true), about: "Correct the word on space." },
    Def { key: "typing.capitals", kind: Kind::Bool, default: || json!(true), about: "Capital at the start of a sentence." },
    Def { key: "typing.double_space_period", kind: Kind::Bool, default: || json!(true), about: "Two spaces type a period." },
    Def { key: "typing.vibrate", kind: Kind::Bool, default: || json!(true), about: "Vibrate on each key." },
    Def { key: "typing.sound", kind: Kind::Bool, default: || json!(false), about: "Click on each key." },
    Def { key: "typing.numbers", kind: Kind::Bool, default: || json!(false), about: "A row of digits over the letters." },
    Def { key: "keyboard.height", kind: Kind::Int { min: 80, max: 130 }, default: || json!(100), about: "Key height, percent." },
    Def {
        key: "languages.default",
        kind: Kind::Lang,
        default: || json!("en"),
        about: "The language meanings and translations come in (as ccez-tube's Default language). Typed in too.",
    },
    Def {
        key: "languages.learning",
        kind: Kind::Langs,
        // The owner's order in ccez-tube (2026-11-08).
        default: || json!(["fr", "de", "es", "zh", "ja", "ko", "pt", "ar", "hi", "ru", "pl", "id", "tr", "it", "no", "cs", "el", "fa", "th", "vi", "ro", "bg", "hu", "hy"]),
        about: "Languages being learned, most wanted first: the learning strip's order. Each one with a layout is typed in too.",
    },
    Def { key: "learning.strip", kind: Kind::Bool, default: || json!(true), about: "The learning strip: what you type, in a language you're learning." },
    Def {
        key: "words.server",
        kind: Kind::Text,
        default: || json!(""),
        about: "The ccez-words server (word statuses shared with ccez-tube, ccez-lang, ccez-llm). Empty: statuses stay on the phone.",
    },
    Def { key: "ai.provider", kind: Kind::Text, default: || json!("muse"), about: "muse, deepseek or zen (the key is set with `keys key`, or pasted in Settings)." },
    Def { key: "ai.tips", kind: Kind::Bool, default: || json!(true), about: "Corrections while typing in a language being learned (needs the AI key)." },
    Def { key: "apps.chat", kind: Kind::List, default: || json!(CHAT_APPS), about: "Apps where the face key opens kaomoji first." },
];

pub fn def(key: &str) -> Option<&'static Def> {
    DEFS.iter().find(|d| d.key == key)
}

pub struct Settings {
    pub values: Map<String, Value>,
}

impl Settings {
    pub fn from_json(text: &str) -> Self {
        let saved: Map<String, Value> = serde_json::from_str(text).unwrap_or_default();
        let mut values = Map::new();
        for d in DEFS {
            let v = saved.get(d.key).filter(|v| check(d, v).is_ok()).cloned().unwrap_or_else(d.default);
            values.insert(d.key.into(), v);
        }
        Settings { values }
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.values.get(key)
    }

    pub fn bool(&self, key: &str) -> bool {
        self.get(key).and_then(Value::as_bool).unwrap_or(false)
    }

    pub fn strings(&self, key: &str) -> Vec<String> {
        self.get(key)
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default()
    }

    pub fn set(&mut self, key: &str, value: Value) -> Result<(), String> {
        let d = def(key).ok_or_else(|| format!("no setting called {key}; `keys schema` lists them"))?;
        check(d, &value)?;
        self.values.insert(key.into(), value);
        Ok(())
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(&self.values).unwrap_or_default()
    }
}

fn is_lang(s: &str) -> bool {
    (2..=3).contains(&s.len()) && s.chars().all(|c| c.is_ascii_lowercase())
}

fn check(d: &Def, v: &Value) -> Result<(), String> {
    let ok = match d.kind {
        Kind::Bool => v.is_boolean(),
        Kind::Lang => v.as_str().is_some_and(is_lang),
        Kind::Int { min, max } => v.as_i64().is_some_and(|n| (min..=max).contains(&n)),
        Kind::Langs => v.as_array().is_some_and(|a| a.iter().all(|x| x.as_str().is_some_and(is_lang))),
        Kind::List => v.as_array().is_some_and(|a| a.iter().all(Value::is_string)),
        Kind::Text => v.is_string(),
    };
    if ok {
        Ok(())
    } else {
        Err(match d.kind {
            Kind::Bool => format!("{} takes true or false", d.key),
            Kind::Lang => format!("{} takes a language code, like \"fr\"", d.key),
            Kind::Int { min, max } => format!("{} takes a number from {min} to {max}", d.key),
            Kind::Langs => format!("{} takes a list of language codes, like [\"fr\",\"ja\"]", d.key),
            Kind::List => format!("{} takes a list of strings", d.key),
            Kind::Text => format!("{} takes a string", d.key),
        })
    }
}

pub fn schema() -> String {
    let v: Vec<Value> = DEFS
        .iter()
        .map(|d| {
            let kind = match d.kind {
                Kind::Bool => json!("bool"),
                Kind::Lang => json!("language"),
                Kind::Int { min, max } => json!({"int": [min, max]}),
                Kind::Langs => json!("languages"),
                Kind::List => json!("strings"),
                Kind::Text => json!("string"),
            };
            json!({"key": d.key, "kind": kind, "default": (d.default)(), "about": d.about})
        })
        .collect();
    serde_json::to_string_pretty(&v).unwrap_or_default()
}
