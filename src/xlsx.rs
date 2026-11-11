//! Read Excel workbooks (.xlsx / .xlsm) without any extra dependency: a small ZIP reader (flate2 inflates)
//! and a scanner for the few XML parts that hold the data. Each worksheet is converted to CSV text, which the
//! table view then opens like any other CSV (sorting, filter, duplicates, selection counts).
//!
//! Supported: shared and inline strings, numbers, booleans, errors, dates (from the cell's number format),
//! empty rows/columns, several sheets. Not supported: formulas are shown as their last calculated value,
//! cell formatting (colours, percent, currency) is not reproduced, charts/images are ignored, and the old
//! old binary .xls format is not read. Excel binary workbooks (.xlsb) are read by the BIFF12 record reader below.

use anyhow::{Context, Result, bail};
use flate2::read::DeflateDecoder;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

const MAX_PART: u64 = 1 << 30; // 1 GB of inflated XML per part
const MAX_COLS: usize = 16384;

struct Entry {
    method: u16,
    csize: u64,
    usize_: u64,
    offset: u64,
}

pub struct Zip {
    f: File,
    entries: HashMap<String, Entry>,
}

fn u16le(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}
fn u32le(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

impl Zip {
    pub fn open(path: &Path) -> Result<Zip> {
        let mut f = File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
        let len = f.metadata()?.len();
        let tail = len.min(66_000);
        f.seek(SeekFrom::Start(len - tail))?;
        let mut buf = vec![0u8; tail as usize];
        f.read_exact(&mut buf)?;
        let eocd = (0..buf.len().saturating_sub(21)).rev().find(|&i| buf[i..i + 4] == [0x50, 0x4b, 0x05, 0x06]);
        let Some(e) = eocd else { bail!("this is not a valid .xlsx file (no ZIP directory found)") };
        let count = u16le(&buf, e + 10) as usize;
        let cd_size = u32le(&buf, e + 12) as u64;
        let cd_off = u32le(&buf, e + 16) as u64;
        if cd_off == 0xFFFF_FFFF || count == 0xFFFF {
            bail!("ZIP64 workbooks (over 4 GB) are not supported");
        }
        f.seek(SeekFrom::Start(cd_off))?;
        let mut cd = vec![0u8; cd_size as usize];
        f.read_exact(&mut cd)?;
        let mut entries = HashMap::new();
        let mut i = 0;
        while i + 46 <= cd.len() && cd[i..i + 4] == [0x50, 0x4b, 0x01, 0x02] {
            let method = u16le(&cd, i + 10);
            let csize = u32le(&cd, i + 20) as u64;
            let usize_ = u32le(&cd, i + 24) as u64;
            let (n, m, k) = (u16le(&cd, i + 28) as usize, u16le(&cd, i + 30) as usize, u16le(&cd, i + 32) as usize);
            let offset = u32le(&cd, i + 42) as u64;
            if i + 46 + n > cd.len() {
                break;
            }
            let name = String::from_utf8_lossy(&cd[i + 46..i + 46 + n]).replace('\\', "/");
            entries.insert(name, Entry { method, csize, usize_, offset });
            i += 46 + n + m + k;
        }
        if entries.is_empty() {
            bail!("this is not a valid .xlsx file (empty ZIP)");
        }
        Ok(Zip { f, entries })
    }

    pub fn has(&self, name: &str) -> bool {
        self.entries.contains_key(name)
    }

    pub fn read(&mut self, name: &str) -> Result<Vec<u8>> {
        let e = self.entries.get(name).with_context(|| format!("missing part {name}"))?;
        if e.usize_ > MAX_PART {
            bail!("worksheet part {name} is larger than 1 GB when unpacked; too big to open here");
        }
        let (method, csize, usize_, offset) = (e.method, e.csize, e.usize_, e.offset);
        self.f.seek(SeekFrom::Start(offset))?;
        let mut h = [0u8; 30];
        self.f.read_exact(&mut h)?;
        if h[0..4] != [0x50, 0x4b, 0x03, 0x04] {
            bail!("damaged ZIP entry {name}");
        }
        let skip = u16le(&h, 26) as i64 + u16le(&h, 28) as i64;
        self.f.seek(SeekFrom::Current(skip))?;
        let src = (&mut self.f).take(csize);
        let mut out = Vec::with_capacity(usize_.min(1 << 28) as usize);
        match method {
            0 => {
                let mut s = src;
                s.read_to_end(&mut out)?;
            }
            8 => {
                DeflateDecoder::new(src).read_to_end(&mut out).with_context(|| format!("cannot unpack {name}"))?;
            }
            m => bail!("unsupported compression method {m} in {name}"),
        }
        Ok(out)
    }
}

// ───────────── tiny XML helpers (the parts of an .xlsx are machine-written and regular) ─────────────

fn decode_entities(s: &str) -> String {
    if !s.contains('&') && !s.contains("_x") {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'&' {
            if let Some(end) = s[i..].find(';').filter(|&e| e <= 10) {
                let ent = &s[i + 1..i + end];
                let ch = match ent {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    e if e.starts_with("#x") => u32::from_str_radix(&e[2..], 16).ok().and_then(char::from_u32),
                    e if e.starts_with('#') => e[1..].parse::<u32>().ok().and_then(char::from_u32),
                    _ => None,
                };
                if let Some(c) = ch {
                    out.push(c);
                    i += end + 1;
                    continue;
                }
            }
        } else if b[i] == b'_' && i + 7 <= b.len() && &b[i + 1..i + 2] == b"x" && b[i + 6] == b'_' {
            // _x000D_ style escape for control characters
            if let Some(c) = u32::from_str_radix(&s[i + 2..i + 6], 16).ok().and_then(char::from_u32) {
                out.push(c);
                i += 7;
                continue;
            }
        }
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// value of attribute `name` in a tag's text (`<c r="A1" t="s">`)
fn attr(tag: &str, name: &str) -> Option<String> {
    let mut from = 0;
    while let Some(p) = tag[from..].find(name) {
        let at = from + p;
        let before_ok = at == 0 || tag.as_bytes()[at - 1].is_ascii_whitespace();
        let rest = &tag[at + name.len()..];
        if before_ok && rest.trim_start().starts_with('=') {
            let r = rest.trim_start()[1..].trim_start();
            let q = r.chars().next()?;
            if q == '"' || q == '\'' {
                let end = r[1..].find(q)?;
                return Some(decode_entities(&r[1..1 + end]));
            }
        }
        from = at + name.len();
    }
    None
}

/// next `<name ...>` or `<name/>` start at or after `from`: (tag_start, tag_end_exclusive, self_closing)
fn next_tag(s: &str, name: &str, from: usize) -> Option<(usize, usize, bool)> {
    let pat = format!("<{name}");
    let mut p = from;
    while let Some(i) = s[p..].find(&pat) {
        let at = p + i;
        let after = s.as_bytes().get(at + pat.len()).copied().unwrap_or(b'>');
        if matches!(after, b' ' | b'>' | b'/' | b'\t' | b'\n' | b'\r') {
            let end = at + s[at..].find('>')? + 1;
            return Some((at, end, s.as_bytes()[end - 2] == b'/'));
        }
        p = at + pat.len();
    }
    None
}

/// all text of `<t>` elements inside `s` (skipping phonetic runs)
fn texts(s: &str) -> String {
    let mut out = String::new();
    let mut p = 0;
    while let Some((_, end, selfc)) = next_tag(s, "t", p) {
        if selfc {
            p = end;
            continue;
        }
        let close = s[end..].find("</t>").map(|c| end + c).unwrap_or(s.len());
        // inside <rPh>…</rPh> ? (look back for an unclosed rPh)
        let before = &s[..end];
        let in_ph = before.rfind("<rPh").is_some_and(|a| before[a..].find("</rPh>").is_none());
        if !in_ph {
            out.push_str(&decode_entities(&s[end..close]));
        }
        p = close;
    }
    out
}

fn shared_strings(xml: &str) -> Vec<String> {
    let mut v = Vec::new();
    let mut p = 0;
    while let Some((_, end, selfc)) = next_tag(xml, "si", p) {
        if selfc {
            v.push(String::new());
            p = end;
            continue;
        }
        let close = xml[end..].find("</si>").map(|c| end + c).unwrap_or(xml.len());
        v.push(texts(&xml[end..close]));
        p = close;
    }
    v
}

fn builtin_date_fmt(id: u32) -> bool {
    matches!(id, 14..=22 | 27..=36 | 45..=47 | 50..=58)
}

fn custom_is_date(code: &str) -> bool {
    let mut s = String::new();
    let mut skip = false;
    let mut br = false;
    let mut esc = false;
    for c in code.chars() {
        if esc {
            esc = false;
            continue;
        }
        match c {
            '\\' | '_' | '*' => esc = true,
            '"' => skip = !skip,
            '[' if !skip => br = true,
            ']' if !skip => br = false,
            _ if skip || br => {}
            c => s.push(c.to_ascii_lowercase()),
        }
    }
    let s = s.split(';').next().unwrap_or("").to_string();
    !s.contains("general") && s.chars().any(|c| matches!(c, 'd' | 'm' | 'y' | 'h' | 's'))
}

/// per cell-format index (the `s` attribute): is it a date format?
fn date_styles(styles: &str) -> Vec<bool> {
    let mut custom: HashMap<u32, bool> = HashMap::new();
    let mut p = 0;
    while let Some((a, end, _)) = next_tag(styles, "numFmt", p) {
        let tag = &styles[a..end];
        if let (Some(id), Some(code)) = (attr(tag, "numFmtId").and_then(|x| x.parse().ok()), attr(tag, "formatCode")) {
            custom.insert(id, custom_is_date(&code));
        }
        p = end;
    }
    let mut out = Vec::new();
    if let Some((_, start, selfc)) = next_tag(styles, "cellXfs", 0) {
        if !selfc {
            let stop = styles[start..].find("</cellXfs>").map(|e| start + e).unwrap_or(styles.len());
            let body = &styles[start..stop];
            let mut q = 0;
            while let Some((a, end, _)) = next_tag(body, "xf", q) {
                let id: u32 = attr(&body[a..end], "numFmtId").and_then(|x| x.parse().ok()).unwrap_or(0);
                out.push(builtin_date_fmt(id) || custom.get(&id).copied().unwrap_or(false));
                q = end;
            }
        }
    }
    out
}

/// days since 1970-01-01 → (y, m, d)
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn excel_date(serial: f64, date1904: bool) -> String {
    let whole = serial.floor();
    // days since 1970-01-01. Excel's 1900 system wrongly counts 1900-02-29, so serials before 60 are shifted by one day.
    let days = if date1904 { whole as i64 - 24_107 } else if serial >= 60.0 { whole as i64 - 25_569 } else { whole as i64 - 25_568 };
    let (y, m, d) = civil(days);
    let secs = ((serial - whole) * 86_400.0).round() as i64;
    if secs == 0 {
        format!("{y:04}-{m:02}-{d:02}")
    } else {
        format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
    }
}

fn num_text(v: &str) -> String {
    match v.trim().parse::<f64>() {
        Ok(x) if x.is_finite() => {
            if x == x.trunc() && x.abs() < 1e15 {
                format!("{}", x as i64)
            } else {
                // shortest representation that round-trips, so 0.1 stays 0.1 (not 0.10000000000000001)
                format!("{x}")
            }
        }
        _ => v.trim().to_string(),
    }
}

fn csv_field(out: &mut Vec<u8>, s: &str) {
    // the table view works line by line, so a line break inside a cell is shown as a ⏎ marker
    let owned;
    let s = if s.contains(['\n', '\r']) {
        owned = s.replace("\r\n", " ⏎ ").replace(['\n', '\r'], " ⏎ ");
        owned.as_str()
    } else {
        s
    };
    if s.contains([',', '"', '\n', '\r']) || s.starts_with(' ') || s.ends_with(' ') {
        out.push(b'"');
        for b in s.bytes() {
            if b == b'"' {
                out.push(b'"');
            }
            out.push(b);
        }
        out.push(b'"');
    } else {
        out.extend_from_slice(s.as_bytes());
    }
}

fn col_index(r: &str) -> Option<usize> {
    let mut n = 0usize;
    let mut any = false;
    for c in r.bytes().take_while(|b| b.is_ascii_alphabetic()) {
        n = n * 26 + (c.to_ascii_uppercase() - b'A') as usize + 1;
        any = true;
        if n > MAX_COLS + 1 {
            return None;
        }
    }
    any.then(|| n - 1)
}

pub struct Sheet {
    pub name: String,
    pub csv: Vec<u8>,
    pub rows: usize,
    pub cols: usize,
}

pub struct Book {
    zip: Zip,
    pub names: Vec<(String, String)>, // (sheet name, part path)
    strings: Vec<String>,
    dates: Vec<bool>,
    date1904: bool,
    bin: bool,
}

// ───────────── .xlsb: a ZIP of BIFF12 binary records ─────────────

/// next record: (id, payload). id and size use the 7-bits-per-byte encoding of the format.
fn rec<'a>(b: &'a [u8], p: &mut usize) -> Option<(u32, &'a [u8])> {
    let b0 = *b.get(*p)?;
    let mut id = (b0 & 0x7F) as u32;
    *p += 1;
    if b0 & 0x80 != 0 {
        id |= ((*b.get(*p)? & 0x7F) as u32) << 7;
        *p += 1;
    }
    let (mut len, mut sh) = (0usize, 0);
    loop {
        let c = *b.get(*p)?;
        *p += 1;
        len |= ((c & 0x7F) as usize) << sh;
        sh += 7;
        if c & 0x80 == 0 || sh > 28 {
            break;
        }
    }
    let end = p.checked_add(len)?.min(b.len());
    let r = &b[*p..end];
    *p = end;
    Some((id, r))
}

fn le32(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?))
}

