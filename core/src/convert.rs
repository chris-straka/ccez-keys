//! Typing Japanese and Chinese on QWERTY: romaji become kana as they're
//! typed, then kana (or pinyin letters) become words. The candidates come
//! from an index built once from the language's dictionary (JMdict's
//! readings, CC-CEDICT's pinyin), ranked by wordfreq; a candidate may cover
//! only the start of what's typed, and the rest stays to be converted next.

use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

/// Candidates kept per reading.
const PER_KEY: usize = 10;

/// Romaji typed so far as kana, and what's left over (a consonant waiting
/// for its vowel). "kyouha" → ("きょうは", ""), "kyout" → ("きょう", "t").
pub fn romaji_to_kana(input: &str) -> (String, String) {
    romaji(input, false)
}

/// As [`romaji_to_kana`]; `finish` takes a last lone "n" as ん (for
/// converting, when nothing more is coming).
pub fn romaji(input: &str, finish: bool) -> (String, String) {
    let (mut kana, pending) = romaji_inner(input);
    if finish {
        // Nothing more is coming: a last "n"/"nn" is ん, other letters stay as typed.
        match pending.as_str() {
            "" => {}
            "n" | "nn" => kana.push('ん'),
            p => kana.push_str(p),
        }
        return (kana, String::new());
    }
    (kana, pending)
}

fn romaji_inner(input: &str) -> (String, String) {
    let s: Vec<char> = input.to_lowercase().chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        let next = s.get(i + 1).copied();
        // ん: "nn", "n'", or n before a consonant other than y.
        if c == 'n' {
            match next {
                // "nn" is ん, unless a vowel follows: then the second n
                // starts the next kana ("konnichiha" → こんにちは).
                Some('n') => {
                    // Typed one letter at a time: "nn" waits, a vowel may still follow.
                    if i + 2 == s.len() {
                        return (out, "nn".into());
                    }
                    out.push('ん');
                    i += if s.get(i + 2).is_some_and(|c| "aiueoy".contains(*c)) { 1 } else { 2 };
                    continue;
                }
                Some('\'') => { out.push('ん'); i += 2; continue; }
                Some(n) if !"aiueoy".contains(n) => { out.push('ん'); i += 1; continue; }
                _ => {}
            }
        }
        // っ: a doubled consonant ("tt", "kk", "cch").
        if let Some(n) = next {
            if c == n && c.is_ascii_alphabetic() && !"aiueon".contains(c) || (c == 't' && n == 'c') {
                out.push('っ');
                i += 1;
                continue;
            }
        }
        if c == '-' {
            out.push('ー');
            i += 1;
            continue;
        }
        let mut done = false;
        for len in (1..=4).rev() {
            if i + len > s.len() {
                continue;
            }
            let key: String = s[i..i + len].iter().collect();
            if let Some(k) = kana_of(&key) {
                out.push_str(k);
                i += len;
                done = true;
                break;
            }
        }
        if !done {
            if c.is_ascii_alphabetic() {
                // A consonant still waiting for its vowel: left over.
                return (out, s[i..].iter().collect());
            }
            out.push(c);
            i += 1;
        }
    }
    (out, String::new())
}

