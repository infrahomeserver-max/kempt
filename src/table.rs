//! Excel-like table view over a delimited text file (CSV, TSV, `;`/`|` separated, …).
//!
//! The file stays a memory-mapped list of lines. A table is only a *view*: a delimiter, a header flag and,
//! when sorted or filtered, a list of row numbers (4 bytes per row). Rows are parsed when they are shown.

use crate::docs::Copy;
use ahash::RandomState;
use rayon::prelude::*;
use std::borrow::Cow;
use crate::tsort::{self, Mode};
use std::sync::Arc;

pub const MAX_COLS: usize = 1000;
pub const CELL_CLIP: usize = 400;

/// Visit the fields of one line (quote aware: `"a,b"`, `""` escapes). `f` returns false to stop.
pub fn walk<'a>(line: &'a [u8], d: u8, mut f: impl FnMut(Cow<'a, [u8]>) -> bool) {
    let n = line.len();
    let mut i = 0;
    loop {
        let (field, end): (Cow<'a, [u8]>, usize) = if i < n && line[i] == b'"' {
            let mut j = i + 1;
            let mut seg = j;
            let mut owned: Vec<u8> = Vec::new();
            let mut esc = false;
            loop {
                match memchr::memchr(b'"', &line[j..]) {
                    None => {
                        let tail = &line[seg..];
                        let c = if esc { owned.extend_from_slice(tail); Cow::Owned(owned) } else { Cow::Borrowed(tail) };
                        break (c, n);
                    }
                    Some(p) => {
                        let q = j + p;
                        if q + 1 < n && line[q + 1] == b'"' {
                            owned.extend_from_slice(&line[seg..=q]);
                            esc = true;
                            j = q + 2;
                            seg = j;
                        } else {
                            let tail = &line[seg..q];
                            let c = if esc { owned.extend_from_slice(tail); Cow::Owned(owned) } else { Cow::Borrowed(tail) };
                            let end = memchr::memchr(d, &line[q + 1..]).map(|x| q + 1 + x).unwrap_or(n);
                            break (c, end);
                        }
                    }
                }
            }
        } else {
            let e = memchr::memchr(d, &line[i..]).map(|x| i + x).unwrap_or(n);
            (Cow::Borrowed(&line[i..e]), e)
        };
        if !f(field) {
            return;
        }
        if end >= n {
            return;
        }
        i = end + 1;
        if i == n {
            f(Cow::Borrowed(&line[n..]));
            return;
        }
    }
}

pub fn split(line: &[u8], d: u8, max: usize) -> Vec<Cow<'_, [u8]>> {
    let mut out = Vec::new();
    walk(line, d, |f| {
        out.push(f);
        out.len() < max
    });
    out
}

pub fn field(line: &[u8], d: u8, col: usize) -> Option<Cow<'_, [u8]>> {
    let mut k = 0;
    let mut res = None;
    walk(line, d, |f| {
        if k == col {
            res = Some(f);
            false
        } else {
            k += 1;
            true
        }
    });
    res
}

/// Pick the delimiter that splits the first lines most consistently.
pub fn sniff(c: &Copy) -> u8 {
    let mut best = (0usize, b',');
    for &d in &[b',', b'\t', b';', b'|'] {
        let mut min = usize::MAX;
        let mut seen = 0;
        for i in 0..c.count().min(40) {
            let l = c.line(i);
            if l.is_empty() {
                continue;
            }
            seen += 1;
            let mut k = 0usize;
            walk(l, d, |_| {
                k += 1;
                true
            });
            min = min.min(k - 1);
        }
        if seen > 0 && min != usize::MAX && min > best.0 {
            best = (min, d);
        }
    }
    best.1
}