/// XLWideString: u32 length in UTF-16 units, then the units (0xFFFFFFFF = none)
fn wstr(b: &[u8], i: usize) -> Option<(String, usize)> {
    let n = le32(b, i)?;
    if n == u32::MAX {
        return Some((String::new(), i + 4));
    }
    let n = n as usize;
    let raw = b.get(i + 4..i + 4 + n.checked_mul(2)?)?;
    let u: Vec<u16> = raw.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    Some((String::from_utf16_lossy(&u), i + 4 + n * 2))
}

fn rk_value(rk: u32) -> f64 {
    let v = if rk & 2 != 0 { ((rk as i32) >> 2) as f64 } else { f64::from_bits(((rk & 0xFFFF_FFFC) as u64) << 32) };
    if rk & 1 != 0 { v / 100.0 } else { v }
}

fn err_text(c: u8) -> &'static str {
    match c {
        0x00 => "#NULL!",
        0x07 => "#DIV/0!",
        0x0F => "#VALUE!",
        0x17 => "#REF!",
        0x1D => "#NAME?",
        0x24 => "#NUM!",
        0x2A => "#N/A",
        _ => "#ERROR!",
    }
}

impl Book {
    fn open_bin(mut zip: Zip) -> Result<Book> {
        let wb = zip.read("xl/workbook.bin")?;
        let rels = if zip.has("xl/_rels/workbook.bin.rels") { String::from_utf8_lossy(&zip.read("xl/_rels/workbook.bin.rels")?).into_owned() } else { String::new() };
        let mut targets: HashMap<String, String> = HashMap::new();
        let mut p = 0;
        while let Some((a, end, _)) = next_tag(&rels, "Relationship", p) {
            let t = &rels[a..end];
            if let (Some(id), Some(target)) = (attr(t, "Id"), attr(t, "Target")) {
                let full = if let Some(abs) = target.strip_prefix('/') { abs.to_string() } else { format!("xl/{}", target.trim_start_matches("./")) };
                targets.insert(id, full);
            }
            p = end;
        }
        let (mut names, mut date1904, mut n, mut pos) = (Vec::new(), false, 0, 0usize);
        while let Some((id, d)) = rec(&wb, &mut pos) {
            match id {
                153 => date1904 = le32(d, 0).is_some_and(|f| f & 1 != 0),   // BrtWbProp
                156 => {                                                    // BrtBundleSh
                    n += 1;
                    let state = le32(d, 0).unwrap_or(0);
                    let Some((rid, i)) = wstr(d, 8) else { continue };
                    let name = wstr(d, i).map(|x| x.0).filter(|x| !x.is_empty()).unwrap_or_else(|| format!("Sheet{n}"));
                    let part = targets.get(&rid).cloned().unwrap_or_else(|| format!("xl/worksheets/sheet{n}.bin"));
                    if state != 2 {
                        names.push((name, part));
                    }
                }
                _ => {}
            }
        }
        if names.is_empty() {
            bail!("this workbook has no worksheets");
        }
        let mut strings = Vec::new();
        if zip.has("xl/sharedStrings.bin") {
            let b = zip.read("xl/sharedStrings.bin")?;
            let mut pos = 0;
            while let Some((id, d)) = rec(&b, &mut pos) {
                if id == 19 {
                    strings.push(wstr(d, 1).map(|x| x.0).unwrap_or_default());   // BrtSSTItem: flags byte, then the text
                }
            }
        }
        let mut dates = Vec::new();
        if zip.has("xl/styles.bin") {
            let b = zip.read("xl/styles.bin")?;
            let (mut pos, mut custom, mut in_xfs) = (0usize, HashMap::<u32, bool>::new(), false);
            while let Some((id, d)) = rec(&b, &mut pos) {
                match id {
                    44 => {                                                // BrtFmt
                        if let (Some(f), Some((code, _))) = (d.get(0..2).map(|x| u16::from_le_bytes([x[0], x[1]]) as u32), wstr(d, 2)) {
                            custom.insert(f, custom_is_date(&code));
                        }
                    }
                    617 => in_xfs = true,                                  // BrtBeginCellXFs
                    618 => in_xfs = false,
                    47 if in_xfs => {                                      // BrtXF: ixfeParent u16, numFmtId u16, ...
                        let f = d.get(2..4).map(|x| u16::from_le_bytes([x[0], x[1]]) as u32).unwrap_or(0);
                        dates.push(builtin_date_fmt(f) || custom.get(&f).copied().unwrap_or(false));
                    }
                    _ => {}
                }
            }
        }
        Ok(Book { zip, names, strings, dates, date1904, bin: true })
    }

