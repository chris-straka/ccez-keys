//! A language's words, and what to suggest for what was typed.
//!
//! Words are kept in a trie over their folded spelling (lower case, no
//! accents, œ → oe, ß → ss), so "deja" finds "déjà" at no cost. A typed
//! word is matched by walking the trie with one edit-distance row per
//! letter (Damerau, with costs: a neighbouring key is a cheap slip, a
//! left-out apostrophe cheaper still). The same walk finds completions:
//! words whose start matches what's typed so far.

use std::collections::{BinaryHeap, HashMap};
use std::cmp::Ordering;

use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

/// Lower case, accents off, ligatures and ß spelled out, curly apostrophes straight.
pub fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.nfd() {
        if is_combining_mark(c) {
            continue;
        }
        for l in c.to_lowercase() {
            match l {
                'œ' => out.push_str("oe"),
                'æ' => out.push_str("ae"),
                'ß' => out.push_str("ss"),
                '’' | 'ʼ' | '‘' => out.push('\''),
                _ => out.push(l),
            }
        }
    }
    out
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub word: String,
    pub folded: String,
    /// ln of the word's relative frequency (0 for the commonest word).
    pub logf: f32,
}

#[derive(Default)]
struct Node {
    kids: Vec<(char, u32)>,
    ends: Vec<u32>,
    /// Highest `logf` of any word at or under this node.
    best: f32,
}

/// Which keys sit next to which, from a layout's letter rows.
#[derive(Default, Clone)]
pub struct Neighbours(HashMap<char, Vec<char>>);

impl Neighbours {
    /// Rows of letters; each row is shifted the way a phone keyboard
    /// staggers them (second row by half a key, third by one and a half).
    pub fn from_rows(rows: &[Vec<String>]) -> Self {
        let offsets = [0.0f32, 0.5, 1.5, 2.0];
        let mut at: Vec<(char, f32, f32)> = Vec::new();
        for (r, row) in rows.iter().enumerate() {
            for (i, k) in row.iter().enumerate() {
                if let Some(c) = k.chars().next() {
                    at.push((c, i as f32 + offsets.get(r).copied().unwrap_or(0.0), r as f32));
                }
            }
        }
        let mut map: HashMap<char, Vec<char>> = HashMap::new();
        for &(a, ax, ay) in &at {
            for &(b, bx, by) in &at {
                if a != b && (ax - bx).abs() <= 1.0 && (ay - by).abs() <= 1.0 {
                    map.entry(a).or_default().push(b);
                }
            }
        }
        Neighbours(map)
    }

    fn near(&self, a: char, b: char) -> bool {
        self.0.get(&a).is_some_and(|v| v.contains(&b))
    }
}

/// Edit costs. One whole edit is 1.0.
const SLIP: f32 = 0.55; // a neighbouring key
const SWAP: f32 = 0.7; // two letters the wrong way round
const DOUBLED: f32 = 0.5; // a letter typed twice ("helllo")
const MARK: f32 = 0.3; // a left-out apostrophe or hyphen ("jai" for "j'ai")

#[derive(Clone, Copy, Debug)]
pub struct Cand {
    pub id: u32,
    /// The cost of reading the whole typed word as this word.
    pub cost: f32,
    /// True when this is a longer word that starts like what's typed.
    pub completion: bool,
}

#[derive(Default)]
pub struct Lexicon {
    pub entries: Vec<Entry>,
    nodes: Vec<Node>,
    by_folded: HashMap<String, Vec<u32>>,
}