fn kana_of(r: &str) -> Option<&'static str> {
    Some(match r {
        "a" => "あ", "i" => "い", "u" => "う", "e" => "え", "o" => "お",
        "ka" => "か", "ki" => "き", "ku" => "く", "ke" => "け", "ko" => "こ",
        "kya" => "きゃ", "kyu" => "きゅ", "kyo" => "きょ",
        "sa" => "さ", "shi" | "si" => "し", "su" => "す", "se" => "せ", "so" => "そ",
        "sha" | "sya" => "しゃ", "shu" | "syu" => "しゅ", "sho" | "syo" => "しょ", "she" => "しぇ",
        "ta" => "た", "chi" | "ti" => "ち", "tsu" | "tu" => "つ", "te" => "て", "to" => "と",
        "cha" | "tya" => "ちゃ", "chu" | "tyu" => "ちゅ", "cho" | "tyo" => "ちょ", "che" => "ちぇ",
        "na" => "な", "ni" => "に", "nu" => "ぬ", "ne" => "ね", "no" => "の",
        "nya" => "にゃ", "nyu" => "にゅ", "nyo" => "にょ",
        "ha" => "は", "hi" => "ひ", "fu" | "hu" => "ふ", "he" => "へ", "ho" => "ほ",
        "hya" => "ひゃ", "hyu" => "ひゅ", "hyo" => "ひょ",
        "fa" => "ふぁ", "fi" => "ふぃ", "fe" => "ふぇ", "fo" => "ふぉ",
        "ma" => "ま", "mi" => "み", "mu" => "む", "me" => "め", "mo" => "も",
        "mya" => "みゃ", "myu" => "みゅ", "myo" => "みょ",
        "ya" => "や", "yu" => "ゆ", "yo" => "よ",
        "ra" => "ら", "ri" => "り", "ru" => "る", "re" => "れ", "ro" => "ろ",
        "rya" => "りゃ", "ryu" => "りゅ", "ryo" => "りょ",
        "wa" => "わ", "wo" => "を", "wi" => "うぃ", "we" => "うぇ",
        "ga" => "が", "gi" => "ぎ", "gu" => "ぐ", "ge" => "げ", "go" => "ご",
        "gya" => "ぎゃ", "gyu" => "ぎゅ", "gyo" => "ぎょ",
        "za" => "ざ", "ji" | "zi" => "じ", "zu" => "ず", "ze" => "ぜ", "zo" => "ぞ",
        "ja" | "zya" | "jya" => "じゃ", "ju" | "zyu" | "jyu" => "じゅ", "jo" | "zyo" | "jyo" => "じょ", "je" => "じぇ",
        "da" => "だ", "di" => "ぢ", "du" => "づ", "de" => "で", "do" => "ど",
        "dhi" => "でぃ", "thi" => "てぃ",
        "ba" => "ば", "bi" => "び", "bu" => "ぶ", "be" => "べ", "bo" => "ぼ",
        "bya" => "びゃ", "byu" => "びゅ", "byo" => "びょ",
        "pa" => "ぱ", "pi" => "ぴ", "pu" => "ぷ", "pe" => "ぺ", "po" => "ぽ",
        "pya" => "ぴゃ", "pyu" => "ぴゅ", "pyo" => "ぴょ",
        "vu" => "ゔ",
        "xa" | "la" => "ぁ", "xi" | "li" => "ぃ", "xu" | "lu" => "ぅ", "xe" | "le" => "ぇ", "xo" | "lo" => "ぉ",
        "xtu" | "ltu" | "xtsu" => "っ", "xya" | "lya" => "ゃ", "xyu" | "lyu" => "ゅ", "xyo" | "lyo" => "ょ",
        _ => return None,
    })
}

/// Katakana for hiragana (the same word written for loanwords).
pub fn to_katakana(s: &str) -> String {
    s.chars().map(|c| if ('\u{3041}'..='\u{3096}').contains(&c) { char::from_u32(c as u32 + 0x60).unwrap_or(c) } else { c }).collect()
}

fn to_hiragana(s: &str) -> String {
    s.chars().map(|c| if ('\u{30A1}'..='\u{30F6}').contains(&c) { char::from_u32(c as u32 - 0x60).unwrap_or(c) } else { c }).collect()
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Conversion {
    pub text: String,
    /// Kana (Japanese) or pinyin with tone marks (Chinese), for over the word.
    pub reading: String,
    /// How much of the input it covers (kana for Japanese, letters for Chinese).
    pub consumed: u32,
}

/// reading → (word, reading to show, ln frequency), commonest first.
#[derive(Default)]
pub struct Index {
    map: HashMap<String, Vec<(String, String, f32)>>,
    /// Longest key, to bound prefix search.
    longest: usize,
}

fn ranks(freq: &str) -> HashMap<&str, usize> {
    freq.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()).enumerate().map(|(i, w)| (w.trim(), i + 1)).collect()
}