pub fn parse_delim(s: &str, c: &Copy, name: &str) -> u8 {
    match s {
        "," => b',',
        "tab" | "\t" => b'\t',
        ";" => b';',
        "|" => b'|',
        " " | "space" => b' ',
        // any other single ASCII character: ^ ~ : # and so on
        o if o.chars().count() == 1 && o.is_ascii() => o.as_bytes()[0],
        _ => {
            let n = name.to_ascii_lowercase();
            if n.ends_with(".tsv") || n.ends_with(".tab") {
                b'\t'
            } else {
                sniff(c)
            }
        }
    }
}

/// A, B, … Z, AA, AB, …
pub fn col_letters(mut i: usize) -> String {
    let mut s = Vec::new();
    loop {
        s.push(b'A' + (i % 26) as u8);
        if i < 26 {
            break;
        }
        i = i / 26 - 1;
    }
    s.reverse();
    String::from_utf8(s).unwrap()
}

fn text(b: &[u8], clip: bool) -> String {
    let s = String::from_utf8_lossy(b);
    if clip && s.chars().count() > CELL_CLIP {
        let mut t: String = s.chars().take(CELL_CLIP).collect();
        t.push('…');
        t
    } else {
        s.into_owned()
    }
}

#[derive(Clone)]
pub struct DupSpec {
    /// columns that together form the key (a row is a duplicate when ALL of them match); empty = the whole line
    pub cols: Vec<usize>,
    pub ci: bool,
}

pub struct DupInfo {
    pub spec: DupSpec,
    /// duplicate rows in group order
    pub rows: Vec<u32>,
    pub groups: usize,
    /// row -> (group number, group size)
    pub by_row: ahash::AHashMap<u32, (u32, u32)>,
}

pub struct Table {
    pub version: u64,
    pub delim: u8,
    pub header: bool,
    pub ncols: usize,
    pub names: Vec<String>,
    /// number of data rows (header excluded)
    pub nrows: usize,
    /// sort keys, most important first
    pub sort: Vec<(usize, bool, Mode)>,
    pub dup: Option<DupInfo>,
    /// column filters (all must match)
    pub filters: Vec<Filter>,
    pub filtered: Option<Arc<Vec<u32>>>,
    /// None = natural order
    pub order: Option<Arc<Vec<u32>>>,
}

impl Table {
    pub fn new(c: &Copy, version: u64, delim: u8, header: bool) -> Table {
        let mut ncols = 1;
        for i in 0..c.count().min(2000) {
            let mut k = 0;
            walk(c.line(i), delim, |_| {
                k += 1;
                k < MAX_COLS
            });
            ncols = ncols.max(k);
        }
        let mut names: Vec<String> = Vec::new();
        if header && c.count() > 0 {
            for f in split(c.line(0), delim, ncols) {
                names.push(text(&f, true));
            }
        }
        for i in 0..ncols {
            if names.len() <= i {
                names.push(col_letters(i));
            } else if names[i].trim().is_empty() {
                names[i] = col_letters(i);
            }
        }
        let nrows = c.count().saturating_sub(header as usize);
        Table { version, delim, header, ncols, names, nrows, sort: Vec::new(), dup: None, filters: Vec::new(), filtered: None, order: None }
    }

    pub fn shown(&self) -> usize {
        match &self.order {
            Some(o) => o.len(),
            None => self.nrows,
        }
    }

    fn line_of(&self, row: u32) -> usize {
        row as usize + self.header as usize
    }

    pub fn rows(&self, c: &Copy, from: usize, count: usize, clip: bool) -> Vec<serde_json::Value> {
        use serde_json::json;
        let end = (from + count).min(self.shown());
        let mut out = Vec::with_capacity(end.saturating_sub(from));
        for i in from..end {
            let r = match &self.order {
                Some(o) => o[i],
                None => i as u32,
            };
            let li = self.line_of(r);
            let cells: Vec<String> = split(c.line(li), self.delim, self.ncols).iter().map(|f| text(f, clip)).collect();
            let mut v = json!({"n": li + 1, "c": cells});
            if let Some(g) = self.dup.as_ref().and_then(|d| d.by_row.get(&r)) {
                v["g"] = json!(g.0);
                v["k"] = json!(g.1);
            }
            out.push(v);
        }
        out
    }