impl Lexicon {
    /// Words, commonest first (one per line; `#` lines are comments).
    /// A line may carry its own rank after a tab, for words the list
    /// lacks (French contractions).
    pub fn from_ranked(text: &str) -> Self {
        let mut lex = Lexicon { nodes: vec![Node { best: f32::MIN, ..Default::default() }], ..Default::default() };
        let mut rank = 0u32;
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            rank += 1;
            let (word, r) = match line.split_once('\t') {
                Some((w, r)) => (w, r.trim().parse().unwrap_or(rank)),
                None => (line, rank),
            };
            lex.insert(word, -(r.max(1) as f32).ln());
        }
        lex
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn insert(&mut self, word: &str, logf: f32) {
        let folded = fold(word);
        if folded.is_empty() {
            return;
        }
        if let Some(ids) = self.by_folded.get(&folded) {
            if let Some(&id) = ids.iter().find(|&&id| self.entries[id as usize].word == word) {
                let e = &mut self.entries[id as usize];
                if logf > e.logf {
                    e.logf = logf;
                    self.raise(&folded, logf);
                }
                return;
            }
        }
        let id = self.entries.len() as u32;
        self.entries.push(Entry { word: word.to_string(), folded: folded.clone(), logf });
        self.by_folded.entry(folded.clone()).or_default().push(id);
        let mut n = 0usize;
        self.nodes[0].best = self.nodes[0].best.max(logf);
        for c in folded.chars() {
            let next = match self.nodes[n].kids.iter().find(|(k, _)| *k == c) {
                Some(&(_, i)) => i as usize,
                None => {
                    self.nodes.push(Node { best: f32::MIN, ..Default::default() });
                    let i = self.nodes.len() - 1;
                    self.nodes[n].kids.push((c, i as u32));
                    i
                }
            };
            n = next;
            self.nodes[n].best = self.nodes[n].best.max(logf);
        }
        self.nodes[n].ends.push(id);
    }

    fn raise(&mut self, folded: &str, logf: f32) {
        let mut n = 0usize;
        self.nodes[0].best = self.nodes[0].best.max(logf);
        for c in folded.chars() {
            match self.nodes[n].kids.iter().find(|(k, _)| *k == c) {
                Some(&(_, i)) => n = i as usize,
                None => return,
            }
            self.nodes[n].best = self.nodes[n].best.max(logf);
        }
    }

    /// Entries spelled like `word` once folded ("deja" → déjà).
    pub fn same_folded(&self, word: &str) -> impl Iterator<Item = &Entry> {
        self.by_folded.get(&fold(word)).into_iter().flatten().map(|&id| &self.entries[id as usize])
    }

    /// Is this exactly a word here (case aside)?
    pub fn knows(&self, word: &str) -> bool {
        let lower = word.to_lowercase();
        self.same_folded(word).any(|e| e.word.to_lowercase() == lower)
    }

    /// Words within `max` edits of `typed`, and completions of it (when
    /// `completions`), cheapest per word.
    pub fn search(&self, typed: &str, max: f32, completions: usize, keys: &Neighbours) -> Vec<Cand> {
        let t: Vec<char> = fold(typed).chars().collect();
        let n = t.len();
        if n == 0 {
            return Vec::new();
        }
        let comp_max = if n <= 3 { 0.0 } else { SLIP };
        let mut found: HashMap<u32, Cand> = HashMap::new();
        let first: Vec<f32> = (0..=n).map(|j| j as f32).collect();
        let mut rows: Vec<Vec<f32>> = vec![first];
        let mut path: Vec<char> = Vec::new();
        let mut walk = Walk { lex: self, t: &t, max, comp_max, completions, keys, found: &mut found };
        walk.visit(0, &mut rows, &mut path, f32::INFINITY);
        let mut out: Vec<Cand> = found.into_values().collect();
        out.sort_by(|a, b| a.cost.partial_cmp(&b.cost).unwrap_or(Ordering::Equal));
        out
    }

    /// The `k` commonest words under node `n`.
    fn top_under(&self, n: usize, k: usize) -> Vec<u32> {
        #[derive(PartialEq)]
        struct Item(f32, usize);
        impl Eq for Item {}
        impl PartialOrd for Item {
            fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
                Some(self.cmp(o))
            }
        }
        impl Ord for Item {
            fn cmp(&self, o: &Self) -> Ordering {
                self.0.partial_cmp(&o.0).unwrap_or(Ordering::Equal)
            }
        }
        let mut heap = BinaryHeap::new();
        heap.push(Item(self.nodes[n].best, n));
        let mut ends: Vec<u32> = Vec::new();
        while let Some(Item(best, i)) = heap.pop() {
            if ends.len() >= k {
                let worst = ends.iter().map(|&id| self.entries[id as usize].logf).fold(f32::MAX, f32::min);
                if best <= worst {
                    break;
                }
            }
            for &id in &self.nodes[i].ends {
                ends.push(id);
            }
            if ends.len() > k {
                ends.sort_by(|a, b| self.entries[*b as usize].logf.partial_cmp(&self.entries[*a as usize].logf).unwrap_or(Ordering::Equal));
                ends.truncate(k);
            }
            for &(_, kid) in &self.nodes[i].kids {
                heap.push(Item(self.nodes[kid as usize].best, kid as usize));
            }
        }
        ends
    }
}