/// From JMdict (ccez-lang-core's ja.tsv.gz: `#id \t [[kanji…],[readings…],common,…]`):
/// words ranked by wordfreq (`freq`), common words next, keeping those two.
pub fn build_japanese(dict_gz: &Path, freq: &str, out: &Path) -> std::io::Result<usize> {
    let rank = ranks(freq);
    let mut all: HashMap<String, Vec<(usize, String)>> = HashMap::new();
    let reader = BufReader::new(flate2::read::GzDecoder::new(fs::File::open(dict_gz)?));
    for line in reader.lines() {
        let line = line?;
        let Some((id, json)) = line.split_once('\t') else { continue };
        if !id.starts_with('#') {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else { continue };
        let strs = |i: usize| -> Vec<String> { v.get(i).and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default() };
        let kanji = strs(0);
        let readings = strs(1);
        let common = v.get(2).and_then(|c| c.as_i64()).unwrap_or(0) > 0;
        let forms = if kanji.is_empty() { readings.clone() } else { kanji };
        for r in &readings {
            let key = to_hiragana(r);
            for f in &forms {
                let score = rank.get(f.as_str()).copied().unwrap_or(if common { 40_000 } else { usize::MAX });
                if score == usize::MAX {
                    continue;
                }
                all.entry(key.clone()).or_default().push((score, f.clone()));
            }
        }
    }
    write_index(all, out)
}

/// From CC-CEDICT (`trad simp [pin1 yin1] /…/`): keys are the toneless
/// pinyin run together ("nihao"), words the simplified forms.
pub fn build_chinese(dict_gz: &Path, freq: &str, out: &Path) -> std::io::Result<usize> {
    let rank = ranks(freq);
    let mut all: HashMap<String, Vec<(usize, String)>> = HashMap::new();
    let reader = BufReader::new(flate2::read::GzDecoder::new(fs::File::open(dict_gz)?));
    for line in reader.lines() {
        let line = line?;
        if line.starts_with('#') {
            continue;
        }
        let Some((head, _)) = line.split_once(" /") else { continue };
        let Some((words, pinyin)) = head.split_once(" [") else { continue };
        let simp = words.split(' ').nth(1).unwrap_or("").to_string();
        let pinyin = pinyin.trim_end_matches(']');
        if simp.is_empty() {
            continue;
        }
        // Proper nouns are capitalised (Zhong1 guo2 中国): lower case for the key.
        let key: String = pinyin.to_lowercase().chars().filter(|c| c.is_ascii_lowercase() || *c == 'ü' || *c == ':').map(|c| if c == 'ü' { 'v' } else { c }).filter(|c| *c != ':').collect();
        let score = rank.get(simp.as_str()).copied().unwrap_or(60_000 + simp.chars().count() * 1000);
        let shown = ccez_lang_core::words::numbered_to_marks(pinyin);
        all.entry(key).or_default().push((score, format!("{simp}\u{1}{shown}")));
    }
    write_index(all, out)
}

fn write_index(all: HashMap<String, Vec<(usize, String)>>, out: &Path) -> std::io::Result<usize> {
    let mut f = std::io::BufWriter::new(fs::File::create(out)?);
    let mut n = 0;
    for (key, mut v) in all {
        v.sort_by_key(|(s, _)| *s);
        let mut seen: Vec<&str> = Vec::new();
        let mut words: Vec<&str> = Vec::new();
        let mut ranked: Vec<String> = Vec::new();
        for (rank, w) in &v {
            let word = w.split('\u{1}').next().unwrap_or(w);
            if !seen.contains(&word) {
                seen.push(word);
                words.push(w);
                ranked.push(format!("{w}\u{2}{rank}"));
            }
            if words.len() == PER_KEY {
                break;
            }
        }
        writeln!(f, "{key}\t{}", ranked.join("\t"))?;
        n += 1;
    }
    f.flush()?;
    Ok(n)
}

impl Index {
    pub fn load(path: &Path) -> Option<Self> {
        let text = fs::read_to_string(path).ok()?;
        let mut map = HashMap::new();
        let mut longest = 0;
        for line in text.lines() {
            let mut parts = line.split('\t');
            let Some(key) = parts.next() else { continue };
            let words: Vec<(String, String, f32)> = parts
                .map(|w| {
                    let (w, rank) = w.split_once('\u{2}').map(|(a, r)| (a, r.parse::<f32>().unwrap_or(1e6))).unwrap_or((w, 1e6));
                    let logf = -rank.max(1.0).ln();
                    match w.split_once('\u{1}') {
                        Some((word, shown)) => (word.to_string(), shown.to_string(), logf),
                        None => (w.to_string(), key.to_string(), logf),
                    }
                })
                .collect();
            longest = longest.max(key.chars().count());
            map.insert(key.to_string(), words);
        }
        Some(Index { map, longest })
    }

    /// The likeliest way to cut `chars` into words (each piece's frequency,
    /// less a cost per piece): the length of the first piece, if any.
    fn best_first(&self, chars: &[char]) -> Option<usize> {
        const PIECE: f32 = 4.0;
        const UNKNOWN: f32 = -14.0;
        let n = chars.len();
        let mut best = vec![f32::NEG_INFINITY; n + 1];
        let mut first = vec![0usize; n + 1];
        best[0] = 0.0;
        for i in 0..n {
            if best[i] == f32::NEG_INFINITY {
                continue;
            }
            for len in 1..=(n - i).min(self.longest) {
                let key: String = chars[i..i + len].iter().collect();
                let score = match self.map.get(&key).and_then(|w| w.first()) {
                    Some((_, _, logf)) => *logf - PIECE,
                    None if len == 1 => UNKNOWN,
                    None => continue,
                };
                let total = best[i] + score;
                if total > best[i + len] {
                    best[i + len] = total;
                    first[i + len] = if i == 0 { len } else { first[i] };
                }
            }
        }
        (best[n] > f32::NEG_INFINITY && first[n] > 0).then_some(first[n])
    }

    /// Words for `input`: the whole of it first, then the start the likeliest
    /// reading of all of it begins with, then the other starts, longest first.
    pub fn convert(&self, input: &str, japanese: bool) -> Vec<Conversion> {
        let chars: Vec<char> = input.chars().collect();
        let n = chars.len();
        let mut out: Vec<Conversion> = Vec::new();
        let push = |out: &mut Vec<Conversion>, text: &str, reading: &str, consumed: usize| {
            if !out.iter().any(|c| c.text == text && c.consumed as usize == consumed) {
                out.push(Conversion { text: text.into(), reading: reading.into(), consumed: consumed as u32 });
            }
        };
        let whole: String = chars.iter().collect();
        if let Some(words) = self.map.get(&whole) {
            for (w, shown, _) in words.iter().take(PER_KEY) {
                push(&mut out, w, shown, n);
            }
        }
        // "がすき": が + 好き reads better than ガス + き, so が leads.
        if let Some(len) = self.best_first(&chars).filter(|&l| l < n) {
            let key: String = chars[..len].iter().collect();
            if let Some(words) = self.map.get(&key) {
                for (w, shown, _) in words.iter().take(3) {
                    push(&mut out, w, shown, len);
                }
            }
        }
        for len in (1..n.min(self.longest + 1)).rev() {
            let key: String = chars[..len].iter().collect();
            if let Some(words) = self.map.get(&key) {
                for (w, shown, _) in words.iter().take(3) {
                    push(&mut out, w, shown, len);
                }
            }
            if out.len() >= 12 {
                break;
            }
        }
        if japanese && n > 0 {
            // The kana themselves, and in katakana.
            push(&mut out, input, "", n);
            push(&mut out, &to_katakana(input), "", n);
        }
        out
    }
}
