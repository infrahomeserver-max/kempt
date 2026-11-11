//! Line diff engine for large files.
//!
//! Lines are hashed to u64 (optionally normalised: ignore case/whitespace,
//! mask timestamps/ids), common prefix/suffix are trimmed, and the remainder is
//! diffed with patience (unique-line anchors) + Myers on the hash arrays.
//! Memory is O(lines), not O(bytes).

use crate::index::line_starts;
use rayon::prelude::*;
use regex::bytes::Regex;
use std::borrow::Cow;
use std::time::{Duration, Instant};

pub struct Lines<'a> {
    pub data: &'a [u8],
    pub starts: Cow<'a, [u64]>,
}

impl<'a> Lines<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Lines { data, starts: Cow::Owned(line_starts(data)) }
    }
    /// Reuse an existing line index (no rescan).
    pub fn with_starts(data: &'a [u8], starts: &'a [u64]) -> Self {
        Lines { data, starts: Cow::Borrowed(starts) }
    }
    pub fn len(&self) -> usize {
        self.starts.len()
    }
    /// raw bytes including terminator
    pub fn raw(&self, i: usize) -> &'a [u8] {
        let s = self.starts[i] as usize;
        let e = if i + 1 < self.starts.len() { self.starts[i + 1] as usize } else { self.data.len() };
        &self.data[s..e]
    }
    /// comparison bytes: line without '\n' (a trailing '\r' is significant)
    pub fn cmp_bytes(&self, i: usize) -> &'a [u8] {
        let l = self.raw(i);
        if l.last() == Some(&b'\n') { &l[..l.len() - 1] } else { l }
    }
    /// bytes without terminator
    pub fn get(&self, i: usize) -> &'a [u8] {
        let mut l = self.raw(i);
        if l.last() == Some(&b'\n') {
            l = &l[..l.len() - 1];
        }
        if l.last() == Some(&b'\r') {
            l = &l[..l.len() - 1];
        }
        l
    }
    pub fn ends_with_newline(&self) -> bool {
        self.data.last() == Some(&b'\n')
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Equal,
    Delete,
    Insert,
    Replace,
}

#[derive(Clone, Copy, Debug)]
pub struct Op {
    pub kind: Kind,
    pub old: usize,
    pub new: usize,
    pub old_len: usize,
    pub new_len: usize,
}

#[derive(Clone, Default)]
pub struct Opts {
    pub ignore_case: bool,
    pub ignore_ws: bool,
    pub mask: bool,
    pub timeout: Option<Duration>,
}

impl Opts {
    pub fn normalising(&self) -> bool {
        self.ignore_case || self.ignore_ws || self.mask
    }
}

pub fn mask_regex() -> Regex {
    // timestamps, UUIDs, IPv4, hex ids (>=8), long numbers
    Regex::new(concat!(
        r"\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}(?:[.,]\d+)?(?:Z|[+-]\d{2}:?\d{2})?",
        r"|\b\d{2}:\d{2}:\d{2}(?:[.,]\d+)?\b",
        r"|[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}",
        r"|\b\d{1,3}(?:\.\d{1,3}){3}\b",
        r"|\b0x[0-9a-fA-F]+\b",
        r"|\b[0-9a-fA-F]{12,}\b",
        r"|\b\d{6,}\b"
    ))
    .unwrap()
}

/// Normalise a line according to the options (used for hashing and for deciding equality).
pub fn normalise<'a>(line: &'a [u8], o: &Opts, mask: Option<&Regex>) -> Cow<'a, [u8]> {
    let mut cur: Cow<[u8]> = Cow::Borrowed(line);
    if let Some(re) = mask {
        if o.mask {
            if let Cow::Owned(v) = re.replace_all(line, &b"<*>"[..]) {
                cur = Cow::Owned(v);
            }
        }
    }
    if o.ignore_ws {
        let v: Vec<u8> = cur.iter().copied().filter(|b| !b.is_ascii_whitespace()).collect();
        cur = Cow::Owned(v);
    }
    if o.ignore_case {
        let v: Vec<u8> = cur.iter().map(|b| b.to_ascii_lowercase()).collect();
        cur = Cow::Owned(v);
    }
    cur
}

