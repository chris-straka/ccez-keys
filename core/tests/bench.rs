//! How well autocorrect does, per language, on typos made the way thumbs
//! make them: a neighbouring key, two letters swapped, a letter missed or
//! doubled, accents left off. Also how often a word typed right is changed.
//!
//!     cargo test --release --test bench -- --ignored --nocapture
//!
//! Prints a table; the numbers to watch are `fixed` (higher is better),
//! `wrong` (changed into another word) and `broke` (a right word changed).

use ccez_keys_core::{Engine, KeyKind, Page};
use std::collections::HashMap;

const LANGS: &[&str] = &["en", "fr", "de", "es", "pt", "it", "pl", "cs", "tr", "id", "no", "ro", "hu", "nl", "sv", "ru", "uk", "bg", "el"];
const SAMPLE: usize = 600;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn engine(langs: &[&str]) -> std::sync::Arc<Engine> {
    let dir = std::env::temp_dir().join(format!("ccez-keys-bench-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let e = Engine::new(dir.to_string_lossy().into());
    for l in langs {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../data/words/");
        let mut text = std::fs::read_to_string(format!("{root}{l}.txt")).unwrap();
        if let Ok(extra) = std::fs::read_to_string(format!("{root}{l}-extra.txt")) {
            text += &extra;
        }
        e.load_words(l.to_string(), text);
    }
    e
}

/// Each letter's neighbours on the language's own layout.
fn neighbours(e: &Engine, lang: &str) -> HashMap<char, Vec<char>> {
    let rows: Vec<Vec<char>> = e
        .layout(lang.into(), Page::Letters)
        .rows
        .iter()
        .map(|r| r.keys.iter().filter(|k| k.kind == KeyKind::Char).filter_map(|k| k.text.chars().next()).filter(|c| c.is_alphabetic()).collect())
        .collect();
    let mut out: HashMap<char, Vec<char>> = HashMap::new();
    for (ri, row) in rows.iter().enumerate() {
        for (ci, &c) in row.iter().enumerate() {
            let mut n = Vec::new();
            if ci > 0 { n.push(row[ci - 1]); }
            if ci + 1 < row.len() { n.push(row[ci + 1]); }
            for dr in [-1i32, 1] {
                let r2 = ri as i32 + dr;
                if r2 < 0 || r2 as usize >= rows.len() { continue; }
                let other = &rows[r2 as usize];
                // Rows are offset by about half a key: the two keys under/over.
                for dc in [-1i32, 0, 1] {
                    let c2 = ci as i32 + dc;
                    if c2 >= 0 && (c2 as usize) < other.len() && dc != (if dr < 0 { 1 } else { -1 }) { n.push(other[c2 as usize]); }
                }
            }
            out.insert(c, n);
        }
    }
    out
}

fn strip_accents(w: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    w.nfd().filter(|c| !('\u{300}'..='\u{36f}').contains(c)).collect()
}

/// A typo of `w`, and what kind it is.
fn typo(w: &str, near: &HashMap<char, Vec<char>>, rng: &mut Rng) -> Option<(String, &'static str)> {
    let chars: Vec<char> = w.chars().collect();
    let n = chars.len();
    let plain = strip_accents(w);
    if plain != w && rng.below(3) == 0 {
        return Some((plain, "accents"));
    }
    for _ in 0..8 {
        let i = 1 + rng.below(n - 1); // the first letter is usually right
        let mut c = chars.clone();
        let kind = match rng.below(4) {
            0 => {
                let lower = c[i].to_lowercase().next()?;
                let opts = near.get(&lower)?;
                c[i] = opts[rng.below(opts.len())];
                "neighbour"
            }
            1 if i + 1 < n => {
                c.swap(i, i + 1);
                "swapped"
            }
            2 => {
                c.remove(i);
                "missed"
            }
            _ => {
                c.insert(i, c[i]);
                "doubled"
            }
        };
        let t: String = c.into_iter().collect();
        if t != w {
            return Some((t, kind));
        }
    }
    None
}

#[test]
#[ignore]
fn autocorrect_bench() {
    let e = engine(LANGS);
    println!("\nlang  fixed  left  wrong  broke   (typos: {SAMPLE} per language; broke: of {SAMPLE} right words)");
    let mut by_kind: HashMap<&str, (u32, u32)> = HashMap::new();
    let mut fails: Vec<String> = Vec::new();
    for &lang in LANGS {
        let mut kept = 0;
        let mut fails_here = |line: String| { if kept < 30 || line.contains(" broke ") { kept += 1; fails.push(line); } };
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../data/words/");
        let words: Vec<String> = std::fs::read_to_string(format!("{root}{lang}.txt"))
            .unwrap()
            .lines()
            .filter(|l| !l.starts_with('#'))
            .skip(60)
            .take(20000)
            .map(str::to_string)
            .filter(|w| w.chars().count() >= 4 && w.chars().all(char::is_alphabetic))
            .collect();
        let near = neighbours(&e, lang);
        let mut rng = Rng(0x9E3779B97F4A7C15 ^ lang.bytes().fold(0u64, |a, b| a * 31 + b as u64));
        let (mut fixed, mut left, mut wrong, mut broke) = (0, 0, 0, 0);
        let mut tried = 0;
        while tried < SAMPLE {
            // As often as people write it (Zipf: rank r about 1/r of the time).
            let r = ((words.len() as f64).ln() * (rng.below(1_000_000) as f64 / 1e6)).exp() as usize - 1;
            let w = &words[r.min(words.len() - 1)];
            let Some((t, kind)) = typo(w, &near, &mut rng) else { continue };
            // A typo that is itself a word isn't autocorrect's to fix.
            if e.suggest(lang.into(), t.clone()).typed_known { continue; }
            tried += 1;
            let got = e.suggest(lang.into(), t.clone()).correction;
            let k = by_kind.entry(kind).or_default();
            k.1 += 1;
            match got {
                Some(g) if g.to_lowercase() == w.to_lowercase() => { fixed += 1; k.0 += 1; }
                None => { left += 1; fails_here(format!("{lang} left  {kind:9} {t} (meant {w})")); }
                Some(g) => { wrong += 1; fails_here(format!("{lang} wrong {kind:9} {t} → {g} (meant {w})")); }
            }
        }
        for _ in 0..SAMPLE {
            let w = &words[rng.below(words.len())];
            if let Some(g) = e.suggest(lang.into(), w.clone()).correction {
                broke += 1;
                fails_here(format!("{lang} broke {w} → {g}"));
            }
        }
        let p = |x: u32| 100.0 * x as f32 / SAMPLE as f32;
        println!("{lang:4} {:5.1}% {:5.1}% {:5.1}% {:5.1}%", p(fixed), p(left), p(wrong), p(broke));
    }
    println!("\nby kind:");
    for (k, (ok, n)) in &by_kind {
        println!("  {k:9} {:5.1}% fixed of {n}", 100.0 * *ok as f32 / *n as f32);
    }
    let out = std::env::temp_dir().join("ccez-keys-bench-fails.txt");
    std::fs::write(&out, fails.join("\n")).unwrap();
    println!("\nexamples: {}", out.display());
}
