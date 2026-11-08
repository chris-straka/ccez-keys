//! Telex: Vietnamese on a plain keyboard. Doubled vowels take the hat
//! (aa → â, ee → ê, oo → ô), w the hook (aw → ă, ow → ơ, uw → ư, uow → ươ),
//! dd → đ, and a letter typed after the vowels sets the tone (s ´, f `,
//! r ̉, x ~, j ̣, z none). "Vieetj" → Việt, "dduowcj" → được.

use unicode_normalization::UnicodeNormalization;

fn tone_mark(c: char) -> Option<Option<char>> {
    Some(match c {
        's' => Some('\u{0301}'),
        'f' => Some('\u{0300}'),
        'r' => Some('\u{0309}'),
        'x' => Some('\u{0303}'),
        'j' => Some('\u{0323}'),
        'z' => None,
        _ => return None,
    })
}

fn is_vowel(c: char) -> bool {
    "aăâeêioôơuưy".contains(c.to_lowercase().next().unwrap_or(c))
}

fn same_case(model: char, c: char) -> char {
    if model.is_uppercase() { c.to_uppercase().next().unwrap_or(c) } else { c }
}

/// The word as Telex writes it, from the letters typed.
pub fn telex(raw: &str) -> String {
    let mut out: Vec<char> = Vec::new();
    let mut tone: Option<char> = None;
    let mut last_tone_key: Option<char> = None;
    for ch in raw.chars() {
        let l = ch.to_lowercase().next().unwrap_or(ch);
        let has_vowel = out.iter().any(|c| is_vowel(*c));
        if let Some(t) = tone_mark(l).filter(|_| has_vowel) {
            // The same tone key twice: the tone goes and the letter stays.
            if last_tone_key == Some(l) {
                tone = None;
                out.push(ch);
                last_tone_key = None;
                continue;
            }
            tone = t;
            last_tone_key = Some(l);
            continue;
        }
        last_tone_key = None;
        let last = out.last().copied();
        let ll = last.map(|c| c.to_lowercase().next().unwrap_or(c));
        match l {
            'a' | 'e' | 'o' if ll == Some(l) => {
                let to = match l { 'a' => 'â', 'e' => 'ê', _ => 'ô' };
                let m = out.pop().unwrap_or(ch);
                out.push(same_case(m, to));
            }
            'd' if ll == Some('d') => {
                let m = out.pop().unwrap_or(ch);
                out.push(same_case(m, 'đ'));
            }
            'w' => {
                // uo+w → ươ; else the last a/o/u takes the hook.
                let n = out.len();
                if n >= 2 && out[n - 2].to_lowercase().next() == Some('u') && out[n - 1].to_lowercase().next() == Some('o') {
                    out[n - 2] = same_case(out[n - 2], 'ư');
                    out[n - 1] = same_case(out[n - 1], 'ơ');
                } else if let Some(i) = out.iter().rposition(|c| "aou".contains(c.to_lowercase().next().unwrap_or(*c))) {
                    let to = match out[i].to_lowercase().next() { Some('a') => 'ă', Some('o') => 'ơ', _ => 'ư' };
                    out[i] = same_case(out[i], to);
                } else {
                    out.push(ch);
                }
            }
            _ => out.push(ch),
        }
    }
    let Some(t) = tone else { return out.into_iter().collect() };
    let at = tone_at(&out).unwrap_or(0);
    let mut s = String::new();
    for (i, c) in out.iter().enumerate() {
        s.push(*c);
        if i == at {
            s.push(t);
        }
    }
    s.nfc().collect()
}

/// Which vowel takes the tone: one with a hat or hook (ơ in ươ); else, with
/// a consonant after, the last vowel; else the middle of three, the first of two.
fn tone_at(w: &[char]) -> Option<usize> {
    let low: Vec<char> = w.iter().map(|c| c.to_lowercase().next().unwrap_or(*c)).collect();
    // qu / gi: their u and i belong to the consonant when a vowel follows.
    let mut start = 0;
    if low.len() > 2 && ((low[0] == 'q' && low[1] == 'u') || (low[0] == 'g' && low[1] == 'i' && is_vowel(low[2]))) {
        start = 2;
    }
    let first = (start..low.len()).find(|&i| is_vowel(low[i]))?;
    let mut end = first;
    while end + 1 < low.len() && is_vowel(low[end + 1]) {
        end += 1;
    }
    let cluster: Vec<usize> = (first..=end).collect();
    if let Some(&i) = cluster.iter().rev().find(|&&i| low[i] == 'ơ') {
        return Some(i);
    }
    if let Some(&i) = cluster.iter().find(|&&i| "ăâêôư".contains(low[i])) {
        return Some(i);
    }
    Some(match cluster.len() {
        1 => first,
        _ if end + 1 < low.len() => end,
        3 => first + 1,
        _ => first,
    })
}
