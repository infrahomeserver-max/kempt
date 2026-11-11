//! Find engine: multiple keywords with AND / OR, exclusions, case / whole-word / regex,
//! per-term highlight ranges, per-line replace, and background scan jobs.
//!
//! Query syntax (non-regex mode):
//!   error timeout          two terms (combined with the chosen mode: Any = OR, All = AND)
//!   "connection reset"     a phrase containing spaces
//!   -debug                 exclude lines containing "debug"
//! Regex mode: the whole query is one regular expression.

use anyhow::{Result, bail};
use regex::bytes::{Regex, RegexBuilder};
use serde_json::Value;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub const MAX_TERMS: usize = 24;
pub const MAX_MATCH_LINES: usize = 20_000_000;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Mode {
    Or,
    And,
}

pub struct Matcher {
    includes: Vec<Regex>,
    excludes: Vec<Regex>,
    mode: Mode,
    /// alternation of all include terms (used for replace)
    any: Option<Regex>,
    #[allow(dead_code)]
    pub terms: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Range {
    pub start: usize,
    pub end: usize,
    pub term: usize,
}

/// Split `q` into terms. Whitespace separates terms; double quotes group a phrase (`"connection timeout"`);
/// a leading `-` marks an exclusion. Escapes: `\"` is a literal quote, and a leading `\-` is a literal dash
/// (so `-1` excludes "1" but `\-1` searches for "-1"). Any other backslash is just a backslash.
pub fn tokenize(q: &str) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_q = false;
    let mut literal = false; // the term had quotes or an escaped dash: never an exclusion
    let flush = |cur: &mut String, literal: &mut bool, out: &mut Vec<(String, bool)>| {
        if cur.is_empty() && !*literal {
            return;
        }
        let t = std::mem::take(cur);
        let lit = *literal;
        *literal = false;
        if !lit && t.len() > 1 && t.starts_with('-') {
            out.push((t[1..].to_string(), true));
        } else if !t.is_empty() {
            out.push((t, false));
        }
    };
    let chars: Vec<char> = q.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        match ch {
            '\\' if chars.get(i + 1) == Some(&'"') => {
                cur.push('"');
                literal = true;
                i += 1;
            }
            '\\' if chars.get(i + 1) == Some(&'-') && cur.is_empty() && !in_q => {
                cur.push('-');
                literal = true;
                i += 1;
            }
            '"' => {
                in_q = !in_q;
                literal = true;
            }
            c if c.is_whitespace() && !in_q => flush(&mut cur, &mut literal, &mut out),
            c => cur.push(c),
        }
        i += 1;
    }
    flush(&mut cur, &mut literal, &mut out);
    out
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn build(pattern: &str, case: bool) -> Result<Regex> {
    Ok(RegexBuilder::new(pattern).case_insensitive(!case).size_limit(32 << 20).build().map_err(|e| {
        let m = e.to_string();
        anyhow::anyhow!("invalid pattern: {}", m.lines().last().unwrap_or("").trim())
    })?)
}

fn literal_pattern(term: &str, word: bool) -> String {
    let mut p = regex::escape(term);
    if word {
        if term.chars().next().is_some_and(is_word) {
            p = format!(r"\b{p}");
        }
        if term.chars().last().is_some_and(is_word) {
            p = format!(r"{p}\b");
        }
    }
    p
}

impl Matcher {
    /// spec: {"q": "...", "case": bool, "word": bool, "regex": bool, "mode": "or"|"and"}
    pub fn parse(spec: &Value) -> Result<Matcher> {
        let q = spec.get("q").and_then(|v| v.as_str()).unwrap_or("");
        let case = spec.get("case").and_then(|v| v.as_bool()).unwrap_or(false);
        let word = spec.get("word").and_then(|v| v.as_bool()).unwrap_or(false);
        let regex = spec.get("regex").and_then(|v| v.as_bool()).unwrap_or(false);
        let mode = match spec.get("mode").and_then(|v| v.as_str()) {
            Some("and") | Some("all") => Mode::And,
            _ => Mode::Or,
        };
        let phrase = spec.get("phrase").and_then(|v| v.as_bool()).unwrap_or(false);
        Self::new_ex(q, case, word, regex, mode, phrase)
    }