    /// Rebuild `order` from the current sort/dup settings.
    pub fn rebuild(&mut self, c: &Copy) {
        let base: Option<Vec<u32>> = match &self.dup {
            Some(d) => Some(d.rows.clone()),
            None => self.filtered.as_ref().map(|f| (**f).clone()),
        };
        if self.sort.is_empty() {
            self.order = base.map(Arc::new);
            return;
        }
        let mut rows: Vec<u32> = base.unwrap_or_else(|| (0..self.nrows as u32).collect());
        // every pass is stable, so sorting by the least important key first and the most important last gives "by A, then by B"
        for &(col, desc, mode) in self.sort.iter().rev() {
            rows = tsort::sort_rows(c, self.delim, self.header, rows, col, desc, mode);
        }
        self.order = Some(Arc::new(rows));
    }

    /// rows that pass the filters (all rows when there are none), in file order
    pub fn base_rows(&self) -> Vec<u32> {
        match &self.filtered {
            Some(f) => (**f).clone(),
            None => (0..self.nrows as u32).collect(),
        }
    }

    pub fn find_dups(&mut self, c: &Copy, spec: DupSpec) {
        let (d, hdr) = (self.delim, self.header);
        let rs = RandomState::with_seeds(0x51, 0xe7, 0x9a, 0x33);
        let base = self.base_rows();
        let hashes: Vec<Option<u64>> = base
            .par_iter()
            .map(|&r| {
                let line = c.line(r as usize + hdr as usize);
                let key: Cow<[u8]> = if spec.cols.is_empty() {
                    Cow::Borrowed(line)
                } else {
                    // several columns form one key; the 0x1f separator keeps ("ab","c") apart from ("a","bc")
                    let mut k: Vec<u8> = Vec::new();
                    let mut any = false;
                    let want = spec.cols.iter().max().map_or(0, |m| m + 1);
                    let cells = split(line, d, want);
                    for (i, &col) in spec.cols.iter().enumerate() {
                        if i > 0 {
                            k.push(0x1f);
                        }
                        if let Some(f) = cells.get(col) {
                            let t = f.trim_ascii();
                            any |= !t.is_empty();
                            k.extend_from_slice(t);
                        }
                    }
                    if !any {
                        return None; // every key cell is blank: not a duplicate of anything
                    }
                    Cow::Owned(k)
                };
                Some(if spec.ci { rs.hash_one(key.to_ascii_lowercase()) } else { rs.hash_one(&*key) })
            })
            .collect();
        let mut map: ahash::AHashMap<u64, (u32, u32)> = ahash::AHashMap::with_capacity(hashes.len() / 2 + 1);
        for (i, h) in hashes.iter().enumerate() {
            if let Some(h) = h {
                let e = map.entry(*h).or_insert((base[i], 0));
                e.1 += 1;
            }
        }
        let mut items: Vec<(u32, u64, u32)> = Vec::new(); // (first row of group, hash, row)
        let mut groups = 0usize;
        for (i, h) in hashes.iter().enumerate() {
            if let Some(h) = h {
                let (first, cnt) = map[h];
                if cnt > 1 {
                    if first == base[i] {
                        groups += 1;
                    }
                    items.push((first, *h, base[i]));
                }
            }
        }
        // keep a group together, groups in order of first appearance
        items.sort_by_key(|&(first, _, r)| (first, r));
        let mut rows = Vec::with_capacity(items.len());
        let mut group = Vec::with_capacity(items.len());
        let mut gi = 0u32;
        let mut last = u32::MAX;
        for (first, h, r) in items {
            if first != last {
                if last != u32::MAX {
                    gi += 1;
                }
                last = first;
            }
            rows.push(r);
            group.push((gi, map[&h].1));
        }
        let by_row = rows.iter().copied().zip(group.iter().copied()).collect();
        self.dup = Some(DupInfo { spec, rows, groups, by_row });
        self.rebuild(c);
    }