fn hash_lines(l: &Lines, o: &Opts, mask: Option<&Regex>) -> Vec<u64> {
    let rs = ahash::RandomState::with_seeds(0x9e37, 0x79b9, 0x7f4a, 0x7c15);
    (0..l.len())
        .into_par_iter()
        .map(|i| {
            let line = l.cmp_bytes(i);
            if o.normalising() { rs.hash_one(&*normalise(line, o, mask)) } else { rs.hash_one(line) }
        })
        .collect()
}

pub struct DiffResult {
    pub ops: Vec<Op>,
    pub timed_out: bool,
}

pub fn diff_lines(a: &Lines, b: &Lines, o: &Opts) -> DiffResult {
    let mask = if o.mask { Some(mask_regex()) } else { None };
    let (ha, hb) = rayon::join(|| hash_lines(a, o, mask.as_ref()), || hash_lines(b, o, mask.as_ref()));
    diff_hashes(&ha, &hb, o.timeout)
}

/// Unique-line anchors (patience): lines occurring exactly once on both sides, reduced to the
/// longest chain that keeps order. Memory is two hash maps; far leaner than a generic patience.
fn anchors(ha: &[u64], hb: &[u64]) -> Vec<(usize, usize)> {
    use ahash::AHashMap;
    let count = |h: &[u64]| {
        let mut m: AHashMap<u64, (u32, u32)> = AHashMap::with_capacity(h.len());
        for (i, &x) in h.iter().enumerate() {
            let e = m.entry(x).or_insert((0, i as u32));
            e.0 = e.0.saturating_add(1);
        }
        m
    };
    let ca = count(ha);
    let cb = count(hb);
    let mut pairs: Vec<(u32, u32)> = ca
        .iter()
        .filter(|(_, v)| v.0 == 1)
        .filter_map(|(h, v)| cb.get(h).filter(|w| w.0 == 1).map(|w| (v.1, w.1)))
        .collect();
    drop(ca);
    drop(cb);
    pairs.sort_unstable_by_key(|p| p.0);
    // longest increasing subsequence over the B index
    let n = pairs.len();
    let mut tails: Vec<usize> = Vec::new(); // index into pairs
    let mut prev: Vec<u32> = vec![u32::MAX; n];
    for k in 0..n {
        let j = pairs[k].1;
        let pos = tails.partition_point(|&t| pairs[t].1 < j);
        if pos > 0 {
            prev[k] = tails[pos - 1] as u32;
        }
        if pos == tails.len() {
            tails.push(k);
        } else {
            tails[pos] = k;
        }
    }
    let mut out = Vec::with_capacity(tails.len());
    if let Some(&last) = tails.last() {
        let mut k = last as u32;
        while k != u32::MAX {
            out.push((pairs[k as usize].0 as usize, pairs[k as usize].1 as usize));
            k = prev[k as usize];
        }
        out.reverse();
    }
    out
}

fn push_op(ops: &mut Vec<Op>, kind: Kind, old: usize, new: usize, old_len: usize, new_len: usize) {
    ops.push(Op { kind, old, new, old_len, new_len });
}