    pub fn new(q: &str, case: bool, word: bool, regex: bool, mode: Mode) -> Result<Matcher> {
        Self::new_ex(q, case, word, regex, mode, false)
    }

    /// `phrase`: everything typed is ONE literal item (spaces, quotes and dashes included); nothing is split.
    pub fn new_ex(q: &str, case: bool, word: bool, regex: bool, mode: Mode, phrase: bool) -> Result<Matcher> {
        if q.trim().is_empty() {
            return Ok(Matcher { includes: vec![], excludes: vec![], mode, any: None, terms: vec![] });
        }
        if regex {
            let p = if word { format!(r"\b(?:{q})\b") } else { q.to_string() };
            let re = build(&p, case)?;
            return Ok(Matcher { any: Some(re.clone()), includes: vec![re], excludes: vec![], mode: Mode::Or, terms: vec![q.to_string()] });
        }
        let toks = if phrase { vec![(q.to_string(), false)] } else { tokenize(q) };
        if toks.len() > MAX_TERMS {
            bail!("too many search terms (max {MAX_TERMS})");
        }
        let mut includes = Vec::new();
        let mut excludes = Vec::new();
        let mut terms = Vec::new();
        for (t, neg) in &toks {
            let re = build(&literal_pattern(t, word), case)?;
            if *neg {
                excludes.push(re);
            } else {
                includes.push(re);
                terms.push(t.clone());
            }
        }
        let any = if includes.is_empty() {
            None
        } else {
            let alt: Vec<String> = terms.iter().map(|t| literal_pattern(t, word)).collect();
            Some(build(&alt.join("|"), case)?)
        };
        Ok(Matcher { includes, excludes, mode, any, terms })
    }

    /// no positive and no negative terms: nothing to search for
    pub fn is_empty(&self) -> bool {
        self.includes.is_empty() && self.excludes.is_empty()
    }

    pub fn line_matches(&self, line: &[u8]) -> bool {
        if self.is_empty() {
            return false;
        }
        if self.excludes.iter().any(|r| r.is_match(line)) {
            return false;
        }
        if self.includes.is_empty() {
            return true; // only exclusions: every line that survives them
        }
        match self.mode {
            Mode::Or => self.includes.iter().any(|r| r.is_match(line)),
            Mode::And => self.includes.iter().all(|r| r.is_match(line)),
        }
    }

    /// Highlight ranges for a line (byte offsets into `line`), non-overlapping, sorted.
    /// Empty if the line does not satisfy the query.
    pub fn ranges(&self, line: &[u8]) -> Vec<Range> {
        if self.includes.is_empty() || !self.line_matches(line) {
            return vec![];
        }
        let mut all: Vec<Range> = Vec::new();
        for (ti, re) in self.includes.iter().enumerate() {
            for m in re.find_iter(line).take(500) {
                if m.end() > m.start() {
                    all.push(Range { start: m.start(), end: m.end(), term: ti });
                }
            }
        }
        all.sort_by_key(|r| (r.start, std::cmp::Reverse(r.end)));
        let mut out: Vec<Range> = Vec::with_capacity(all.len());
        let mut last_end = 0;
        for r in all {
            if r.start >= last_end {
                last_end = r.end;
                out.push(r);
            }
        }
        out
    }

    /// Replace every term occurrence on a line that satisfies the query.
    /// `literal`: treat `rep` as plain text (no `$1` expansion). Returns None if nothing changed.
    pub fn replace_line(&self, line: &[u8], rep: &str, literal: bool) -> Option<Vec<u8>> {
        let any = self.any.as_ref()?;
        if !self.line_matches(line) {
            return None;
        }
        let out = if literal { any.replace_all(line, regex::bytes::NoExpand(rep.as_bytes())) } else { any.replace_all(line, rep.as_bytes()) };
        match out {
            std::borrow::Cow::Borrowed(_) => None,
            std::borrow::Cow::Owned(v) => {
                if v == line { None } else { Some(v) }
            }
        }
    }

    pub fn count_occurrences(&self, line: &[u8]) -> usize {
        match &self.any {
            Some(a) if self.line_matches(line) => a.find_iter(line).filter(|m| m.end() > m.start()).count(),
            _ => 0,
        }
    }
}

