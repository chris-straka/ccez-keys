//! Every setting, in one table: the keyboard's settings panel and the
//! `keys` CLI both read it, so a new setting shows up in both.

use serde_json::{Map, Value, json};

pub enum Kind {
    Bool,
    Int { min: i64, max: i64 },
    /// Language codes, in order.
    Langs,
    /// Plain strings (app package names).
    List,
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
    Def { key: "keyboard.height", kind: Kind::Int { min: 80, max: 130 }, default: || json!(100), about: "Key height, percent." },
    Def { key: "languages.mine", kind: Kind::Langs, default: || json!(["en"]), about: "Languages the owner speaks, first is the main one." },
    Def {
        key: "languages.learning",
        kind: Kind::Langs,
        default: || json!(["fr", "de", "es", "ja", "zh", "ar"]),
        about: "Languages being learned, in the order the learning strip uses. All of them are typed in too.",
    },
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

fn check(d: &Def, v: &Value) -> Result<(), String> {
    let ok = match d.kind {
        Kind::Bool => v.is_boolean(),
        Kind::Int { min, max } => v.as_i64().is_some_and(|n| (min..=max).contains(&n)),
        Kind::Langs => v.as_array().is_some_and(|a| {
            a.iter().all(|x| x.as_str().is_some_and(|s| (2..=3).contains(&s.len()) && s.chars().all(|c| c.is_ascii_lowercase())))
        }),
        Kind::List => v.as_array().is_some_and(|a| a.iter().all(Value::is_string)),
    };
    if ok {
        Ok(())
    } else {
        Err(match d.kind {
            Kind::Bool => format!("{} takes true or false", d.key),
            Kind::Int { min, max } => format!("{} takes a number from {min} to {max}", d.key),
            Kind::Langs => format!("{} takes a list of language codes, like [\"fr\",\"ja\"]", d.key),
            Kind::List => format!("{} takes a list of strings", d.key),
        })
    }
}

pub fn schema() -> String {
    let v: Vec<Value> = DEFS
        .iter()
        .map(|d| {
            let kind = match d.kind {
                Kind::Bool => json!("bool"),
                Kind::Int { min, max } => json!({"int": [min, max]}),
                Kind::Langs => json!("languages"),
                Kind::List => json!("strings"),
            };
            json!({"key": d.key, "kind": kind, "default": (d.default)(), "about": d.about})
        })
        .collect();
    serde_json::to_string_pretty(&v).unwrap_or_default()
}