    fn sheet_bin(&mut self, i: usize) -> Result<Sheet> {
        let (name, part) = self.names[i].clone();
        let b = self.zip.read(&part)?;
        let mut csv: Vec<u8> = Vec::with_capacity(b.len());
        let (mut next_row, mut max_cols, mut written, mut pos) = (0usize, 0usize, 0usize, 0usize);
        let mut cells: Vec<String> = Vec::new();
        let mut open = false;
        let flush = |csv: &mut Vec<u8>, cells: &mut Vec<String>, max_cols: &mut usize, next_row: &mut usize, written: &mut usize, open: &mut bool| {
            if !*open {
                return;
            }
            *max_cols = (*max_cols).max(cells.len());
            for (k, c) in cells.iter().enumerate() {
                if k > 0 {
                    csv.push(b',');
                }
                csv_field(csv, c);
            }
            csv.push(b'\n');
            cells.clear();
            *next_row += 1;
            *written += 1;
            *open = false;
        };
        while let Some((id, d)) = rec(&b, &mut pos) {
            if id == 0 {                                                    // BrtRowHdr
                flush(&mut csv, &mut cells, &mut max_cols, &mut next_row, &mut written, &mut open);
                let rnum = le32(d, 0).unwrap_or(next_row as u32) as usize;
                while next_row < rnum && next_row < 1_048_576 {
                    csv.push(b'\n');
                    next_row += 1;
                    written += 1;
                }
                open = true;
                if written >= 1_048_576 {
                    break;
                }
                continue;
            }
            if !(1..=11).contains(&id) || !open {
                continue;
            }
            let col = le32(d, 0).unwrap_or(0) as usize;
            let style = (le32(d, 4).unwrap_or(0) & 0x00FF_FFFF) as usize;
            let is_date = self.dates.get(style).copied().unwrap_or(false);
            let num = |x: f64| if is_date { excel_date(x, self.date1904) } else { num_text(&format!("{x}")) };
            let f64at = |o: usize| d.get(o..o + 8).map(|x| f64::from_le_bytes(x.try_into().unwrap()));
            let text = match id {
                2 => le32(d, 8).map(|r| num(rk_value(r))).unwrap_or_default(),
                3 | 11 => d.get(8).map(|&c| err_text(c).to_string()).unwrap_or_default(),
                4 | 10 => if d.get(8) == Some(&1) { "TRUE".into() } else { "FALSE".into() },
                5 | 9 => f64at(8).map(num).unwrap_or_default(),
                6 | 8 => wstr(d, 8).map(|x| x.0).unwrap_or_default(),
                7 => le32(d, 8).and_then(|k| self.strings.get(k as usize).cloned()).unwrap_or_default(),
                _ => String::new(),
            };
            if col < MAX_COLS {
                if cells.len() <= col {
                    cells.resize(col + 1, String::new());
                }
                cells[col] = text;
            }
        }
        flush(&mut csv, &mut cells, &mut max_cols, &mut next_row, &mut written, &mut open);
        while csv.ends_with(b"\n\n") {
            csv.pop();
        }
        Ok(Sheet { name, rows: written, cols: max_cols, csv })
    }
}