    /// Replace the column filters. An active duplicate search is redone over the filtered rows.
    pub fn set_filters(&mut self, c: &Copy, filters: Vec<Filter>) {
        self.filtered = if filters.is_empty() { None } else { Some(Arc::new(passing(c, self, &filters, None))) };
        self.filters = filters;
        match self.dup.as_ref().map(|d| d.spec.clone()) {
            Some(spec) => self.find_dups(c, spec),
            None => self.rebuild(c),
        }
    }

    pub fn clear_dups(&mut self, c: &Copy) {
        self.dup = None;
        self.rebuild(c);
    }

    pub fn set_sort(&mut self, c: &Copy, keys: Vec<(usize, bool, Mode)>) {
        // "automatic" is settled once, here, so the status bar can say what each column was treated as
        self.sort = keys.into_iter().take(8).map(|(col, desc, m)| (col, desc, tsort::resolve(c, self.delim, self.header, self.nrows, col, m))).collect();
        self.rebuild(c);
    }
}

pub fn parse_num(b: &[u8]) -> Option<f64> {
    let s = std::str::from_utf8(b).ok()?.trim();
    if s.is_empty() {
        return None;
    }
    let v = s.parse::<f64>().ok().or_else(|| s.replace(',', "").parse::<f64>().ok())?;
    v.is_finite().then_some(v)
}

/// A column is numeric when every non-blank value among the first 5000 parses as a number.
pub fn column_is_numeric(c: &Copy, d: u8, header: bool, nrows: usize, col: usize) -> bool {
    let mut seen = 0;
    for r in 0..nrows {
        let f = field(c.line(r + header as usize), d, col);
        let Some(f) = f else { continue };
        if f.trim_ascii().is_empty() {
            continue;
        }
        if parse_num(&f).is_none() {
            return false;
        }
        seen += 1;
        if seen >= 5000 {
            break;
        }
    }
    seen > 0
}

// ───────────── column filters (the funnel) ─────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FOp {
    Contains,
    NotContains,
    Eq,
    Neq,
    Starts,
    Ends,
    Blank,
    NotBlank,
    Gt,
    Ge,
    Lt,
    Le,
    Between,
    In,
}

impl FOp {
    pub fn parse(s: &str) -> Option<FOp> {
        Some(match s {
            "contains" => FOp::Contains,
            "ncontains" => FOp::NotContains,
            "eq" => FOp::Eq,
            "neq" => FOp::Neq,
            "starts" => FOp::Starts,
            "ends" => FOp::Ends,
            "blank" => FOp::Blank,
            "nblank" => FOp::NotBlank,
            "gt" => FOp::Gt,
            "ge" => FOp::Ge,
            "lt" => FOp::Lt,
            "le" => FOp::Le,
            "between" => FOp::Between,
            "in" => FOp::In,
            _ => return None,
        })
    }
    pub fn name(self) -> &'static str {
        match self {
            FOp::Contains => "contains",
            FOp::NotContains => "ncontains",
            FOp::Eq => "eq",
            FOp::Neq => "neq",
            FOp::Starts => "starts",
            FOp::Ends => "ends",
            FOp::Blank => "blank",
            FOp::NotBlank => "nblank",
            FOp::Gt => "gt",
            FOp::Ge => "ge",
            FOp::Lt => "lt",
            FOp::Le => "le",
            FOp::Between => "between",
            FOp::In => "in",
        }
    }
    pub fn is_numeric(self) -> bool {
        matches!(self, FOp::Gt | FOp::Ge | FOp::Lt | FOp::Le | FOp::Between)
    }
}

#[derive(Clone, Debug)]
pub struct Filter {
    pub col: usize,
    pub op: FOp,
    pub v: String,
    pub v2: String,
    /// text comparisons ignore upper/lower case unless this is false
    pub ci: bool,
    /// for `In`: the exact (trimmed) cell values to keep; "" means blank cells
    pub list: Vec<String>,
}

