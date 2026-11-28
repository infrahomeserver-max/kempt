//! Timestamps at the start of log lines: reading them, and finding a time in a (mostly) time-ordered log by binary search.
//! Recognised: `2026-11-27 10:30:15`, `2026-11-27T10:30:15.123Z`, `[2026-11-27 10:30]`, with `-` or `/` between date parts.
//! Lines without a timestamp of their own (stack traces, wrapped text) belong to the timestamped line before them.

/// Sortable key `YYYYMMDDHHMMSSmmm` of the timestamp that starts `line` (after an optional `[`, `(` or spaces).
pub fn key(line: &[u8]) -> Option<u64> {
    let mut i = 0;
    while i < line.len() && i < 4 && matches!(line[i], b' ' | b'[' | b'(' | b'\t') {
        i += 1;
    }
    let l = &line[i..];
    let d = |p: usize, n: usize| -> Option<u64> {
        let s = l.get(p..p + n)?;
        s.iter().all(u8::is_ascii_digit).then(|| s.iter().fold(0u64, |a, b| a * 10 + (b - b'0') as u64))
    };
    let (y, mo, da) = (d(0, 4)?, d(5, 2)?, d(8, 2)?);
    if !matches!(l.get(4)?, b'-' | b'/') || l.get(7) != l.get(4) || !(1..=12).contains(&mo) || !(1..=31).contains(&da) {
        return None;
    }
    let (mut h, mut mi, mut s, mut ms) = (0, 0, 0, 0);
    if matches!(l.get(10), Some(b'T') | Some(b' ')) {
        if let (Some(hh), Some(mm)) = (d(11, 2), d(14, 2)) {
            if l.get(13) == Some(&b':') && hh < 24 && mm < 60 {
                h = hh;
                mi = mm;
                if l.get(16) == Some(&b':') {
                    if let Some(ss) = d(17, 2) {
                        s = ss.min(60);
                        if matches!(l.get(19), Some(b'.') | Some(b',')) {
                            let f: Vec<u64> = l[20..].iter().take_while(|c| c.is_ascii_digit()).take(3).map(|c| (c - b'0') as u64).collect();
                            ms = f.iter().enumerate().fold(0, |a, (k, v)| a + v * [100, 10, 1][k]);
                        }
                    }
                }
            }
        }
    }
    Some((((((y * 100 + mo) * 100 + da) * 100 + h) * 100 + mi) * 100 + s) * 1000 + ms)
}

/// What the person typed: `2026-11-27`, `2026-11-27 10:30`, `2026-11-27T10:30:15.5`, or just `10:30` / `10:30:15` (the date of the log).
/// `end` makes a missing part count as its last value (so a range "to 10:30" includes the whole minute).
pub fn parse_target(text: &str, default_date: Option<u64>, end: bool) -> Option<u64> {
    let t = text.trim().replace(['T', ','], " ");
    let mut parts = t.split_whitespace();
    let first = parts.next()?;
    let second = parts.next();
    if parts.next().is_some() {
        return None;
    }
    let (date, time) = if first.contains(':') && second.is_none() { (None, Some(first)) } else { (Some(first), second) };
    let ymd = match date {
        Some(ds) => {
            let n: Vec<u64> = ds.split(['-', '/']).map(|x| x.parse().ok()).collect::<Option<_>>()?;
            if n.len() != 3 || n[0] < 1000 || !(1..=12).contains(&n[1]) || !(1..=31).contains(&n[2]) {
                return None;
            }
            (n[0] * 100 + n[1]) * 100 + n[2]
        }
        None => default_date?,
    };
    let (mut h, mut mi, mut s, mut ms) = if end { (23, 59, 59, 999) } else { (0, 0, 0, 0) };
    if let Some(ts) = time {
        let (main, frac) = ts.split_once('.').unwrap_or((ts, ""));
        let n: Vec<u64> = main.split(':').map(|x| x.parse().ok()).collect::<Option<_>>()?;
        if n.is_empty() || n.len() > 3 || n[0] > 23 || n.get(1).is_some_and(|v| *v > 59) || n.get(2).is_some_and(|v| *v > 59) {
            return None;
        }
        h = n[0];
        mi = n.get(1).copied().unwrap_or(if end { 59 } else { 0 });
        s = n.get(2).copied().unwrap_or(if end { 59 } else { 0 });
        ms = if frac.is_empty() { if end && n.len() == 3 { 999 } else if end { 999 } else { 0 } } else { frac.chars().take(3).collect::<String>().parse::<u64>().ok()? * [100, 10, 1][frac.len().clamp(1, 3) - 1] };
    }
    Some(((((ymd * 100 + h) * 100 + mi) * 100 + s) * 1000) + ms)
}

/// `20261127103015123` as `2026-11-27 10:30:15`.
pub fn show(k: u64) -> String {
    let k = k / 1000;
    let (s, mi, h, d, mo, y) = (k % 100, k / 100 % 100, k / 10_000 % 100, k / 1_000_000 % 100, k / 100_000_000 % 100, k / 10_000_000_000);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}")
}

