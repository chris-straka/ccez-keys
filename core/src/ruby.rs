//! Furigana split by kanji run: each run of kanji gets its own part of the
//! reading, kana in the word get none, found by matching the word's kana
//! against the reading. A split that fails or could go more than one way
//! falls back to the whole reading over the whole word.

use crate::learn::Ruby;

fn is_kanji(c: char) -> bool {
    matches!(
        c,
        '\u{4E00}'..='\u{9FFF}'
            | '\u{3400}'..='\u{4DBF}'
            | '\u{F900}'..='\u{FAFF}'
            | '々'
            | '〆'
            | 'ヶ'
            | 'ヵ'
    )
}

fn is_hiragana(c: char) -> bool {
    matches!(c, '\u{3041}'..='\u{3096}' | 'ゝ' | 'ゞ')
}

fn is_katakana(c: char) -> bool {
    matches!(c, '\u{30A1}'..='\u{30FA}' | 'ー' | 'ヽ' | 'ヾ') && !matches!(c, 'ヵ' | 'ヶ')
}

fn fold_kana(c: char) -> char {
    if ('\u{30A1}'..='\u{30F4}').contains(&c) {
        char::from_u32(c as u32 - 0x60).unwrap_or(c)
    } else {
        c
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Kanji,
    Hiragana,
    Katakana,
}

struct Run {
    kind: Kind,
    text: String,
    folded: Vec<char>,
}

/// Split `word` and `reading` into ruby parts: each run of kanji with its own
/// reading, each run of kana with an empty reading. Falls back to one part
/// with the whole reading when the split fails or is ambiguous, or an empty
/// reading when `reading` is blank or `word` has no kanji.
#[uniffi::export]
pub fn ruby_split(word: String, reading: String) -> Vec<Ruby> {
    let reading = reading.trim();
    if reading.is_empty() || !word.chars().any(is_kanji) {
        return vec![Ruby { text: word, rt: String::new() }];
    }

    let whole = || vec![Ruby { text: word.clone(), rt: reading.to_string() }];

    let mut runs: Vec<Run> = Vec::new();
    for c in word.chars() {
        let kind = if is_kanji(c) {
            Kind::Kanji
        } else if is_hiragana(c) {
            Kind::Hiragana
        } else if is_katakana(c) {
            if c == 'ー' && runs.last().is_some_and(|r| r.kind == Kind::Hiragana) {
                Kind::Hiragana
            } else {
                Kind::Katakana
            }
        } else {
            return whole();
        };
        if let Some(last) = runs.last_mut().filter(|r| r.kind == kind) {
            last.text.push(c);
        } else {
            runs.push(Run { kind, text: c.to_string(), folded: Vec::new() });
        }
    }
    for r in &mut runs {
        if r.kind != Kind::Kanji {
            r.folded = r.text.chars().map(fold_kana).collect();
        }
    }

    let r_chars: Vec<char> = reading.chars().collect();
    let r_folded: Vec<char> = r_chars.iter().copied().map(fold_kana).collect();

    let mut min_len = vec![0usize; runs.len() + 1];
    for i in (0..runs.len()).rev() {
        min_len[i] = min_len[i + 1] + if runs[i].kind == Kind::Kanji { 1 } else { runs[i].folded.len() };
    }

    let mut current: Vec<(usize, usize)> = Vec::with_capacity(runs.len());
    let mut solutions: Vec<Vec<(usize, usize)>> = Vec::new();

    fn search(
        runs: &[Run],
        min_len: &[usize],
        r_folded: &[char],
        idx: usize,
        r_idx: usize,
        current: &mut Vec<(usize, usize)>,
        solutions: &mut Vec<Vec<(usize, usize)>>,
    ) {
        if solutions.len() >= 2 {
            return;
        }
        if idx == runs.len() {
            if r_idx == r_folded.len() {
                solutions.push(current.clone());
            }
            return;
        }
        if r_folded.len() - r_idx < min_len[idx] {
            return;
        }
        let run = &runs[idx];
        if run.kind != Kind::Kanji {
            let k = &run.folded;
            if r_folded[r_idx..r_idx + k.len()] == k[..] {
                current.push((0, 0));
                search(runs, min_len, r_folded, idx + 1, r_idx + k.len(), current, solutions);
                current.pop();
            }
        } else {
            let max_end = r_folded.len() - min_len[idx + 1];
            let start_end = if idx + 1 == runs.len() { r_folded.len() } else { r_idx + 1 };
            for end in start_end..=max_end {
                current.push((r_idx, end));
                search(runs, min_len, r_folded, idx + 1, end, current, solutions);
                current.pop();
                if solutions.len() >= 2 {
                    return;
                }
            }
        }
    }

    search(&runs, &min_len, &r_folded, 0, 0, &mut current, &mut solutions);
    if solutions.len() != 1 {
        return whole();
    }
    let sol = &solutions[0];
    runs.into_iter()
        .zip(sol.iter())
        .map(|(r, &(start, end))| {
            let rt = if r.kind == Kind::Kanji {
                r_chars[start..end].iter().collect()
            } else {
                String::new()
            };
            Ruby { text: r.text, rt }
        })
        .collect()
}
