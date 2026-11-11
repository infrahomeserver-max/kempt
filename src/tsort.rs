//! Sort modes for the table view: alphabetical, lexicographic, numeric, natural, version, date, length, frequency.

use crate::docs::Copy;
use crate::table::{column_is_numeric, field};
use ahash::RandomState;
use rayon::prelude::*;
use regex::Regex;
use std::borrow::Cow;
use std::cmp::Ordering;
use std::sync::OnceLock;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Auto,
    Alpha,
    Lex,
    Number,
    Natural,
    Version,
    Date,
    DateUs,
    Length,
    Freq,
}

impl Mode {
    pub fn parse(s: &str) -> Mode {
        match s {
            "alpha" => Mode::Alpha,
            "lex" => Mode::Lex,
            "number" => Mode::Number,
            "natural" => Mode::Natural,
            "version" => Mode::Version,
            "date" => Mode::Date,
            "date_us" => Mode::DateUs,
            "length" => Mode::Length,
            "freq" => Mode::Freq,
            _ => Mode::Auto,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Mode::Auto => "auto",
            Mode::Alpha => "alpha",
            Mode::Lex => "lex",
            Mode::Number => "number",
            Mode::Natural => "natural",
            Mode::Version => "version",
            Mode::Date => "date",
            Mode::DateUs => "date_us",
            Mode::Length => "length",
            Mode::Freq => "freq",
        }
    }
}

// ───────────── comparison helpers ─────────────

fn lower(b: u8) -> u8 {
    b.to_ascii_lowercase()
}

pub fn cmp_ci(a: &[u8], b: &[u8]) -> Ordering {
    a.iter().map(|&x| lower(x)).cmp(b.iter().map(|&x| lower(x)))
}

fn digits_end(s: &[u8], mut i: usize) -> usize {
    while i < s.len() && s[i].is_ascii_digit() {
        i += 1;
    }
    i
}

/// compare two digit runs by value (arbitrary length)
fn cmp_digits(a: &[u8], b: &[u8]) -> Ordering {
    let a = &a[a.iter().position(|&x| x != b'0').unwrap_or(a.len())..];
    let b = &b[b.iter().position(|&x| x != b'0').unwrap_or(b.len())..];
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}

/// "file2" < "file10", ignoring case
pub fn natural_cmp(a: &[u8], b: &[u8]) -> Ordering {
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if a[i].is_ascii_digit() && b[j].is_ascii_digit() {
            let (ie, je) = (digits_end(a, i), digits_end(b, j));
            let o = cmp_digits(&a[i..ie], &b[j..je]);
            if o != Ordering::Equal {
                return o;
            }
            i = ie;
            j = je;
        } else {
            let o = lower(a[i]).cmp(&lower(b[j]));
            if o != Ordering::Equal {
                return o;
            }
            i += 1;
            j += 1;
        }
    }
    (a.len() - i).cmp(&(b.len() - j))
}

/// "1.2.9" < "1.2.10" < "1.10"; a leading v is ignored and `.`, `-`, `_`, `+` all separate parts
pub fn version_cmp(a: &[u8], b: &[u8]) -> Ordering {
    fn parts(s: &[u8]) -> Vec<&[u8]> {
        let s = if s.first().is_some_and(|c| lower(*c) == b'v') && s.get(1).is_some_and(|c| c.is_ascii_digit()) { &s[1..] } else { s };
        s.split(|c| matches!(c, b'.' | b'-' | b'_' | b'+')).filter(|p| !p.is_empty()).collect()
    }
    let (pa, pb) = (parts(a), parts(b));
    for (x, y) in pa.iter().zip(pb.iter()) {
        let o = match (x.iter().all(u8::is_ascii_digit), y.iter().all(u8::is_ascii_digit)) {
            (true, true) => cmp_digits(x, y),
            (true, false) => Ordering::Less, // numbers before words ("1.0.0" before "1.0.0-beta" is not modelled)
            (false, true) => Ordering::Greater,
            (false, false) => cmp_ci(x, y),
        };
        if o != Ordering::Equal {
            return o;
        }
    }
    pa.len().cmp(&pb.len())
}

// ───────────── value parsers ─────────────