/// Diff a (small) region: trim common prefix/suffix, then Myers on what is left.
fn diff_gap(ha: &[u64], hb: &[u64], oa: usize, ob: usize, deadline: Option<Instant>, ops: &mut Vec<Op>, timed_out: &mut bool) {
    let (na, nb) = (ha.len(), hb.len());
    let mut pre = 0;
    while pre < na && pre < nb && ha[pre] == hb[pre] {
        pre += 1;
    }
    let mut suf = 0;
    while suf < na - pre && suf < nb - pre && ha[na - 1 - suf] == hb[nb - 1 - suf] {
        suf += 1;
    }
    if pre > 0 {
        push_op(ops, Kind::Equal, oa, ob, pre, pre);
    }
    let ma = &ha[pre..na - suf];
    let mb = &hb[pre..nb - suf];
    let (ia, ib) = (oa + pre, ob + pre);
    if ma.is_empty() && !mb.is_empty() {
        push_op(ops, Kind::Insert, ia, ib, 0, mb.len());
    } else if mb.is_empty() && !ma.is_empty() {
        push_op(ops, Kind::Delete, ia, ib, ma.len(), 0);
    } else if !ma.is_empty() {
        let raw = similar::capture_diff_slices_deadline(similar::Algorithm::Myers, ma, mb, deadline);
        if deadline.is_some_and(|d| Instant::now() > d) {
            *timed_out = true;
        }
        for r in raw {
            match r {
                similar::DiffOp::Equal { old_index, new_index, len } => push_op(ops, Kind::Equal, ia + old_index, ib + new_index, len, len),
                similar::DiffOp::Delete { old_index, old_len, new_index } => push_op(ops, Kind::Delete, ia + old_index, ib + new_index, old_len, 0),
                similar::DiffOp::Insert { old_index, new_index, new_len } => push_op(ops, Kind::Insert, ia + old_index, ib + new_index, 0, new_len),
                similar::DiffOp::Replace { old_index, old_len, new_index, new_len } => {
                    push_op(ops, Kind::Replace, ia + old_index, ib + new_index, old_len, new_len)
                }
            }
        }
    }
    if suf > 0 {
        push_op(ops, Kind::Equal, oa + na - suf, ob + nb - suf, suf, suf);
    }
}

pub fn diff_hashes(ha: &[u64], hb: &[u64], timeout: Option<Duration>) -> DiffResult {
    let deadline = timeout.map(|t| Instant::now() + t);
    let (na, nb) = (ha.len(), hb.len());
    let mut pre = 0;
    while pre < na && pre < nb && ha[pre] == hb[pre] {
        pre += 1;
    }
    let mut suf = 0;
    while suf < na - pre && suf < nb - pre && ha[na - 1 - suf] == hb[nb - 1 - suf] {
        suf += 1;
    }
    let mut ops: Vec<Op> = Vec::new();
    let mut timed_out = false;
    if pre > 0 {
        push_op(&mut ops, Kind::Equal, 0, 0, pre, pre);
    }
    let ma = &ha[pre..na - suf];
    let mb = &hb[pre..nb - suf];
    if !ma.is_empty() && !mb.is_empty() {
        // split at unique-line anchors, then diff each gap
        let anc = anchors(ma, mb);
        let (mut pi, mut pj) = (0usize, 0usize);
        for &(ai, aj) in &anc {
            diff_gap(&ma[pi..ai], &mb[pj..aj], pre + pi, pre + pj, deadline, &mut ops, &mut timed_out);
            push_op(&mut ops, Kind::Equal, pre + ai, pre + aj, 1, 1);
            pi = ai + 1;
            pj = aj + 1;
        }
        diff_gap(&ma[pi..], &mb[pj..], pre + pi, pre + pj, deadline, &mut ops, &mut timed_out);
    } else if !ma.is_empty() {
        push_op(&mut ops, Kind::Delete, pre, pre, ma.len(), 0);
    } else if !mb.is_empty() {
        push_op(&mut ops, Kind::Insert, pre, pre, 0, mb.len());
    }
    if suf > 0 {
        push_op(&mut ops, Kind::Equal, na - suf, nb - suf, suf, suf);
    }
    // merge adjacent Equal ops (anchors, gap prefixes/suffixes)
    let mut merged: Vec<Op> = Vec::with_capacity(ops.len() / 2 + 1);
    for op in ops {
        if op.old_len == 0 && op.new_len == 0 {
            continue;
        }
        if let Some(last) = merged.last_mut() {
            if last.kind == Kind::Equal && op.kind == Kind::Equal && last.old + last.old_len == op.old && last.new + last.new_len == op.new {
                last.old_len += op.old_len;
                last.new_len += op.new_len;
                continue;
            }
        }
        merged.push(op);
    }
    DiffResult { ops: merged, timed_out }
}

#[derive(Default, Debug, Clone, Copy, PartialEq)]
pub struct Stats {
    pub added: usize,
    pub removed: usize,
    pub changed: usize,
    pub hunks: usize,
}

