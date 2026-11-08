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
    let mut base = ' ';
    for c in s.nfd() {
        // Accents off Latin, Greek and Cyrillic letters, short vowels off
        // Arabic; but Devanagari and Thai vowel signs are letters there.
        if is_combining_mark(c) && (accented_script(base) || ('\u{064B}'..='\u{065F}').contains(&c) || c == '\u{0670}') {
            continue;
        }
        if !is_combining_mark(c) {
            base = c;
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

/// Scripts whose combining marks are accents (dropped when folding).
fn accented_script(c: char) -> bool {
    c.is_ascii() || ('\u{00C0}'..='\u{024F}').contains(&c) || ('\u{0370}'..='\u{03FF}').contains(&c) || ('\u{0400}'..='\u{04FF}').contains(&c) || ('\u{1E00}'..='\u{1EFF}').contains(&c)
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub word: Box<str>,
    /// Letters in the folded spelling (for telling completions apart).
    pub folded_len: u16,
    /// ln of the word's relative frequency (0 for the commonest word).
    pub logf: f32,
}

/// A trie node while building (and for words added later).
#[derive(Default)]
struct Node {
    kids: Vec<(char, u32)>,
    ends: Vec<u32>,
    /// Highest `logf` of any word at or under this node.
    best: f32,
}

/// The read-only part of a trie, as the search needs it.
trait Trie {
    fn kids(&self, n: usize) -> &[(char, u32)];
    fn ends(&self, n: usize) -> &[u32];
    fn best(&self, n: usize) -> f32;
    fn has_root(&self) -> bool;
}

/// Built nodes: a growable trie (words added after loading).
#[derive(Default)]
struct Loose {
    nodes: Vec<Node>,
}

impl Trie for Loose {
    fn kids(&self, n: usize) -> &[(char, u32)] {
        &self.nodes[n].kids
    }
    fn ends(&self, n: usize) -> &[u32] {
        &self.nodes[n].ends
    }
    fn best(&self, n: usize) -> f32 {
        self.nodes[n].best
    }
    fn has_root(&self) -> bool {
        !self.nodes.is_empty()
    }
}

impl Loose {
    fn add(&mut self, folded: &str, id: u32, logf: f32) {
        if self.nodes.is_empty() {
            self.nodes.push(Node { best: f32::MIN, ..Default::default() });
        }
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
}

/// The loaded word list, packed into flat arrays (a few bytes a node
/// instead of two small lists each): a keyboard keeps many languages.
#[derive(Default)]
struct Packed {
    nodes: Vec<PNode>,
    kids: Vec<(char, u32)>,
    ends: Vec<u32>,
}

#[derive(Clone, Copy)]
struct PNode {
    kid_start: u32,
    kid_len: u32,
    end_start: u32,
    end_len: u32,
    best: f32,
}

impl Trie for Packed {
    fn kids(&self, n: usize) -> &[(char, u32)] {
        let p = self.nodes[n];
        &self.kids[p.kid_start as usize..(p.kid_start + p.kid_len) as usize]
    }
    fn ends(&self, n: usize) -> &[u32] {
        let p = self.nodes[n];
        &self.ends[p.end_start as usize..(p.end_start + p.end_len) as usize]
    }
    fn best(&self, n: usize) -> f32 {
        self.nodes[n].best
    }
    fn has_root(&self) -> bool {
        !self.nodes.is_empty()
    }
}

impl Packed {
    fn from(l: Loose) -> Self {
        let mut p = Packed { nodes: Vec::with_capacity(l.nodes.len()), kids: Vec::new(), ends: Vec::new() };
        let total_kids: usize = l.nodes.iter().map(|n| n.kids.len()).sum();
        let total_ends: usize = l.nodes.iter().map(|n| n.ends.len()).sum();
        p.kids.reserve_exact(total_kids);
        p.ends.reserve_exact(total_ends);
        for n in &l.nodes {
            p.nodes.push(PNode {
                kid_start: p.kids.len() as u32,
                kid_len: n.kids.len() as u32,
                end_start: p.ends.len() as u32,
                end_len: n.ends.len() as u32,
                best: n.best,
            });
            p.kids.extend_from_slice(&n.kids);
            p.ends.extend_from_slice(&n.ends);
        }
        p
    }
}

fn walk_to<T: Trie>(t: &T, folded: &str) -> Option<usize> {
    if !t.has_root() {
        return None;
    }
    let mut n = 0usize;
    for c in folded.chars() {
        n = t.kids(n).iter().find(|(k, _)| *k == c)?.1 as usize;
    }
    Some(n)
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
    packed: Packed,
    extra: Loose,
}

impl Lexicon {
    /// Words, commonest first (one per line; `#` lines are comments).
    /// A line may carry its own rank after a tab, for words the list
    /// lacks (French contractions).
    pub fn from_ranked(text: &str) -> Self {
        let mut lex = Lexicon::default();
        let mut build = Loose::default();
        let mut seen: HashMap<Box<str>, u32> = HashMap::new();
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
            let logf = -(r.max(1) as f32).ln();
            if let Some(&id) = seen.get(word) {
                let e = &mut lex.entries[id as usize];
                e.logf = e.logf.max(logf);
                continue;
            }
            let folded = fold(word);
            if folded.is_empty() {
                continue;
            }
            let id = lex.entries.len() as u32;
            lex.entries.push(Entry { word: word.into(), folded_len: folded.chars().count() as u16, logf });
            seen.insert(word.into(), id);
            build.add(&folded, id, logf);
        }
        lex.entries.shrink_to_fit();
        lex.packed = Packed::from(build);
        lex
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    fn ids_folded(&self, folded: &str) -> Vec<u32> {
        let mut v: Vec<u32> = walk_to(&self.packed, folded).map(|n| self.packed.ends(n).to_vec()).unwrap_or_default();
        if let Some(n) = walk_to(&self.extra, folded) {
            v.extend_from_slice(self.extra.ends(n));
        }
        v
    }

    /// A word (the owner's own): added, or made commoner if it's there.
    pub fn insert(&mut self, word: &str, logf: f32) {
        let folded = fold(word);
        if folded.is_empty() {
            return;
        }
        if let Some(id) = self.ids_folded(&folded).into_iter().find(|&id| &*self.entries[id as usize].word == word) {
            let e = &mut self.entries[id as usize];
            if logf > e.logf {
                e.logf = logf;
                // Its subtree's best may now be low in the packed trie: also
                // index it in the small one, where completions find it.
                self.extra.add(&folded, id, logf);
            }
            return;
        }
        let id = self.entries.len() as u32;
        self.entries.push(Entry { word: word.into(), folded_len: folded.chars().count() as u16, logf });
        self.extra.add(&folded, id, logf);
    }

    /// Entries spelled like `word` once folded ("deja" → déjà).
    pub fn same_folded(&self, word: &str) -> impl Iterator<Item = &Entry> {
        let mut ids = self.ids_folded(&fold(word));
        ids.sort_unstable();
        ids.dedup();
        ids.into_iter().map(|id| &self.entries[id as usize])
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
        for trie in [&self.packed as &dyn TrieDyn, &self.extra as &dyn TrieDyn] {
            if !trie.root() {
                continue;
            }
            let first: Vec<f32> = (0..=n).map(|j| j as f32).collect();
            let mut rows: Vec<Vec<f32>> = vec![first];
            let mut path: Vec<char> = Vec::new();
            let mut walk = Walk { lex: self, trie, t: &t, max, comp_max, completions, keys, found: &mut found };
            walk.visit(0, &mut rows, &mut path, f32::INFINITY);
        }
        let mut out: Vec<Cand> = found.into_values().collect();
        out.sort_by(|a, b| a.cost.partial_cmp(&b.cost).unwrap_or(Ordering::Equal));
        out
    }

    /// The `k` commonest words under node `n` of `trie`.
    fn top_under(&self, trie: &dyn TrieDyn, n: usize, k: usize) -> Vec<u32> {
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
        heap.push(Item(trie.best_of(n), n));
        let mut ends: Vec<u32> = Vec::new();
        while let Some(Item(best, i)) = heap.pop() {
            if ends.len() >= k {
                let worst = ends.iter().map(|&id| self.entries[id as usize].logf).fold(f32::MAX, f32::min);
                if best <= worst {
                    break;
                }
            }
            for &id in trie.ends_of(i) {
                ends.push(id);
            }
            if ends.len() > k {
                ends.sort_by(|a, b| self.entries[*b as usize].logf.partial_cmp(&self.entries[*a as usize].logf).unwrap_or(Ordering::Equal));
                ends.truncate(k);
            }
            for &(_, kid) in trie.kids_of(i) {
                heap.push(Item(trie.best_of(kid as usize), kid as usize));
            }
        }
        ends
    }
}

/// The trie methods the search uses, object-safe.
trait TrieDyn {
    fn kids_of(&self, n: usize) -> &[(char, u32)];
    fn ends_of(&self, n: usize) -> &[u32];
    fn best_of(&self, n: usize) -> f32;
    fn root(&self) -> bool;
}

impl<T: Trie> TrieDyn for T {
    fn kids_of(&self, n: usize) -> &[(char, u32)] {
        self.kids(n)
    }
    fn ends_of(&self, n: usize) -> &[u32] {
        self.ends(n)
    }
    fn best_of(&self, n: usize) -> f32 {
        self.best(n)
    }
    fn root(&self) -> bool {
        self.has_root()
    }
}

struct Walk<'a> {
    lex: &'a Lexicon,
    trie: &'a dyn TrieDyn,
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
            for &id in self.trie.ends_of(node) {
                self.keep(Cand { id, cost: whole, completion: false });
            }
        }
        // Everything typed reads as the start of this path: offer the
        // commonest words under it, once, at the cheapest such point.
        let mut best_prefix = best_prefix;
        if self.completions > 0 && whole <= self.comp_max && whole < best_prefix && !path.is_empty() {
            best_prefix = whole;
            for id in self.lex.top_under(self.trie, node, self.completions) {
                if self.lex.entries[id as usize].folded_len as usize > n {
                    self.keep(Cand { id, cost: whole, completion: true });
                }
            }
        }
        if row.iter().cloned().fold(f32::INFINITY, f32::min) > self.max {
            return;
        }
        let trie = self.trie;
        for &(c, kid) in trie.kids_of(node) {
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