/// numbers as people write them: `$1,234.50`, `12%`, `(5)` (negative), `1 234`
pub fn parse_loose(b: &[u8]) -> Option<f64> {
    let s = std::str::from_utf8(b).ok()?.trim();
    if s.is_empty() {
        return None;
    }
    let (neg, s) = if s.starts_with('(') && s.ends_with(')') && s.len() > 2 { (true, &s[1..s.len() - 1]) } else { (false, s) };
    let cleaned: String = s.chars().filter(|c| !matches!(c, '$' | '€' | '£' | '¥' | '₹' | '%' | ',' | ' ' | '\u{a0}' | '_' | '\'')).collect();
    let v = cleaned.parse::<f64>().ok().filter(|v| v.is_finite())?;
    Some(if neg { -v } else { v })
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn month_of(s: &str) -> Option<i64> {
    let m = s.to_ascii_lowercase();
    let m = &m[..m.len().min(3)];
    ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"].iter().position(|x| *x == m).map(|i| i as i64 + 1)
}

struct Res {
    iso: Regex,
    dmy: Regex,
    dmon: Regex,
    mond: Regex,
    time: Regex,
    tail: Regex,
}

fn res() -> &'static Res {
    static R: OnceLock<Res> = OnceLock::new();
    R.get_or_init(|| Res {
        iso: Regex::new(r"^(\d{4})[-/.](\d{1,2})[-/.](\d{1,2})(.*)$").unwrap(),
        dmy: Regex::new(r"^(\d{1,2})[-/.](\d{1,2})[-/.](\d{4}|\d{2})(.*)$").unwrap(),
        dmon: Regex::new(r"^(\d{1,2})(?:st|nd|rd|th)?[\s.-]+([A-Za-z]{3,9})\.?[\s,.-]+(\d{4})(.*)$").unwrap(),
        mond: Regex::new(r"^([A-Za-z]{3,9})\.?\s+(\d{1,2})(?:st|nd|rd|th)?,?\s+(\d{4})(.*)$").unwrap(),
        time: Regex::new(r"^(\d{1,2}):(\d{2})(?::(\d{2}))?(?:[.,]\d+)?\s*([AaPp][Mm])?$").unwrap(),
        tail: Regex::new(r"^(?:[T\s,]+(\d{1,2}):(\d{2})(?::(\d{2}))?(?:[.,]\d+)?\s*([AaPp][Mm])?\s*(Z|UTC|[+-]\d{2}:?\d{2})?)?\s*$").unwrap(),
    })
}

fn clock(h: i64, m: i64, s: i64, ampm: Option<&str>) -> Option<i64> {
    let mut h = h;
    if let Some(ap) = ampm {
        if !(1..=12).contains(&h) {
            return None;
        }
        h %= 12;
        if ap.eq_ignore_ascii_case("pm") {
            h += 12;
        }
    }
    (h < 24 && m < 60 && s < 61).then_some(h * 3600 + m * 60 + s)
}