impl Book {
    pub fn open(path: &Path) -> Result<Book> {
        let mut zip = Zip::open(path)?;
        if !zip.has("xl/workbook.xml") && zip.has("xl/workbook.bin") {
            return Book::open_bin(zip);
        }
        if !zip.has("xl/workbook.xml") {
            if zip.has("EncryptedPackage") || zip.has("EncryptionInfo") {
                bail!("this workbook is password-protected; remove the password in Excel first");
            }
            bail!("this ZIP file is not an Excel workbook (no xl/workbook.xml)");
        }
        let wb = String::from_utf8_lossy(&zip.read("xl/workbook.xml")?).into_owned();
        let rels = if zip.has("xl/_rels/workbook.xml.rels") { String::from_utf8_lossy(&zip.read("xl/_rels/workbook.xml.rels")?).into_owned() } else { String::new() };
        let mut targets: HashMap<String, String> = HashMap::new();
        let mut p = 0;
        while let Some((a, end, _)) = next_tag(&rels, "Relationship", p) {
            let t = &rels[a..end];
            if let (Some(id), Some(target)) = (attr(t, "Id"), attr(t, "Target")) {
                let full = if let Some(abs) = target.strip_prefix('/') { abs.to_string() } else { format!("xl/{}", target.trim_start_matches("./")) };
                targets.insert(id, full);
            }
            p = end;
        }
        let mut names = Vec::new();
        let mut p = 0;
        let mut n = 0;
        while let Some((a, end, _)) = next_tag(&wb, "sheet", p) {
            n += 1;
            let t = &wb[a..end];
            let name = attr(t, "name").unwrap_or_else(|| format!("Sheet{n}"));
            let rid = attr(t, "r:id").or_else(|| attr(t, "id"));
            let part = rid.and_then(|r| targets.get(&r).cloned()).unwrap_or_else(|| format!("xl/worksheets/sheet{n}.xml"));
            if attr(t, "state").as_deref() != Some("veryHidden") {
                names.push((name, part));
            }
            p = end;
        }
        if names.is_empty() {
            bail!("this workbook has no worksheets");
        }
        let strings = if zip.has("xl/sharedStrings.xml") { shared_strings(&String::from_utf8_lossy(&zip.read("xl/sharedStrings.xml")?)) } else { Vec::new() };
        let dates = if zip.has("xl/styles.xml") { date_styles(&String::from_utf8_lossy(&zip.read("xl/styles.xml")?)) } else { Vec::new() };
        let date1904 = next_tag(&wb, "workbookPr", 0).is_some_and(|(a, e, _)| attr(&wb[a..e], "date1904").is_some_and(|v| v == "1" || v == "true"));
        Ok(Book { zip, names, strings, dates, date1904, bin: false })
    }

