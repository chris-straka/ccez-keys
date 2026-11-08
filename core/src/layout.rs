//! Key layouts, from data files (`data/layouts/*.json`): a new script is
//! a new file, not new code.

use std::collections::HashMap;

use serde::Deserialize;

use crate::words::Neighbours;

#[derive(Deserialize)]
struct LayoutFile {
    languages: Vec<String>,
    letters: Vec<Vec<String>>,
    popups: HashMap<String, HashMap<String, Vec<String>>>,
    period: Vec<String>,
    symbols: Vec<Vec<String>>,
    symbols_more: Vec<Vec<String>>,
    symbol_popups: HashMap<String, Vec<String>>,
}

const FILES: &[&str] = &[include_str!("../../data/layouts/latin.json")];

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum KeyKind {
    Char,
    Shift,
    Backspace,
    /// To the symbols page.
    Symbols,
    /// Back to the letters.
    Letters,
    /// The second symbols page.
    More,
    /// Emoji and kaomoji.
    Face,
    Space,
    Enter,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Key {
    pub kind: KeyKind,
    pub label: String,
    /// What a tap types (Char keys).
    pub text: String,
    /// Small mark in the corner (the top row's digits).
    pub hint: String,
    /// What holding the key offers, first one preselected.
    pub popups: Vec<String>,
    /// In key widths.
    pub width: f32,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Row {
    pub keys: Vec<Key>,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Layout {
    pub rows: Vec<Row>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum Page {
    Letters,
    Symbols,
    SymbolsMore,
}

pub struct Layouts {
    files: Vec<LayoutFile>,
}

fn key(kind: KeyKind, label: &str, width: f32) -> Key {
    Key { kind, label: label.into(), text: String::new(), hint: String::new(), popups: vec![], width }
}

fn ch(text: &str, popups: Vec<String>) -> Key {
    Key { kind: KeyKind::Char, label: text.into(), text: text.into(), hint: String::new(), popups, width: 1.0 }
}

impl Layouts {
    pub fn load() -> Self {
        Layouts { files: FILES.iter().map(|f| serde_json::from_str(f).expect("layout data")).collect() }
    }

    fn file(&self, lang: &str) -> Option<&LayoutFile> {
        self.files.iter().find(|f| f.languages.iter().any(|l| l == lang))
    }

    pub fn has(&self, lang: &str) -> bool {
        self.file(lang).is_some()
    }

    pub fn neighbours(&self, lang: &str) -> Neighbours {
        self.file(lang).map(|f| Neighbours::from_rows(&f.letters)).unwrap_or_default()
    }

    pub fn layout(&self, lang: &str, page: Page) -> Layout {
        let Some(f) = self.file(lang).or_else(|| self.files.first()) else { return Layout { rows: vec![] } };
        let pops = f.popups.get(lang).or_else(|| f.popups.get("en"));
        let mut rows: Vec<Row> = Vec::new();
        let grid = match page {
            Page::Letters => &f.letters,
            Page::Symbols => &f.symbols,
            Page::SymbolsMore => &f.symbols_more,
        };
        for (r, row) in grid.iter().enumerate() {
            let mut keys: Vec<Key> = row
                .iter()
                .enumerate()
                .map(|(i, c)| match page {
                    Page::Letters => {
                        let mut popups: Vec<String> = Vec::new();
                        let digit = if r == 0 && i < 10 { Some(((i + 1) % 10).to_string()) } else { None };
                        if let Some(d) = &digit {
                            popups.push(d.clone());
                        }
                        if let Some(p) = pops.and_then(|p| p.get(c)) {
                            popups.extend(p.iter().cloned());
                        }
                        let mut k = ch(c, popups);
                        k.hint = digit.unwrap_or_default();
                        k
                    }
                    _ => ch(c, f.symbol_popups.get(c).cloned().unwrap_or_default()),
                })
                .collect();
            if r == grid.len() - 1 {
                let first = match page {
                    Page::Letters => key(KeyKind::Shift, "⇧", 1.5),
                    Page::Symbols => key(KeyKind::More, "=\\<", 1.5),
                    Page::SymbolsMore => key(KeyKind::Symbols, "?123", 1.5),
                };
                keys.insert(0, first);
                keys.push(key(KeyKind::Backspace, "⌫", 1.5));
            }
            rows.push(Row { keys });
        }
        let mut period = ch(".", f.period.clone());
        period.width = 0.8;
        let mut comma = ch(",", vec![]);
        comma.width = 0.8;
        let switch = match page {
            Page::Letters => key(KeyKind::Symbols, "?123", 1.3),
            _ => key(KeyKind::Letters, "ABC", 1.3),
        };
        rows.push(Row {
            keys: vec![
                switch,
                key(KeyKind::Face, "☺", 1.0),
                comma,
                key(KeyKind::Space, "", 3.8),
                period,
                key(KeyKind::Enter, "↵", 1.3),
            ],
        });
        Layout { rows }
    }
}