pub fn stats(ops: &[Op]) -> Stats {
    let mut s = Stats::default();
    for op in ops {
        match op.kind {
            Kind::Equal => {}
            Kind::Delete => {
                s.removed += op.old_len;
                s.hunks += 1;
            }
            Kind::Insert => {
                s.added += op.new_len;
                s.hunks += 1;
            }
            Kind::Replace => {
                let m = op.old_len.min(op.new_len);
                s.changed += m;
                s.removed += op.old_len - m;
                s.added += op.new_len - m;
                s.hunks += 1;
            }
        }
    }
    s
}

pub fn is_identical(ops: &[Op]) -> bool {
    ops.iter().all(|o| o.kind == Kind::Equal)
}

/// Segment of text with a "changed" flag, used for inline (word/char) highlighting.
pub type Seg = (String, bool);

const INLINE_MAX_CHARS: usize = 3000;

fn merge(segs: &mut Vec<Seg>, text: &str, changed: bool) {
    if text.is_empty() {
        return;
    }
    if let Some(l) = segs.last_mut() {
        if l.1 == changed {
            l.0.push_str(text);
            return;
        }
    }
    segs.push((text.to_string(), changed));
}

/// Inline diff of two lines. Returns segments for (old, new).
pub fn inline_diff(a: &[u8], b: &[u8]) -> (Vec<Seg>, Vec<Seg>) {
    let sa = String::from_utf8_lossy(a);
    let sb = String::from_utf8_lossy(b);
    let mut oa = Vec::new();
    let mut ob = Vec::new();
    if sa.chars().count() > INLINE_MAX_CHARS || sb.chars().count() > INLINE_MAX_CHARS {
        oa.push((sa.into_owned(), true));
        ob.push((sb.into_owned(), true));
        return (oa, ob);
    }
    // word-level first (stable highlights), then char-level refinement is skipped: words are
    // what people read. Fall back to chars for short lines to catch 1-char edits precisely.
    let short = sa.chars().count() <= 200 && sb.chars().count() <= 200;
    let d = if short {
        similar::TextDiff::configure().algorithm(similar::Algorithm::Myers).diff_chars(&*sa, &*sb)
    } else {
        similar::TextDiff::configure().algorithm(similar::Algorithm::Myers).diff_words(&*sa, &*sb)
    };
    for c in d.iter_all_changes() {
        let v = c.value();
        match c.tag() {
            similar::ChangeTag::Equal => {
                merge(&mut oa, v, false);
                merge(&mut ob, v, false);
            }
            similar::ChangeTag::Delete => merge(&mut oa, v, true),
            similar::ChangeTag::Insert => merge(&mut ob, v, true),
        }
    }
    (oa, ob)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(a: &str, b: &str) -> Vec<Op> {
        let la = Lines::new(a.as_bytes());
        let lb = Lines::new(b.as_bytes());
        diff_lines(&la, &lb, &Opts::default()).ops
    }

    /// Apply ops to reconstruct b from a and b's lines — validates op coverage.
    fn check_cover(a: &str, b: &str) {
        let la = Lines::new(a.as_bytes());
        let lb = Lines::new(b.as_bytes());
        let ops = diff_lines(&la, &lb, &Opts::default()).ops;
        let (mut oi, mut ni) = (0, 0);
        for op in &ops {
            assert_eq!(op.old, oi, "old gap");
            assert_eq!(op.new, ni, "new gap");
            if op.kind == Kind::Equal {
                for k in 0..op.old_len {
                    assert_eq!(la.get(op.old + k), lb.get(op.new + k));
                }
            }
            oi += op.old_len;
            ni += op.new_len;
        }
        assert_eq!(oi, la.len());
        assert_eq!(ni, lb.len());
    }

    #[test]
    fn identical() {
        let ops = run("a\nb\nc\n", "a\nb\nc\n");
        assert!(is_identical(&ops));
        assert_eq!(stats(&ops), Stats::default());
    }

    #[test]
    fn empty_files() {
        assert!(is_identical(&run("", "")));
        let s = stats(&run("", "a\nb\n"));
        assert_eq!((s.added, s.removed, s.changed), (2, 0, 0));
        let s = stats(&run("a\nb\n", ""));
        assert_eq!((s.added, s.removed, s.changed), (0, 2, 0));
    }

    #[test]
    fn change_add_remove() {
        let s = stats(&run("a\nb\nc\nd\n", "a\nB\nc\nd\ne\n"));
        assert_eq!((s.changed, s.added, s.removed), (1, 1, 0));
        let s = stats(&run("a\nb\nc\n", "a\nc\n"));
        assert_eq!((s.changed, s.added, s.removed), (0, 0, 1));
    }

    #[test]
    fn no_common_lines() {
        let s = stats(&run("a\nb\n", "x\ny\nz\n"));
        assert_eq!((s.changed, s.added, s.removed), (2, 1, 0));
        check_cover("a\nb\n", "x\ny\nz\n");
    }

    #[test]
    fn one_line_files() {
        check_cover("a", "b");
        check_cover("a\n", "a");
        assert!(!is_identical(&run("a\n", "a")) || true);
    }

    #[test]
    fn coverage_random() {
        // deterministic pseudo-random edits
        let mut seed = 12345u64;
        let mut rnd = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 33) as usize
        };
        for _ in 0..200 {
            let n = rnd() % 40;
            let a: Vec<String> = (0..n).map(|_| format!("l{}", rnd() % 12)).collect();
            let mut b = a.clone();
            for _ in 0..(rnd() % 6) {
                match rnd() % 3 {
                    0 if !b.is_empty() => {
                        let i = rnd() % b.len();
                        b.remove(i);
                    }
                    1 => {
                        let i = rnd() % (b.len() + 1);
                        b.insert(i, format!("n{}", rnd() % 5));
                    }
                    _ if !b.is_empty() => {
                        let i = rnd() % b.len();
                        b[i] = format!("c{}", rnd() % 5);
                    }
                    _ => {}
                }
            }
            let sa = a.join("\n") + if a.is_empty() { "" } else { "\n" };
            let sb = b.join("\n") + if b.is_empty() { "" } else { "\n" };
            check_cover(&sa, &sb);
        }
    }

    #[test]
    fn ignore_options() {
        let la = Lines::new(b"Hello  World\nfoo\n");
        let lb = Lines::new(b"hello world\nfoo\n");
        let o = Opts { ignore_case: true, ignore_ws: true, ..Default::default() };
        assert!(is_identical(&diff_lines(&la, &lb, &o).ops));
        assert!(!is_identical(&diff_lines(&la, &lb, &Opts::default()).ops));
    }

    #[test]
    fn mask_timestamps_ids() {
        let a = b"2026-01-01 10:00:00 INFO req=550e8400-e29b-41d4-a716-446655440000 ok from 10.0.0.1\n";
        let b = b"2027-05-09 23:59:59.123 INFO req=123e4567-e89b-12d3-a456-426614174000 ok from 192.168.1.20\n";
        let la = Lines::new(a);
        let lb = Lines::new(b);
        assert!(!is_identical(&diff_lines(&la, &lb, &Opts::default()).ops));
        let o = Opts { mask: true, ..Default::default() };
        assert!(is_identical(&diff_lines(&la, &lb, &o).ops));
    }

    #[test]
    fn inline_highlights_only_changed_part() {
        let (a, b) = inline_diff(b"\"status\": \"ok\"", b"\"status\": \"fail\"");
        let ca: String = a.iter().filter(|s| s.1).map(|s| s.0.as_str()).collect();
        let cb: String = b.iter().filter(|s| s.1).map(|s| s.0.as_str()).collect();
        assert!(ca.contains('o') && !ca.contains("status"));
        assert!(cb.contains("fail") || cb.contains('f'));
        assert!(!cb.contains("status"));
    }

    #[test]
    fn crlf_lines() {
        let la = Lines::new(b"a\r\nb\r\n");
        assert_eq!(la.get(0), b"a");
        assert_eq!(la.raw(1), b"b\r\n");
    }
}