struct Compiled {
    col: usize,
    op: FOp,
    s: Vec<u8>,
    n1: f64,
    n2: f64,
    ci: bool,
    set: ahash::AHashSet<Vec<u8>>,
}

fn fold(b: &[u8], ci: bool) -> Cow<'_, [u8]> {
    if !ci {
        Cow::Borrowed(b)
    } else if b.is_ascii() {
        Cow::Owned(b.to_ascii_lowercase())
    } else {
        Cow::Owned(String::from_utf8_lossy(b).to_lowercase().into_bytes())
    }
}

impl Filter {
    /// error text if the filter value is unusable (for example a word in a number comparison)
    pub fn check(&self) -> Option<String> {
        if self.op.is_numeric() {
            if tsort::parse_loose(self.v.as_bytes()).is_none() {
                return Some(format!("“{}” is not a number", self.v));
            }
            if self.op == FOp::Between && tsort::parse_loose(self.v2.as_bytes()).is_none() {
                return Some(format!("“{}” is not a number", self.v2));
            }
        }
        None
    }
    fn compile(&self) -> Compiled {
        let (a, b) = (tsort::parse_loose(self.v.as_bytes()).unwrap_or(0.0), tsort::parse_loose(self.v2.as_bytes()).unwrap_or(0.0));
        let (n1, n2) = if self.op == FOp::Between && b < a { (b, a) } else { (a, b) };
        Compiled {
            col: self.col,
            op: self.op,
            s: fold(self.v.trim().as_bytes(), self.ci).into_owned(),
            n1,
            n2,
            ci: self.ci,
            set: self.list.iter().map(|x| x.trim().as_bytes().to_vec()).collect(),
        }
    }
}

impl Compiled {
    fn matches(&self, cell: &[u8]) -> bool {
        let t = cell.trim_ascii();
        match self.op {
            FOp::Blank => t.is_empty(),
            FOp::NotBlank => !t.is_empty(),
            FOp::In => self.set.contains(t),
            FOp::Gt | FOp::Ge | FOp::Lt | FOp::Le | FOp::Between => match tsort::parse_loose(t) {
                None => false,
                Some(x) => match self.op {
                    FOp::Gt => x > self.n1,
                    FOp::Ge => x >= self.n1,
                    FOp::Lt => x < self.n1,
                    FOp::Le => x <= self.n1,
                    _ => x >= self.n1 && x <= self.n2,
                },
            },
            op => {
                let f = fold(t, self.ci);
                match op {
                    FOp::Contains => memchr::memmem::find(&f, &self.s).is_some(),
                    FOp::NotContains => memchr::memmem::find(&f, &self.s).is_none(),
                    FOp::Eq => *f == *self.s,
                    FOp::Neq => *f != *self.s,
                    FOp::Starts => f.starts_with(&self.s),
                    _ => f.ends_with(&self.s),
                }
            }
        }
    }
}

/// rows (in file order) that pass `filters`, ignoring the filters of column `skip`
fn passing(c: &Copy, t: &Table, filters: &[Filter], skip: Option<usize>) -> Vec<u32> {
    let cf: Vec<Compiled> = filters.iter().filter(|f| Some(f.col) != skip).map(|f| f.compile()).collect();
    let (d, hdr) = (t.delim, t.header);
    let want = cf.iter().map(|f| f.col).max().map_or(0, |m| m + 1);
    (0..t.nrows as u32)
        .into_par_iter()
        .filter(|&r| {
            if cf.is_empty() {
                return true;
            }
            let cells = split(c.line(r as usize + hdr as usize), d, want);
            cf.iter().all(|f| f.matches(cells.get(f.col).map_or(&b""[..], |x| &x[..])))
        })
        .collect()
}