/// seconds since 1970 (UTC) for the usual written dates and times; `us` = month comes first in `03/04/2026`
pub fn parse_date(b: &[u8], us: bool) -> Option<f64> {
    let s = std::str::from_utf8(b).ok()?.trim();
    if s.is_empty() {
        return None;
    }
    let r = res();
    let (y, mo, d, rest): (i64, i64, i64, &str);
    let caps;
    if let Some(c) = r.iso.captures(s) {
        caps = c;
        (y, mo, d) = (caps[1].parse().ok()?, caps[2].parse().ok()?, caps[3].parse().ok()?);
        rest = caps.get(4).map_or("", |m| m.as_str());
    } else if let Some(c) = r.dmy.captures(s) {
        caps = c;
        let (a, b2): (i64, i64) = (caps[1].parse().ok()?, caps[2].parse().ok()?);
        let mut yy: i64 = caps[3].parse().ok()?;
        if caps[3].len() == 2 {
            yy += if yy < 70 { 2000 } else { 1900 };
        }
        (y, mo, d) = if us { (yy, a, b2) } else { (yy, b2, a) };
        rest = caps.get(4).map_or("", |m| m.as_str());
    } else if let Some(c) = r.dmon.captures(s) {
        caps = c;
        (y, mo, d) = (caps[3].parse().ok()?, month_of(&caps[2])?, caps[1].parse().ok()?);
        rest = caps.get(4).map_or("", |m| m.as_str());
    } else if let Some(c) = r.mond.captures(s) {
        caps = c;
        (y, mo, d) = (caps[3].parse().ok()?, month_of(&caps[1])?, caps[2].parse().ok()?);
        rest = caps.get(4).map_or("", |m| m.as_str());
    } else if let Some(c) = r.time.captures(s) {
        // a bare time of day sorts by seconds since midnight
        let ss = c.get(3).map_or(0, |m| m.as_str().parse().unwrap_or(0));
        return clock(c[1].parse().ok()?, c[2].parse().ok()?, ss, c.get(4).map(|m| m.as_str())).map(|v| v as f64);
    } else {
        return None;
    }
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) {
        return None;
    }
    let t = r.tail.captures(rest)?;
    let mut secs = 0;
    if let Some(h) = t.get(1) {
        let ss = t.get(3).map_or(0, |m| m.as_str().parse().unwrap_or(0));
        secs = clock(h.as_str().parse().ok()?, t[2].parse().ok()?, ss, t.get(4).map(|m| m.as_str()))?;
    }
    let mut off = 0;
    if let Some(z) = t.get(5) {
        let z = z.as_str();
        if z.len() >= 5 && (z.starts_with('+') || z.starts_with('-')) {
            let digits: String = z[1..].chars().filter(|c| c.is_ascii_digit()).collect();
            let (hh, mm): (i64, i64) = (digits[..2].parse().ok()?, digits[2..4].parse().ok()?);
            off = (hh * 3600 + mm * 60) * if z.starts_with('-') { -1 } else { 1 };
        }
    }
    Some((days_from_civil(y, mo, d) * 86400 + secs - off) as f64)
}

// ───────────── the sort ─────────────

fn prefix_key(b: &[u8], fold: bool) -> u64 {
    let mut k = [0u8; 8];
    for (i, x) in b.iter().take(8).enumerate() {
        k[i] = if fold { lower(*x) } else { *x };
    }
    u64::from_be_bytes(k)
}

/// Turn `Auto` into what the column really holds.
pub fn resolve(c: &Copy, d: u8, header: bool, nrows: usize, col: usize, m: Mode) -> Mode {
    match m {
        Mode::Auto => {
            if column_is_numeric(c, d, header, nrows, col) {
                Mode::Number
            } else {
                Mode::Alpha
            }
        }
        m => m,
    }
}