    pub fn sheet(&mut self, i: usize) -> Result<Sheet> {
        if self.bin {
            return self.sheet_bin(i);
        }
        let (name, part) = self.names[i].clone();
        let xml_b = self.zip.read(&part)?;
        let xml = String::from_utf8_lossy(&xml_b);
        let mut csv: Vec<u8> = Vec::with_capacity(xml.len() / 3);
        let (mut next_row, mut max_cols, mut written) = (0usize, 0usize, 0usize);
        let mut p = 0;
        let mut cells: Vec<String> = Vec::new();
        while let Some((a, end, selfc)) = next_tag(&xml, "row", p) {
            let rtag = &xml[a..end];
            let rnum = attr(rtag, "r").and_then(|x| x.parse::<usize>().ok()).map(|r| r - 1).unwrap_or(next_row);
            let (body, after) = if selfc { ("", end) } else {
                let c = xml[end..].find("</row>").map(|c| end + c).unwrap_or(xml.len());
                (&xml[end..c], c)
            };
            while next_row < rnum && next_row < 1_048_576 {
                csv.push(b'\n');
                next_row += 1;
                written += 1;
            }
            cells.clear();
            let mut q = 0;
            while let Some((ca, cend, cself)) = next_tag(body, "c", q) {
                let ctag = &body[ca..cend];
                let col = attr(ctag, "r").and_then(|r| col_index(&r)).unwrap_or(cells.len());
                let (inner, cnext) = if cself { ("", cend) } else {
                    let c = body[cend..].find("</c>").map(|c| cend + c).unwrap_or(body.len());
                    (&body[cend..c], c)
                };
                let t = attr(ctag, "t").unwrap_or_default();
                let style: usize = attr(ctag, "s").and_then(|x| x.parse().ok()).unwrap_or(0);
                let val = |s: &str| -> String {
                    match next_tag(s, "v", 0) {
                        Some((_, e, false)) => decode_entities(&s[e..s[e..].find("</v>").map(|x| e + x).unwrap_or(s.len())]),
                        _ => String::new(),
                    }
                };
                let text = match t.as_str() {
                    "s" => val(inner).trim().parse::<usize>().ok().and_then(|i| self.strings.get(i).cloned()).unwrap_or_default(),
                    "inlineStr" => texts(inner),
                    "b" => if val(inner).trim() == "1" { "TRUE".into() } else { "FALSE".into() },
                    "str" | "e" | "d" => val(inner),
                    _ => {
                        let v = val(inner);
                        if !v.trim().is_empty() && self.dates.get(style).copied().unwrap_or(false) {
                            v.trim().parse::<f64>().map(|x| excel_date(x, self.date1904)).unwrap_or(v)
                        } else {
                            num_text(&v)
                        }
                    }
                };
                if col < MAX_COLS {
                    if cells.len() <= col {
                        cells.resize(col + 1, String::new());
                    }
                    cells[col] = text;
                }
                q = cnext;
            }
            max_cols = max_cols.max(cells.len());
            for (i, c) in cells.iter().enumerate() {
                if i > 0 {
                    csv.push(b',');
                }
                csv_field(&mut csv, c);
            }
            csv.push(b'\n');
            next_row += 1;
            written += 1;
            p = after;
            if written >= 1_048_576 {
                break;
            }
        }
        // trailing empty lines carry no information
        while csv.ends_with(b"\n\n") {
            csv.pop();
        }
        Ok(Sheet { name, rows: written, cols: max_cols, csv })
    }
}