/// Distinct values of a column (with how many rows have each), among the rows that pass every OTHER column's filter,
/// most frequent first. Like Excel's value list in the filter drop-down.
pub fn distinct(c: &Copy, t: &Table, col: usize, limit: usize) -> (Vec<(String, u32)>, bool, usize) {
    let rows = passing(c, t, &t.filters, Some(col));
    let (d, hdr) = (t.delim, t.header);
    let mut map: ahash::AHashMap<Vec<u8>, u32> = ahash::AHashMap::new();
    let mut truncated = false;
    for &r in &rows {
        let f = field(c.line(r as usize + hdr as usize), d, col).unwrap_or(Cow::Borrowed(&[]));
        let k = f.trim_ascii();
        if let Some(n) = map.get_mut(k) {
            *n += 1;
        } else if map.len() < 100_000 {
            map.insert(k.to_vec(), 1);
        } else {
            truncated = true;
        }
    }
    let mut v: Vec<(String, u32)> = map.into_iter().map(|(k, n)| (String::from_utf8_lossy(&k).into_owned(), n)).collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase())));
    if v.len() > limit {
        truncated = true;
        v.truncate(limit);
    }
    (v, truncated, rows.len())
}

pub struct ColStats {
    pub count: usize,
    pub blank: usize,
    pub unique: usize,
    pub dup_rows: usize,
    pub numeric: Option<(usize, f64, f64, f64)>, // n, sum, min, max
}