/// UTF-16 offset of byte offset `b` in `s` (what JavaScript string indices use).
pub fn utf16_offset(s: &str, b: usize) -> usize {
    s[..b.min(s.len())].encode_utf16().count()
}

// ───────────────────────────── background scan job ─────────────────────────────

pub struct Job {
    pub matches: Mutex<Vec<u32>>,
    pub scanned_lines: AtomicU64,
    pub total_lines: u64,
    pub done: AtomicBool,
    pub cancel: AtomicBool,
    pub truncated: AtomicBool,
    pub occurrences: AtomicU64,
}

impl Job {
    pub fn new(total_lines: u64) -> Job {
        Job {
            matches: Mutex::new(Vec::new()),
            scanned_lines: AtomicU64::new(0),
            total_lines,
            done: AtomicBool::new(false),
            cancel: AtomicBool::new(false),
            truncated: AtomicBool::new(false),
            occurrences: AtomicU64::new(0),
        }
    }

    pub fn count(&self) -> usize {
        self.matches.lock().unwrap().len()
    }

    /// next/previous matching line relative to `from` (0-based). Returns
    /// (line, index_in_matches, wrapped) or Err(pending) if more scanning is needed to decide.
    pub fn step(&self, from: i64, dir: i32) -> StepResult {
        let m = self.matches.lock().unwrap();
        let done = self.done.load(Ordering::Acquire);
        if m.is_empty() {
            return if done { StepResult::None } else { StepResult::Pending };
        }
        if dir >= 0 {
            let idx = m.partition_point(|&l| (l as i64) <= from);
            if idx < m.len() {
                StepResult::Found { line: m[idx] as usize, index: idx, wrapped: false }
            } else if done {
                StepResult::Found { line: m[0] as usize, index: 0, wrapped: true }
            } else {
                StepResult::Pending
            }
        } else {
            let idx = m.partition_point(|&l| (l as i64) < from);
            if idx > 0 {
                StepResult::Found { line: m[idx - 1] as usize, index: idx - 1, wrapped: false }
            } else if done {
                let last = m.len() - 1;
                StepResult::Found { line: m[last] as usize, index: last, wrapped: true }
            } else {
                StepResult::Pending
            }
        }
    }

    /// number of matching lines at or before `line`
    pub fn rank(&self, line: i64) -> usize {
        let m = self.matches.lock().unwrap();
        m.partition_point(|&l| (l as i64) <= line)
    }

    pub fn ruler(&self, buckets: usize) -> Vec<u32> {
        let m = self.matches.lock().unwrap();
        let mut v = vec![0u32; buckets];
        let total = self.total_lines.max(1);
        for &l in m.iter() {
            let b = ((l as u64 * buckets as u64) / total).min(buckets as u64 - 1) as usize;
            v[b] = v[b].saturating_add(1);
        }
        v
    }
}

#[derive(Debug, PartialEq)]
pub enum StepResult {
    Found { line: usize, index: usize, wrapped: bool },
    Pending,
    None,
}