/// worksheet → temp file (so the normal memory-mapped table engine can use it)
pub fn sheet_to_file(s: &Sheet) -> Result<File> {
    let mut f = tempfile::tempfile().context("cannot create a temp file")?;
    f.write_all(&s.csv)?;
    f.flush()?;
    Ok(f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_and_numbers() {
        assert_eq!(excel_date(45000.0, false), "2023-03-15");
        assert_eq!(excel_date(1.0, false), "1900-01-01");
        assert_eq!(excel_date(61.0, false), "1900-03-01");
        assert_eq!(excel_date(44197.5, false), "2021-01-01 12:00:00");
        assert_eq!(excel_date(0.0, true), "1904-01-01");
        assert_eq!(num_text("3.0"), "3");
        assert_eq!(num_text("0.10000000000000001"), "0.1");
        assert_eq!(col_index("AB12"), Some(27));
        assert!(custom_is_date("yyyy\\-mm\\-dd"));
        assert!(custom_is_date("[$-409]d-mmm-yy;@"));
        assert!(!custom_is_date("0.00\"d\""));
        assert!(!custom_is_date("General"));
    }

    #[test]
    fn strings_and_entities() {
        let v = shared_strings("<sst><si><t>a &amp; b</t></si><si><r><t>x</t></r><r><t xml:space=\"preserve\"> y</t></r><rPh><t>ph</t></rPh></si><si/></sst>");
        assert_eq!(v, vec!["a & b", "x y", ""]);
        assert_eq!(decode_entities("_x000D_a&#x1F600;"), "\ra😀");
    }
}