pub fn col_stats(c: &Copy, t: &Table, col: usize) -> ColStats {
    let (d, hdr) = (t.delim, t.header);
    let rs = RandomState::with_seeds(0x51, 0xe7, 0x9a, 0x33);
    let rows = t.base_rows();
    let vals: Vec<(Option<u64>, Option<f64>)> = rows
        .par_iter()
        .map(|&r| {
            let f = field(c.line(r as usize + hdr as usize), d, col);
            match f {
                Some(f) if !f.trim_ascii().is_empty() => (Some(rs.hash_one(f.trim_ascii())), parse_num(&f)),
                _ => (None, None),
            }
        })
        .collect();
    let mut counts: ahash::AHashMap<u64, u32> = ahash::AHashMap::new();
    let mut blank = 0;
    let (mut n, mut sum, mut mn, mut mx, mut nonblank) = (0usize, 0f64, f64::INFINITY, f64::NEG_INFINITY, 0usize);
    for (h, v) in &vals {
        match h {
            Some(h) => {
                nonblank += 1;
                *counts.entry(*h).or_insert(0) += 1;
            }
            None => blank += 1,
        }
        if let Some(v) = v {
            n += 1;
            sum += v;
            mn = mn.min(*v);
            mx = mx.max(*v);
        }
    }
    let dup_rows = counts.values().filter(|&&c| c > 1).map(|&c| c as usize).sum();
    ColStats { count: rows.len(), blank, unique: counts.len(), dup_rows, numeric: (nonblank > 0 && n == nonblank).then_some((n, sum, mn, mx)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(s: &str, d: u8) -> Vec<String> {
        split(s.as_bytes(), d, 100).iter().map(|x| String::from_utf8_lossy(x).into_owned()).collect()
    }

    #[test]
    fn splits_quotes_and_empties() {
        assert_eq!(f("a,b,c", b','), ["a", "b", "c"]);
        assert_eq!(f("a,,c,", b','), ["a", "", "c", ""]);
        assert_eq!(f("\"a,b\",c", b','), ["a,b", "c"]);
        assert_eq!(f("\"he said \"\"hi\"\"\",x", b','), ["he said \"hi\"", "x"]);
        assert_eq!(f("", b','), [""]);
        assert_eq!(f("a\tb", b'\t'), ["a", "b"]);
        assert_eq!(f("\"open,x", b','), ["open,x"]);
    }

    #[test]
    fn letters() {
        assert_eq!(col_letters(0), "A");
        assert_eq!(col_letters(25), "Z");
        assert_eq!(col_letters(26), "AA");
        assert_eq!(col_letters(27), "AB");
        assert_eq!(col_letters(701), "ZZ");
        assert_eq!(col_letters(702), "AAA");
    }

    fn copy(s: &str) -> Copy {
        let mut f = tempfile::tempfile().unwrap();
        use std::io::Write;
        f.write_all(s.as_bytes()).unwrap();
        Copy::from_file(&f).unwrap()
    }

    #[test]
    fn filters_multi_dups_and_custom_separator() {
        let c = copy("n,city,age\nAnn,Paris,30\nBob,Rome,25\nann,Paris,41\nCy,,19\nBob,Rome,70\n");
        let mut t = Table::new(&c, 1, b',', true);
        let f = |col, op: &str, v: &str| Filter { col, op: FOp::parse(op).unwrap(), v: v.into(), v2: String::new(), ci: true, list: vec![] };
        t.set_filters(&c, vec![f(1, "eq", "paris")]);
        assert_eq!(t.base_rows(), vec![0, 2]);
        t.set_filters(&c, vec![f(1, "eq", "paris"), f(2, "gt", "30")]);
        assert_eq!(t.base_rows(), vec![2]);
        assert!(f(2, "gt", "abc").check().is_some());
        t.set_filters(&c, vec![f(1, "blank", "")]);
        assert_eq!(t.base_rows(), vec![3]);
        t.set_filters(&c, vec![]);
        t.find_dups(&c, DupSpec { cols: vec![0, 1], ci: false });
        assert_eq!(t.dup.as_ref().unwrap().rows.len(), 2); // Bob+Rome twice
        t.find_dups(&c, DupSpec { cols: vec![0, 1], ci: true });
        assert_eq!(t.dup.as_ref().unwrap().rows.len(), 4); // plus Ann/ann+Paris
        t.find_dups(&c, DupSpec { cols: vec![1], ci: false });
        assert_eq!(t.dup.as_ref().unwrap().rows.len(), 4); // blank city never counts
        let (d, trunc, total) = distinct(&c, &t, 1, 10);
        assert!(!trunc && total == 5 && d[0].1 == 2);
        assert_eq!(parse_delim("^", &c, "x.txt"), b'^');
        assert_eq!(parse_delim("|", &c, "x.txt"), b'|');
    }

    #[test]
    fn sniffs_and_sorts_and_dups() {
        let c = copy("name;qty\nb;10\na;2\nc;\nb;10\n");
        assert_eq!(sniff(&c), b';');
        let mut t = Table::new(&c, 1, b';', true);
        assert_eq!(t.nrows, 4);
        assert_eq!(t.names, ["name", "qty"]);
        t.set_sort(&c, vec![(1, false, Mode::Auto)]);
        assert_eq!(t.order.as_ref().unwrap().as_slice(), &[1, 0, 3, 2]); // numeric, blank last
        t.set_sort(&c, vec![(1, true, Mode::Auto)]);
        assert_eq!(t.order.as_ref().unwrap().as_slice(), &[0, 3, 1, 2]);
        t.set_sort(&c, vec![(0, false, Mode::Auto)]);
        assert_eq!(t.order.as_ref().unwrap().as_slice(), &[1, 0, 3, 2]);
        t.set_sort(&c, vec![]);
        t.find_dups(&c, DupSpec { cols: vec![], ci: false });
        let d = t.dup.as_ref().unwrap();
        assert_eq!((d.groups, d.rows.len()), (1, 2));
        assert_eq!(t.order.as_ref().unwrap().as_slice(), &[0, 3]);
        t.find_dups(&c, DupSpec { cols: vec![0], ci: true });
        assert_eq!(t.dup.as_ref().unwrap().rows, [0, 3]);
        let s = col_stats(&c, &t, 1);
        assert_eq!((s.blank, s.unique, s.dup_rows), (1, 2, 2));
        assert_eq!(s.numeric.map(|x| x.1), Some(22.0));
    }
}