/// Scan `data` (lines separated by '\n') in the calling thread, publishing progress.
pub fn run_scan(job: &Job, data: &[u8], m: &Matcher) {
    let mut pos = 0usize;
    let mut line = 0u32;
    let mut batch: Vec<u32> = Vec::with_capacity(4096);
    let mut occ = 0u64;
    let publish = |batch: &mut Vec<u32>, job: &Job| {
        if !batch.is_empty() {
            job.matches.lock().unwrap().extend_from_slice(batch);
            batch.clear();
        }
    };
    let mut total_matches = 0usize;
    while pos < data.len() {
        if job.cancel.load(Ordering::Relaxed) {
            break;
        }
        let end = memchr::memchr(b'\n', &data[pos..]).map(|p| pos + p).unwrap_or(data.len());
        let mut e = end;
        if e > pos && data[e - 1] == b'\r' {
            e -= 1;
        }
        let l = &data[pos..e];
        if m.line_matches(l) {
            if total_matches < MAX_MATCH_LINES {
                batch.push(line);
                total_matches += 1;
                occ += m.count_occurrences(l).max(1) as u64;
            } else {
                job.truncated.store(true, Ordering::Relaxed);
            }
            if batch.len() >= 4096 {
                publish(&mut batch, job);
                job.occurrences.store(occ, Ordering::Relaxed);
            }
        }
        line += 1;
        if line % 65536 == 0 {
            job.scanned_lines.store(line as u64, Ordering::Relaxed);
        }
        pos = end + 1;
    }
    publish(&mut batch, job);
    job.occurrences.store(occ, Ordering::Relaxed);
    job.scanned_lines.store(line as u64, Ordering::Relaxed);
    job.done.store(true, Ordering::Release);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn m(q: &str, mode: &str) -> Matcher {
        Matcher::parse(&json!({"q": q, "mode": mode})).unwrap()
    }

    #[test]
    fn tokenizer() {
        assert_eq!(tokenize("a b"), vec![("a".into(), false), ("b".into(), false)]);
        assert_eq!(tokenize(r#"foo "bar baz" -x"#), vec![("foo".into(), false), ("bar baz".into(), false), ("x".into(), true)]);
        assert_eq!(tokenize("-"), vec![("-".into(), false)]);
        assert_eq!(tokenize(r#""-quoted""#), vec![("-quoted".into(), false)]);
        // escapes: a literal quote, a literal leading dash, backslashes left alone
        assert_eq!(tokenize(r#"\"status\": ok"#), vec![("\"status\":".into(), false), ("ok".into(), false)]);
        assert_eq!(tokenize(r"\-1 -2"), vec![("-1".into(), false), ("2".into(), true)]);
        assert_eq!(tokenize(r"C:\temp\x a-b"), vec![(r"C:\temp\x".into(), false), ("a-b".into(), false)]);
        assert_eq!(tokenize(r#""say \"hi\" now" x"#), vec![("say \"hi\" now".into(), false), ("x".into(), false)]);
        assert!(tokenize("   ").is_empty());
    }

    #[test]
    fn single_phrase_mode() {
        let p = Matcher::parse(&json!({"q": r#"error -x "q" a  b"#, "phrase": true})).unwrap();
        assert!(p.line_matches(br#"an error -x "q" a  b here"#));
        assert!(!p.line_matches(b"error"));
        assert!(!p.line_matches(br#"error -x "q" a b"#)); // the double space counts
        let esc = Matcher::parse(&json!({"q": r#"\"status\": "ok""#})).unwrap();
        assert!(esc.line_matches(br#"{"status": "ok"}"#) || esc.line_matches(b"\"status\":"));
    }

    #[test]
    fn or_and_modes() {
        let o = m("error timeout", "or");
        assert!(o.line_matches(b"an ERROR happened"));
        assert!(o.line_matches(b"a timeout"));
        assert!(!o.line_matches(b"all fine"));
        let a = m("error timeout", "and");
        assert!(!a.line_matches(b"an error happened"));
        assert!(a.line_matches(b"error: connection timeout"));
    }

    #[test]
    fn exclusion_and_phrase() {
        let x = m(r#""connection reset" -debug"#, "or");
        assert!(x.line_matches(b"WARN connection reset by peer"));
        assert!(!x.line_matches(b"DEBUG connection reset by peer"));
        assert!(!x.line_matches(b"connection  reset")); // phrase is literal
        let only_neg = m("-debug", "or");
        assert!(only_neg.line_matches(b"INFO ok"));
        assert!(!only_neg.line_matches(b"debug stuff"));
    }

    #[test]
    fn case_word_regex_flags() {
        let c = Matcher::parse(&json!({"q": "Error", "case": true})).unwrap();
        assert!(c.line_matches(b"Error"));
        assert!(!c.line_matches(b"error"));
        let w = Matcher::parse(&json!({"q": "err", "word": true})).unwrap();
        assert!(w.line_matches(b"an err here"));
        assert!(!w.line_matches(b"error"));
        let r = Matcher::parse(&json!({"q": r"req=\d+", "regex": true})).unwrap();
        assert!(r.line_matches(b"x req=123 y"));
        assert!(!r.line_matches(b"req=abc"));
        assert!(Matcher::parse(&json!({"q": "(", "regex": true})).is_err());
        // special characters are literal outside regex mode
        let lit = Matcher::parse(&json!({"q": "a.b*c"})).unwrap();
        assert!(lit.line_matches(b"xa.b*cx"));
        assert!(!lit.line_matches(b"aXbbc"));
        // whole-word with non-word edge chars
        let e = Matcher::parse(&json!({"q": "[x]", "word": true})).unwrap();
        assert!(e.line_matches(b"a [x] b"));
    }

    #[test]
    fn empty_query_matches_nothing() {
        assert!(m("", "or").is_empty());
        assert!(!m("", "or").line_matches(b"anything"));
    }

    #[test]
    fn ranges_per_term_nonoverlapping() {
        let x = m("ab bc", "or");
        let r = x.ranges(b"xxabcxx ab");
        // "ab" at 2..4 wins; "bc" at 3..5 overlaps and is dropped; second "ab" at 8..10
        assert_eq!(r, vec![Range { start: 2, end: 4, term: 0 }, Range { start: 8, end: 10, term: 0 }]);
        let y = m("foo bar", "and");
        assert!(y.ranges(b"only foo").is_empty()); // AND not satisfied -> no highlights
        let z = y.ranges(b"bar then foo");
        assert_eq!(z.len(), 2);
        assert_eq!(z[0].term, 1);
        assert_eq!(z[1].term, 0);
    }

    #[test]
    fn replace_variants() {
        let x = m("foo bar", "or");
        assert_eq!(x.replace_line(b"foo and bar and foo", "X", true).unwrap(), b"X and X and X");
        assert!(x.replace_line(b"nothing", "X", true).is_none());
        let a = m("foo bar", "and");
        assert!(a.replace_line(b"foo only", "X", true).is_none());
        assert_eq!(a.replace_line(b"foo bar", "X", true).unwrap(), b"X X");
        let r = Matcher::parse(&json!({"q": r"(\w+)@(\w+)", "regex": true})).unwrap();
        assert_eq!(r.replace_line(b"me@home", "$2:$1", false).unwrap(), b"home:me");
        assert_eq!(r.replace_line(b"me@home", "$2:$1", true).unwrap(), b"$2:$1");
        // replacing with the same text is "no change"
        assert!(x.replace_line(b"foo", "foo", true).is_none());
    }

    #[test]
    fn utf16_offsets() {
        let s = "a🚀bé";
        assert_eq!(utf16_offset(s, 0), 0);
        assert_eq!(utf16_offset(s, 1), 1);
        assert_eq!(utf16_offset(s, 5), 3); // after the rocket (2 UTF-16 units)
    }

    #[test]
    fn scan_job_and_navigation() {
        let data = b"a\nERROR 1\nb\nERROR 2\nc\nerror 3\n";
        let mm = m("error", "or");
        let job = Job::new(6);
        run_scan(&job, data, &mm);
        assert!(job.done.load(Ordering::Acquire));
        assert_eq!(*job.matches.lock().unwrap(), vec![1, 3, 5]);
        assert_eq!(job.occurrences.load(Ordering::Relaxed), 3);
        assert_eq!(job.step(-1, 1), StepResult::Found { line: 1, index: 0, wrapped: false });
        assert_eq!(job.step(1, 1), StepResult::Found { line: 3, index: 1, wrapped: false });
        assert_eq!(job.step(5, 1), StepResult::Found { line: 1, index: 0, wrapped: true });
        assert_eq!(job.step(3, -1), StepResult::Found { line: 1, index: 0, wrapped: false });
        assert_eq!(job.step(1, -1), StepResult::Found { line: 5, index: 2, wrapped: true });
        assert_eq!(job.rank(3), 2);
        assert_eq!(job.ruler(3).iter().sum::<u32>(), 3);
    }

    #[test]
    fn scan_cancel_and_crlf() {
        let data = b"x ERROR\r\ny\r\nERROR\r\n";
        let mm = m("error", "or");
        let job = Job::new(3);
        run_scan(&job, data, &mm);
        assert_eq!(*job.matches.lock().unwrap(), vec![0, 2]);
        let job2 = Job::new(3);
        job2.cancel.store(true, Ordering::Relaxed);
        run_scan(&job2, data, &mm);
        assert!(job2.matches.lock().unwrap().is_empty());
    }
}