struct Walk<'a> {
    lex: &'a Lexicon,
    t: &'a [char],
    max: f32,
    comp_max: f32,
    completions: usize,
    keys: &'a Neighbours,
    found: &'a mut HashMap<u32, Cand>,
}

impl Walk<'_> {
    fn keep(&mut self, c: Cand) {
        let e = self.found.entry(c.id).or_insert(c);
        if c.cost < e.cost {
            *e = c;
        }
    }

    /// `rows[i]` is the edit row for the trie path so far (length i).
    fn visit(&mut self, node: usize, rows: &mut Vec<Vec<f32>>, path: &mut Vec<char>, best_prefix: f32) {
        let n = self.t.len();
        let row = rows.last().unwrap().clone();
        let whole = row[n];
        if whole <= self.max {
            for &id in &self.lex.nodes[node].ends {
                self.keep(Cand { id, cost: whole, completion: false });
            }
        }
        // Everything typed reads as the start of this path: offer the
        // commonest words under it, once, at the cheapest such point.
        let mut best_prefix = best_prefix;
        if self.completions > 0 && whole <= self.comp_max && whole < best_prefix && !path.is_empty() {
            best_prefix = whole;
            for id in self.lex.top_under(node, self.completions) {
                if self.lex.entries[id as usize].folded.chars().count() > n {
                    self.keep(Cand { id, cost: whole, completion: true });
                }
            }
        }
        if row.iter().cloned().fold(f32::INFINITY, f32::min) > self.max {
            return;
        }
        let kids = self.lex.nodes[node].kids.clone();
        for (c, kid) in kids {
            let i = path.len() + 1;
            let mut next = vec![0f32; n + 1];
            next[0] = row[0] + self.insert_cost(c);
            for j in 1..=n {
                let tj = self.t[j - 1];
                let sub = row[j - 1] + self.sub_cost(c, tj);
                // A letter in the word the typist left out.
                let left_out = row[j] + self.insert_cost(c);
                // A letter typed that the word doesn't have.
                let extra = next[j - 1] + self.extra_cost(j);
                let mut best = sub.min(left_out).min(extra);
                if i > 1 && j > 1 && c == self.t[j - 2] && path[i - 2] == tj {
                    best = best.min(rows[i - 2][j - 2] + SWAP);
                }
                next[j] = best;
            }
            rows.push(next);
            path.push(c);
            self.visit(kid as usize, rows, path, best_prefix);
            path.pop();
            rows.pop();
        }
    }

    fn sub_cost(&self, word: char, typed: char) -> f32 {
        if word == typed {
            0.0
        } else if self.keys.near(word, typed) {
            SLIP
        } else {
            1.0
        }
    }

    fn insert_cost(&self, c: char) -> f32 {
        if c == '\'' || c == '-' { MARK } else { 1.0 }
    }

    fn extra_cost(&self, j: usize) -> f32 {
        if j > 1 && self.t[j - 1] == self.t[j - 2] { DOUBLED } else { 1.0 }
    }
}

/// The typed word's capitals carried onto a suggestion: "Bonjour" stays
/// capitalised, "OK" stays upper case, and a word that is always
/// capitalised (Haus, I, Paris) keeps its own.
pub fn with_case(typed: &str, word: &str) -> String {
    let letters: Vec<char> = typed.chars().filter(|c| c.is_alphabetic()).collect();
    let all_upper = letters.len() > 1 && letters.iter().all(|c| c.is_uppercase());
    if all_upper {
        return word.to_uppercase();
    }
    if word.chars().any(|c| c.is_uppercase()) {
        return word.to_string();
    }
    if typed.chars().next().is_some_and(|c| c.is_uppercase()) {
        let mut cs = word.chars();
        return match cs.next() {
            Some(f) => f.to_uppercase().chain(cs).collect(),
            None => String::new(),
        };
    }
    word.to_string()
}