/// Rows that have no value (blank, or not a number/date in a number/date sort) always go last.
pub fn sort_rows(c: &Copy, d: u8, header: bool, rows: Vec<u32>, col: usize, desc: bool, mode: Mode) -> Vec<u32> {
    let line = |r: u32| c.line(r as usize + header as usize);
    let cell = |r: u32| -> Cow<[u8]> { field(line(r), d, col).unwrap_or(Cow::Borrowed(&[])) };
    let by_key = |key: &(dyn Fn(usize, u32) -> Option<f64> + Sync)| -> Vec<u32> {
        let keyed: Vec<(Option<f64>, u32)> = rows.par_iter().enumerate().map(|(i, &r)| (key(i, r), r)).collect();
        let (mut ok, blank): (Vec<_>, Vec<_>) = keyed.into_iter().partition(|k| k.0.is_some());
        ok.par_sort_by(|a, b| {
            let o = a.0.unwrap().total_cmp(&b.0.unwrap());
            if desc { o.reverse() } else { o }
        });
        ok.into_iter().chain(blank).map(|k| k.1).collect()
    };
    let nonblank = |r: u32| -> bool { !cell(r).trim_ascii().is_empty() };
    match mode {
        Mode::Number | Mode::Auto => by_key(&|_, r| parse_loose(&cell(r))),
        Mode::Date => by_key(&|_, r| parse_date(&cell(r), false)),
        Mode::DateUs => by_key(&|_, r| parse_date(&cell(r), true)),
        Mode::Length => by_key(&|_, r| nonblank(r).then(|| String::from_utf8_lossy(cell(r).trim_ascii()).chars().count() as f64)),
        Mode::Freq => {
            let rs = RandomState::with_seeds(0x51, 0xe7, 0x9a, 0x33);
            let hs: Vec<Option<u64>> = rows
                .par_iter()
                .map(|&r| {
                    let f = cell(r);
                    let t = f.trim_ascii();
                    (!t.is_empty()).then(|| rs.hash_one(t.to_ascii_lowercase()))
                })
                .collect();
            let mut counts: ahash::AHashMap<u64, u32> = ahash::AHashMap::new();
            for h in hs.iter().flatten() {
                *counts.entry(*h).or_insert(0) += 1;
            }
            by_key(&|i, _| hs[i].map(|h| counts[&h] as f64))
        }
        Mode::Alpha | Mode::Lex | Mode::Natural | Mode::Version => {
            let fold = mode == Mode::Alpha;
            let use_prefix = matches!(mode, Mode::Alpha | Mode::Lex);
            let keyed: Vec<(u64, bool, u32)> = rows
                .par_iter()
                .map(|&r| {
                    let f = cell(r);
                    let t = f.trim_ascii();
                    (if use_prefix { prefix_key(t, fold) } else { 0 }, t.is_empty(), r)
                })
                .collect();
            let (mut ok, blank): (Vec<_>, Vec<_>) = keyed.into_iter().partition(|k| !k.1);
            ok.par_sort_by(|a, b| {
                let o = a.0.cmp(&b.0).then_with(|| {
                    let (fa, fb) = (cell(a.2), cell(b.2));
                    let (x, y) = (fa.trim_ascii(), fb.trim_ascii());
                    match mode {
                        Mode::Alpha => cmp_ci(x, y),
                        Mode::Lex => x.cmp(y),
                        Mode::Natural => natural_cmp(x, y),
                        _ => version_cmp(x, y),
                    }
                });
                if desc { o.reverse() } else { o }
            });
            ok.into_iter().chain(blank).map(|k| k.2).collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_and_version() {
        assert_eq!(natural_cmp(b"file2", b"file10"), Ordering::Less);
        assert_eq!(natural_cmp(b"File2", b"file2"), Ordering::Equal);
        assert_eq!(natural_cmp(b"a007", b"a7"), Ordering::Equal);
        assert_eq!(version_cmp(b"1.2.9", b"1.2.10"), Ordering::Less);
        assert_eq!(version_cmp(b"v2.0", b"1.10.3"), Ordering::Greater);
        assert_eq!(version_cmp(b"1.2", b"1.2.1"), Ordering::Less);
    }

    #[test]
    fn loose_numbers() {
        assert_eq!(parse_loose(b"$1,234.50"), Some(1234.5));
        assert_eq!(parse_loose(b"12%"), Some(12.0));
        assert_eq!(parse_loose(b"(5)"), Some(-5.0));
        assert_eq!(parse_loose(b"-3.5e2"), Some(-350.0));
        assert_eq!(parse_loose(b"abc"), None);
        assert_eq!(parse_loose(b""), None);
    }

    #[test]
    fn dates() {
        let iso = parse_date(b"2026-11-03", false).unwrap();
        assert_eq!(iso, 1793664000.0);
        assert_eq!(parse_date(b"2026-11-03 10:00:01", false).unwrap(), iso + 36001.0);
        assert_eq!(parse_date(b"2026-11-03T10:00:00Z", false).unwrap(), iso + 36000.0);
        assert_eq!(parse_date(b"2026-11-03T10:00:00+05:30", false).unwrap(), iso + 36000.0 - 19800.0);
        assert_eq!(parse_date(b"03/11/2026", false), Some(iso));
        assert_eq!(parse_date(b"11/03/2026", true), Some(iso));
        assert_eq!(parse_date(b"3 Nov 2026", false), Some(iso));
        assert_eq!(parse_date(b"November 3, 2026", false), Some(iso));
        assert_eq!(parse_date(b"03.11.26", false), Some(iso));
        assert_eq!(parse_date(b"2026-11-03 3:30 PM", false).unwrap(), iso + 15.5 * 3600.0);
        assert_eq!(parse_date(b"14:30", false), Some(52200.0));
        assert_eq!(parse_date(b"2026-13-03", false), None);
        assert_eq!(parse_date(b"hello", false), None);
        assert!(parse_date(b"1999-12-31", false).unwrap() < parse_date(b"2000-01-01", false).unwrap());
    }
}