/// The key that belongs to line `i`: its own, or that of the nearest timestamped line before it (looking back up to `look` lines).
fn key_of(n: usize, line: &dyn Fn(usize) -> Vec<u8>, i: usize, look: usize) -> Option<(usize, u64)> {
    let _ = n;
    let mut j = i;
    for _ in 0..=look {
        if let Some(k) = key(&line(j)) {
            return Some((j, k));
        }
        if j == 0 {
            break;
        }
        j -= 1;
    }
    None
}

/// First line whose time is at or after `target`, assuming the log is time-ordered. `None` if no line has a timestamp.
pub fn first_at_or_after(n: usize, line: &dyn Fn(usize) -> Vec<u8>, target: u64) -> Option<usize> {
    let (mut lo, mut hi) = (0usize, n);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        match key_of(n, line, mid, 200) {
            Some((_, k)) if k >= target => hi = mid,
            Some(_) => lo = mid + 1,
            None => lo = mid + 1,   // no timestamp nearby: carry on to the right
        }
    }
    // land on a line that has its own timestamp
    let mut i = lo.min(n.saturating_sub(1));
    for _ in 0..200 {
        if i >= n || key(&line(i)).is_some() {
            break;
        }
        i += 1;
    }
    Some(i.min(n.saturating_sub(1)))
}

/// Timestamp of the first and of the last line that has one (searching at most 100 000 lines from each end).
pub fn span(n: usize, line: &dyn Fn(usize) -> Vec<u8>) -> Option<(u64, u64)> {
    let a = (0..n.min(100_000)).find_map(|i| key(&line(i)))?;
    let b = (0..n.min(100_000)).find_map(|i| key(&line(n - 1 - i)))?;
    Some((a, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn log() -> Vec<Vec<u8>> {
        let mut v = vec![];
        for m in 0..120u64 {
            v.push(format!("2026-11-27 10:{:02}:{:02}.{:03} INFO [x] line {m}", m / 2, (m % 2) * 30, m % 7).into_bytes());
            if m % 10 == 3 {
                v.push(b"    at com.example.Foo(Foo.java:1)".to_vec());
                v.push(b"    at com.example.Bar(Bar.java:2)".to_vec());
            }
        }
        v
    }

    #[test]
    fn reads_common_timestamps() {
        assert_eq!(key(b"2026-11-27 10:30:15 x"), Some(20261127103015000));
        assert_eq!(key(b"2026-11-27T10:30:15.5Z x"), Some(20261127103015500));
        assert_eq!(key(b"[2026-11-27 10:30:15,123] x"), Some(20261127103015123));
        assert_eq!(key(b"2026/11/27 10:30 x"), Some(20261127103000000));
        assert_eq!(key(b"2026-11-27 x"), Some(20261127000000000));
        assert_eq!(key(b"    at java.lang.Thread"), None);
        assert_eq!(key(b"2026-13-27 10:30:15"), None);
        assert_eq!(key(b"12345 2026-11-27"), None);
        assert_eq!(key(b""), None);
    }

    #[test]
    fn parses_what_people_type() {
        let d = Some(20261127);
        assert_eq!(parse_target("2026-11-27 10:30:15", None, false), Some(20261127103015000));
        assert_eq!(parse_target("2026-11-27T10:30", None, false), Some(20261127103000000));
        assert_eq!(parse_target("10:30", d, false), Some(20261127103000000));
        assert_eq!(parse_target("10:30", d, true), Some(20261127103059999));
        assert_eq!(parse_target("2026-11-27", None, false), Some(20261127000000000));
        assert_eq!(parse_target("2026-11-27", None, true), Some(20261127235959999));
        assert_eq!(parse_target("10:30", None, false), None);
        assert_eq!(parse_target("25:00", d, false), None);
        assert_eq!(parse_target("hello", d, false), None);
        assert_eq!(parse_target("2026-02-30 10:00 extra", d, false), None);
    }

    #[test]
    fn binary_search_finds_the_first_line_and_skips_stack_traces() {
        let l = log();
        let f = |i: usize| l[i].clone();
        // every timestamped line is found by its own time
        for (i, line) in l.iter().enumerate() {
            if let Some(k) = key(line) {
                // times repeat only when equal; the first line with that time must be at or before i
                let got = first_at_or_after(l.len(), &f, k).unwrap();
                assert!(got <= i && key(&l[got]) == Some(k), "line {i}: got {got}");
            }
        }
        // a time between two lines lands on the next timestamped line, never in the middle of a stack trace
        let k = parse_target("10:03:10", Some(20261127), false).unwrap();
        let g = first_at_or_after(l.len(), &f, k).unwrap();
        assert!(key(&l[g]).is_some() && key(&l[g]).unwrap() >= k, "{g}");
        assert!(g == 0 || key(&l[g - 1]).map_or(true, |p| p < k) || key(&l[g - 1]).is_none());
        // after the end: the last line
        assert_eq!(first_at_or_after(l.len(), &f, 99991231235959999), Some(l.len() - 1));
    }

    #[test]
    fn shows_a_key() {
        assert_eq!(show(20261127103015123), "2026-11-27 10:30:15");
    }
}
