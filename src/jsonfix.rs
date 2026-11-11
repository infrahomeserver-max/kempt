//! Tolerant, streaming JSON repair.
//!
//! Reads "almost JSON" and writes valid, formatted JSON, reporting what it fixed.
//! Handles: comments, trailing/missing/duplicate commas, single-quoted strings,
//! unquoted keys and values, Python/JS literals (True/None/NaN/undefined),
//! hex/`+5`/`.5` numbers, raw control characters in strings, missing colons or
//! values, mismatched / missing / stray brackets, unterminated strings, BOM.
//! Constant memory: works byte by byte and pipes through the streaming formatter.

use crate::jsonfmt::Fmt;
use anyhow::{Result, bail};
use std::collections::BTreeMap;
use std::io::{Read, Write};

pub type Report = BTreeMap<&'static str, u64>;

/// longest string value held back to test whether it is embedded JSON (longer ones are copied through)
const EMBED_MAX: usize = 32 << 20;

#[derive(Clone, Copy, PartialEq, Debug)]
enum St {
    Value,
    ValueOrEnd,
    KeyOrEnd,
    Key,
    Colon,
    CommaOrEnd,
}

#[derive(Clone, Copy, PartialEq)]
enum Role {
    Key,
    Value,
}

#[derive(Clone, Copy, PartialEq)]
enum Lex {
    Normal,
    Str { quote: u8, esc: bool },
    Bare,
    Slash,
    LineComment,
    BlockComment { star: bool },
}

struct Fix<W: Write> {
    fmt: Fmt<W>,
    buf: Vec<u8>,
    sent: u64,
    stack: Vec<u8>,
    st: St,
    pending_comma: bool,
    after_colon: bool,
    lex: Lex,
    role: Role,
    bare: Vec<u8>,
    top_values: u64,
    report: Report,
    /// levels of embedded JSON text (a string value that holds a JSON document) still allowed to expand; 0 = off
    expand: u8,
    sbuf: Option<Vec<u8>>,
}

fn bump(r: &mut Report, k: &'static str) {
    *r.entry(k).or_insert(0) += 1;
}

impl<W: Write> Fix<W> {
    fn new(w: W, indent: Option<usize>) -> Self {
        Fix {
            fmt: Fmt::new(w, indent),
            buf: Vec::with_capacity(1 << 16),
            sent: 0,
            stack: Vec::new(),
            st: St::Value,
            pending_comma: false,
            after_colon: false,
            lex: Lex::Normal,
            role: Role::Value,
            bare: Vec::new(),
            top_values: 0,
            report: Report::new(),
            expand: 0,
            sbuf: None,
        }
    }

    /// string bytes: held back while a value string might turn out to be embedded JSON
    fn semit(&mut self, b: &[u8]) -> Result<()> {
        if let Some(sb) = self.sbuf.as_mut() {
            sb.extend_from_slice(b);
            if sb.len() > EMBED_MAX {
                let sb = self.sbuf.take().unwrap();
                self.emit(&sb)?;
            }
            return Ok(());
        }
        self.emit(b)
    }

    fn emit(&mut self, b: &[u8]) -> Result<()> {
        self.buf.extend_from_slice(b);
        if self.buf.len() >= 1 << 16 {
            self.flush_buf()?;
        }
        Ok(())
    }

    fn flush_buf(&mut self) -> Result<()> {
        if !self.buf.is_empty() {
            let b = std::mem::take(&mut self.buf);
            if let Err(e) = self.fmt.feed(&b, self.sent) {
                bail!("internal repair error: {e}");
            }
            self.sent += b.len() as u64;
            self.buf = b;
            self.buf.clear();
        }
        Ok(())
    }

    fn value_done(&mut self) {
        self.after_colon = false;
        if self.stack.is_empty() {
            self.top_values += 1;
            self.st = St::Value;
        } else {
            self.st = St::CommaOrEnd;
        }
    }

    /// Called when a key/value token begins. Fixes separators; returns the token's role.
    fn begin(&mut self, off: u64, opens_container: bool) -> Result<Role> {
        match self.st {
            St::CommaOrEnd => {
                bump(&mut self.report, "missing commas added");
                self.emit(b",")?;
                self.st = if self.stack.last() == Some(&b'{') { St::Key } else { St::Value };
            }
            St::Colon => {
                bump(&mut self.report, "missing colons added");
                self.emit(b":")?;
                self.st = St::Value;
                self.after_colon = true;
            }
            _ => {}
        }
        if self.pending_comma {
            self.pending_comma = false;
            self.emit(b",")?;
        }
        if self.stack.is_empty() && self.top_values > 0 {
            self.emit(b"\n")?;
        }
        let role = if matches!(self.st, St::Key | St::KeyOrEnd) { Role::Key } else { Role::Value };
        if role == Role::Key && opens_container {
            bail!("cannot repair: a '{{' or '[' appears where an object key was expected (byte {off})");
        }
        Ok(role)
    }

    fn after_key(&mut self) {
        self.st = St::Colon;
    }

    fn finish_value_slot(&mut self) -> Result<()> {
        // a key without value / colon without value → null
        if self.st == St::Colon {
            bump(&mut self.report, "missing values set to null");
            self.emit(b":null")?;
            self.value_done();
        } else if self.after_colon {
            bump(&mut self.report, "missing values set to null");
            self.emit(b"null")?;
            self.value_done();
        }
        Ok(())
    }

    fn open(&mut self, b: u8, off: u64) -> Result<()> {
        self.begin(off, true)?;
        self.after_colon = false;
        self.emit(&[b])?;
        self.stack.push(b);
        self.st = if b == b'{' { St::KeyOrEnd } else { St::ValueOrEnd };
        Ok(())
    }

    fn close_top(&mut self) -> Result<()> {
        self.finish_value_slot()?;
        if self.pending_comma {
            self.pending_comma = false;
            bump(&mut self.report, "trailing commas removed");
        }
        let top = self.stack.pop().unwrap();
        self.emit(if top == b'{' { b"}" } else { b"]" })?;
        self.value_done();
        Ok(())
    }

    fn close(&mut self, b: u8) -> Result<()> {
        let want = if b == b'}' { b'{' } else { b'[' };
        match self.stack.iter().rposition(|&c| c == want) {
            None => {
                bump(&mut self.report, "stray closing brackets removed");
            }
            Some(k) => {
                let depth_to_close = self.stack.len() - 1 - k;
                if depth_to_close > 0 || *self.stack.last().unwrap() != want {
                    bump(&mut self.report, "missing/mismatched closing brackets fixed");
                }
                for _ in 0..depth_to_close {
                    self.close_top()?;
                }
                self.close_top()?;
            }
        }
        Ok(())
    }

    fn comma(&mut self) {
        if self.st == St::CommaOrEnd {
            self.pending_comma = true;
            self.st = if self.stack.last() == Some(&b'{') { St::Key } else { St::Value };
        } else {
            bump(&mut self.report, "extra commas removed");
        }
    }

    fn colon(&mut self) -> Result<()> {
        if self.st == St::Colon {
            self.emit(b":")?;
            self.st = St::Value;
            self.after_colon = true;
        } else {
            bump(&mut self.report, "stray colons removed");
        }
        Ok(())
    }

    // ---- strings
    fn str_start(&mut self, quote: u8, off: u64) -> Result<()> {
        self.role = self.begin(off, false)?;
        self.after_colon = false;
        if quote == b'\'' {
            bump(&mut self.report, "single-quoted strings converted");
        }
        if self.expand > 0 && self.role == Role::Value {
            self.sbuf = Some(vec![b'"']);
        } else {
            self.emit(b"\"")?;
        }
        self.lex = Lex::Str { quote, esc: false };
        Ok(())
    }

    fn str_end(&mut self) -> Result<()> {
        if let Some(mut sb) = self.sbuf.take() {
            sb.push(b'"');
            match self.embedded(&sb) {
                Some(json) => {
                    bump(&mut self.report, "embedded JSON text expanded");
                    self.emit(&json)?;
                }
                None => self.emit(&sb)?,
            }
        } else {
            self.emit(b"\"")?;
        }
        self.lex = Lex::Normal;
        if self.role == Role::Key {
            self.after_key();
        } else {
            self.value_done();
        }
        Ok(())
    }

    /// `sb` is a complete JSON string literal. When its text is itself a JSON object or array, return that text, compact
    /// (with any JSON inside it expanded the same way), so the formatter lays it out as real structure.
    fn embedded(&self, sb: &[u8]) -> Option<Vec<u8>> {
        if sb.len() < 5 {
            return None;
        }
        let text: String = serde_json::from_slice(sb).ok()?;
        let t = text.trim();
        if !((t.starts_with('{') && t.ends_with('}')) || (t.starts_with('[') && t.ends_with(']'))) {
            return None;
        }
        if serde_json::from_str::<serde_json::Value>(t).is_err() {
            return None;
        }
        let mut out = Vec::with_capacity(t.len());
        let mut sub = Fix::new(&mut out, None);
        sub.expand = self.expand - 1;
        sub.feed(t.as_bytes(), 0).ok()?;
        sub.finish(t.len() as u64).ok()?;
        drop(sub);
        Some(out)
    }

    fn str_byte(&mut self, quote: u8, esc: bool, b: u8) -> Result<()> {
        if esc {
            self.lex = Lex::Str { quote, esc: false };
            match b {
                b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' | b'u' => self.semit(&[b'\\', b])?,
                b'\'' => self.semit(b"'")?,
                b'\n' => {} // line continuation
                _ => {
                    bump(&mut self.report, "invalid escapes fixed");
                    self.semit(&[b'\\', b'\\', b])?;
                }
            }
            return Ok(());
        }
        match b {
            b'\\' => self.lex = Lex::Str { quote, esc: true },
            q if q == quote => self.str_end()?,
            b'"' => self.semit(b"\\\"")?, // inside a single-quoted string
            b'\n' => {
                bump(&mut self.report, "raw control characters escaped");
                self.semit(b"\\n")?;
            }
            b'\r' => {
                bump(&mut self.report, "raw control characters escaped");
                self.semit(b"\\r")?;
            }
            b'\t' => {
                bump(&mut self.report, "raw control characters escaped");
                self.semit(b"\\t")?;
            }
            c if c < 0x20 => {
                bump(&mut self.report, "raw control characters escaped");
                self.semit(format!("\\u{:04x}", c).as_bytes())?;
            }
            c => self.semit(&[c])?,
        }
        Ok(())
    }

    // ---- bare tokens
    fn bare_start(&mut self, b: u8, off: u64) -> Result<()> {
        self.role = self.begin(off, false)?;
        self.after_colon = false;
        self.bare.clear();
        self.bare.push(b);
        self.lex = Lex::Bare;
        Ok(())
    }

    fn quote_bytes(raw: &[u8]) -> Vec<u8> {
        let mut o = Vec::with_capacity(raw.len() + 2);
        o.push(b'"');
        for &c in raw {
            match c {
                b'"' => o.extend_from_slice(b"\\\""),
                b'\\' => o.extend_from_slice(b"\\\\"),
                c if c < 0x20 => o.extend_from_slice(format!("\\u{:04x}", c).as_bytes()),
                c => o.push(c),
            }
        }
        o.push(b'"');
        o
    }

    fn normalise_number(t: &[u8]) -> Option<Vec<u8>> {
        let s = std::str::from_utf8(t).ok()?;
        let (neg, body) = match s.strip_prefix('-') {
            Some(r) => (true, r),
            None => (false, s.strip_prefix('+').unwrap_or(s)),
        };
        if body.is_empty() {
            return None;
        }
        let mut out = String::new();
        if neg {
            out.push('-');
        }
        if let Some(h) = body.strip_prefix("0x").or_else(|| body.strip_prefix("0X")) {
            let v = u128::from_str_radix(h, 16).ok()?;
            out.push_str(&v.to_string());
            return Some(out.into_bytes());
        }
        // split mantissa / exponent
        let (mant, exp) = match body.find(['e', 'E']) {
            Some(i) => (&body[..i], Some(&body[i + 1..])),
            None => (body, None),
        };
        let (ip, fp) = match mant.find('.') {
            Some(i) => (&mant[..i], Some(&mant[i + 1..])),
            None => (mant, None),
        };
        if !ip.chars().all(|c| c.is_ascii_digit()) || fp.is_some_and(|f| !f.chars().all(|c| c.is_ascii_digit())) {
            return None;
        }
        if ip.is_empty() && fp.is_none_or(|f| f.is_empty()) {
            return None;
        }
        let ip_trim = ip.trim_start_matches('0');
        out.push_str(if ip_trim.is_empty() { "0" } else { ip_trim });
        if let Some(f) = fp {
            if !f.is_empty() {
                out.push('.');
                out.push_str(f);
            }
        }
        if let Some(e) = exp {
            let (sign, digits) = match e.strip_prefix('-') {
                Some(r) => ("-", r),
                None => ("", e.strip_prefix('+').unwrap_or(e)),
            };
            if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            out.push('e');
            out.push_str(sign);
            out.push_str(digits);
        }
        Some(out.into_bytes())
    }

    fn bare_end(&mut self) -> Result<()> {
        self.lex = Lex::Normal;
        let t = std::mem::take(&mut self.bare);
        if self.role == Role::Key {
            if !t.is_empty() {
                bump(&mut self.report, "unquoted keys quoted");
            }
            let q = Self::quote_bytes(&t);
            self.emit(&q)?;
            self.after_key();
            return Ok(());
        }
        let lit: Option<&[u8]> = match t.as_slice() {
            b"true" | b"false" | b"null" => Some(&t),
            b"True" | b"TRUE" => Some(b"true"),
            b"False" | b"FALSE" => Some(b"false"),
            b"None" | b"NULL" | b"Null" | b"nil" | b"undefined" => Some(b"null"),
            b"NaN" | b"nan" | b"Infinity" | b"-Infinity" | b"+Infinity" | b"inf" | b"-inf" => Some(b"null"),
            _ => None,
        };
        if let Some(l) = lit {
            if l != t.as_slice() {
                bump(&mut self.report, "non-JSON literals converted (True/None/NaN/...)");
            }
            let l = l.to_vec();
            self.emit(&l)?;
        } else if let Some(n) = Self::normalise_number(&t) {
            if n != t {
                bump(&mut self.report, "numbers normalised (+5, .5, 0x1F, 007)");
            }
            self.emit(&n)?;
        } else {
            bump(&mut self.report, "unquoted strings quoted");
            let q = Self::quote_bytes(&t);
            self.emit(&q)?;
        }
        self.value_done();
        Ok(())
    }

    fn is_delim(b: u8) -> bool {
        matches!(b, b' ' | b'\t' | b'\n' | b'\r' | b',' | b':' | b'{' | b'}' | b'[' | b']' | b'"' | b'\'' | b'/')
    }

    fn feed(&mut self, chunk: &[u8], base: u64) -> Result<()> {
        let mut i = 0;
        while i < chunk.len() {
            let b = chunk[i];
            let off = base + i as u64;
            match self.lex {
                Lex::Str { quote, esc } => {
                    self.str_byte(quote, esc, b)?;
                    i += 1;
                }
                Lex::LineComment => {
                    if b == b'\n' {
                        self.lex = Lex::Normal;
                    }
                    i += 1;
                }
                Lex::BlockComment { star } => {
                    if star && b == b'/' {
                        self.lex = Lex::Normal;
                    } else {
                        self.lex = Lex::BlockComment { star: b == b'*' };
                    }
                    i += 1;
                }
                Lex::Slash => {
                    if b == b'/' {
                        bump(&mut self.report, "comments removed");
                        self.lex = Lex::LineComment;
                        i += 1;
                    } else if b == b'*' {
                        bump(&mut self.report, "comments removed");
                        self.lex = Lex::BlockComment { star: false };
                        i += 1;
                    } else {
                        // a lone '/' starts a bare token; reprocess current byte
                        self.bare_start(b'/', off)?;
                    }
                }
                Lex::Bare => {
                    if Self::is_delim(b) {
                        self.bare_end()?;
                        // reprocess delimiter in Normal state
                    } else {
                        if self.bare.len() > (1 << 20) {
                            bail!("token too long at byte {off}");
                        }
                        self.bare.push(b);
                        i += 1;
                    }
                }
                Lex::Normal => {
                    match b {
                        b' ' | b'\t' | b'\n' | b'\r' => {}
                        b'"' | b'\'' => self.str_start(b, off)?,
                        b'{' | b'[' => self.open(b, off)?,
                        b'}' | b']' => self.close(b)?,
                        b',' => {
                            self.finish_value_slot()?;
                            self.comma();
                        }
                        b':' => self.colon()?,
                        b'/' => self.lex = Lex::Slash,
                        _ => self.bare_start(b, off)?,
                    }
                    i += 1;
                }
            }
        }
        Ok(())
    }

    fn finish(&mut self, total: u64) -> Result<()> {
        match self.lex {
            Lex::Str { .. } => {
                bump(&mut self.report, "unterminated strings closed");
                self.str_end()?;
            }
            Lex::Bare => self.bare_end()?,
            Lex::Slash => {
                self.bare_start(b'/', total)?;
                self.bare_end()?;
            }
            _ => {}
        }
        if !self.stack.is_empty() {
            bump(&mut self.report, "missing/mismatched closing brackets fixed");
        }
        self.pending_comma = false;
        while !self.stack.is_empty() {
            self.close_top()?;
        }
        self.finish_value_slot()?;
        self.flush_buf()?;
        self.fmt.finish(self.sent).map_err(|e| anyhow::anyhow!("internal repair error: {e}"))
    }
}

/// Repair + format. `indent = None` gives compact output.
#[allow(dead_code)]
pub fn fix<R: Read, W: Write>(r: R, w: W, indent: Option<usize>) -> Result<Report> {
    fix_with(r, w, indent, 0)
}

/// Like `fix`, and also: a document that is itself escaped JSON text (`{\"a\":1}` or `"{\"a\":1}"`) is unescaped first, and a
/// string value that holds a JSON object or array (up to 3 levels deep) becomes real nested structure.
pub fn fix_deep(bytes: &[u8], w: impl Write, indent: Option<usize>) -> Result<Report> {
    let mut rep = Report::new();
    let mut cur: std::borrow::Cow<[u8]> = std::borrow::Cow::Borrowed(bytes);
    for _ in 0..2 {
        match unescape_document(&cur) {
            Some(u) => {
                bump(&mut rep, "escaped JSON text unescaped");
                cur = std::borrow::Cow::Owned(u);
            }
            None => break,
        }
    }
    let more = fix_with(&cur[..], w, indent, 3)?;
    for (k, v) in more {
        *rep.entry(k).or_insert(0) += v;
    }
    Ok(rep)
}

/// Text that is the *inside* of a JSON string (backslash before every quote), or a whole quoted JSON string whose text is a
/// JSON object or array, is turned into the JSON it contains. None when the document is not like that.
fn unescape_document(b: &[u8]) -> Option<Vec<u8>> {
    if b.len() > (1 << 29) {
        return None;
    }
    let start = b.iter().position(|c| !c.is_ascii_whitespace())?;
    let mut body = &b[start..];
    while let Some((l, rest)) = body.split_last() {
        if l.is_ascii_whitespace() {
            body = rest;
        } else {
            break;
        }
    }
    let quoted = body.len() >= 2 && body[0] == b'"' && body[body.len() - 1] == b'"';
    let inner = if quoted { &body[1..body.len() - 1] } else { body };
    let first = *inner.iter().find(|c| !c.is_ascii_whitespace())?;
    if first != b'{' && first != b'[' {
        return None;
    }
    // escaped means: the first quote that appears is preceded by a backslash
    let q = inner.iter().position(|&c| c == b'"')?;
    if q == 0 || inner[q - 1] != b'\\' {
        return None;
    }
    let mut out = Vec::with_capacity(inner.len());
    let mut i = 0;
    while i < inner.len() {
        let c = inner[i];
        if c == b'\\' && i + 1 < inner.len() {
            i += 1;
            match inner[i] {
                b'"' => out.push(b'"'),
                b'\\' => out.push(b'\\'),
                b'/' => out.push(b'/'),
                b'n' => out.push(b'\n'),
                b'r' => out.push(b'\r'),
                b't' => out.push(b'\t'),
                b'b' => out.push(8),
                b'f' => out.push(12),
                b'u' if i + 4 < inner.len() => {
                    let hex = std::str::from_utf8(&inner[i + 1..i + 5]).ok().and_then(|h| u32::from_str_radix(h, 16).ok());
                    match hex.and_then(char::from_u32) {
                        Some(ch) => {
                            let mut tmp = [0u8; 4];
                            out.extend_from_slice(ch.encode_utf8(&mut tmp).as_bytes());
                            i += 4;
                        }
                        None => out.extend_from_slice(b"\\u"),
                    }
                }
                o => {
                    out.push(b'\\');
                    out.push(o);
                }
            }
        } else {
            out.push(c);
        }
        i += 1;
    }
    Some(out)
}

fn fix_with<R: Read, W: Write>(mut r: R, w: W, indent: Option<usize>, expand: u8) -> Result<Report> {
    let mut f = Fix::new(w, indent);
    f.expand = expand;
    let mut buf = vec![0u8; 1 << 20];
    let mut base = 0u64;
    let mut first = true;
    loop {
        let n = match r.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        };
        let mut chunk = &buf[..n];
        let mut off = base;
        if first {
            first = false;
            if chunk.starts_with(&[0xEF, 0xBB, 0xBF]) {
                chunk = &chunk[3..];
                off += 3;
            }
        }
        // count top-level values as they complete so separators are inserted
        f.feed(chunk, off)?;
        base += n as u64;
    }
    f.finish(base)?;
    Ok(f.report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fx(s: &str) -> (String, Report) {
        let mut out = Vec::new();
        let r = fix(s.as_bytes(), &mut out, None).unwrap();
        (String::from_utf8(out).unwrap().trim_end().to_string(), r)
    }

    fn ok(s: &str, want: &str) {
        let (got, _) = fx(s);
        assert_eq!(got, want, "input: {s}");
        // output must itself be valid JSON that our strict formatter accepts
        let mut o2 = Vec::new();
        crate::jsonfmt::format(got.as_bytes(), &mut o2, None).unwrap();
    }

    #[test]
    fn trailing_commas_and_unquoted_keys() {
        ok("{a:1,'b':'x',}", r#"{"a":1,"b":"x"}"#);
        ok("[1,2,3,]", "[1,2,3]");
        ok(r#"{"a":[1,2,],}"#, r#"{"a":[1,2]}"#);
    }

    #[test]
    fn missing_and_extra_commas() {
        ok(r#"{"a":1 "b":2}"#, r#"{"a":1,"b":2}"#);
        ok(r#"[{"a":1}{"b":2}]"#, r#"[{"a":1},{"b":2}]"#);
        ok(r#"{"a":1,,"b":2}"#, r#"{"a":1,"b":2}"#);
        ok("[,1]", "[1]");
        ok("[1 2 3]", "[1,2,3]");
    }

    #[test]
    fn comments() {
        ok("// head\n{\"a\":1, /* x */ \"b\":2 // tail\n}", r#"{"a":1,"b":2}"#);
        ok("{\"url\":\"http://x/y\"}", r#"{"url":"http://x/y"}"#); // // inside a string is not a comment
    }

    #[test]
    fn python_and_js_literals() {
        ok("{'a': True, 'b': None, 'c': NaN, d: undefined, e: False}", r#"{"a":true,"b":null,"c":null,"d":null,"e":false}"#);
    }

    #[test]
    fn numbers() {
        ok("[0x1F, .5, +3, 007, -.25, 5., 1e5, 1E+5]", "[31,0.5,3,7,-0.25,5,1e5,1e5]");
    }

    #[test]
    fn brackets() {
        ok(r#"{"a":[1,2}"#, r#"{"a":[1,2]}"#);
        ok(r#"{"a":1"#, r#"{"a":1}"#);
        ok(r#"{"a":1}}"#, r#"{"a":1}"#);
        ok(r#"[1,2}"#, "[1,2]");
        ok(r#"{"a":{"b":[1,{"c":2"#, r#"{"a":{"b":[1,{"c":2}]}}"#);
    }

    #[test]
    fn missing_values_colons() {
        ok(r#"{"a":}"#, r#"{"a":null}"#);
        ok(r#"{"a"}"#, r#"{"a":null}"#);
        ok(r#"{"a":,"b":1}"#, r#"{"a":null,"b":1}"#);
        ok(r#"{"a" 1}"#, r#"{"a":1}"#);
    }

    #[test]
    fn strings() {
        ok("{\"a\":\"x\ny\ttab\"}", r#"{"a":"x\ny\ttab"}"#);
        ok(r#"{'a': 'it\'s "q"'}"#, r#"{"a":"it's \"q\""}"#);
        ok(r#"{"a":"unterminated}"#, r#"{"a":"unterminated}"}"#);
        ok(r#"{"p":"C:\dir\x"}"#, r#"{"p":"C:\\dir\\x"}"#);
        ok("{\"k\":\"é世🚀\"}", "{\"k\":\"é世🚀\"}");
    }

    #[test]
    fn unquoted_values() {
        ok(r#"{"a": hello, "b": 2026-11-03}"#, r#"{"a":"hello","b":"2026-11-03"}"#);
    }

    #[test]
    fn valid_json_unchanged_and_report_empty() {
        let src = r#"{"a":[1,2.5,true,null,{"b":"c"}],"d":"e\n"}"#;
        let (got, rep) = fx(src);
        assert_eq!(got, src);
        assert!(rep.is_empty(), "{rep:?}");
    }

    #[test]
    fn report_counts() {
        let (_, rep) = fx("{a:1,'b':2,}");
        assert_eq!(rep["unquoted keys quoted"], 1);
        assert_eq!(rep["single-quoted strings converted"], 1);
        assert_eq!(rep["trailing commas removed"], 1);
    }

    #[test]
    fn multiple_top_level_values() {
        let (got, _) = fx("{'a':1}\n{'a':2} [3,]");
        assert_eq!(got, "{\"a\":1}\n{\"a\":2}\n[3]");
    }

    #[test]
    fn pretty_output() {
        let mut out = Vec::new();
        fix("{a:[1,2,],}".as_bytes(), &mut out, Some(2)).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "{\n  \"a\": [\n    1,\n    2\n  ]\n}\n");
    }

    #[test]
    fn chunk_boundaries() {
        struct OneByte<'a>(&'a [u8]);
        impl Read for OneByte<'_> {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                if self.0.is_empty() {
                    return Ok(0);
                }
                buf[0] = self.0[0];
                self.0 = &self.0[1..];
                Ok(1)
            }
        }
        let src = "// c\n{a:'x\\'y', b:[0x10,True,],\n/* z */ c:\"q\" d:1}";
        let mut out = Vec::new();
        fix(OneByte(src.as_bytes()), &mut out, None).unwrap();
        let (want, _) = fx(src);
        assert_eq!(String::from_utf8(out).unwrap().trim_end(), want);
    }

    #[test]
    fn unrepairable_key_position_container() {
        let mut out = Vec::new();
        assert!(fix("{ {1}: 2 }".as_bytes(), &mut out, None).is_err());
    }

    #[test]
    fn escaped_and_embedded_json() {
        let mut o = Vec::new();
        let r = fix_deep(br#"{\"a\":1,\"b\":\"x\\ny\"}"#, &mut o, Some(0)).unwrap();
        assert_eq!(String::from_utf8(o).unwrap().replace(['\n', ' '], ""), r#"{"a":1,"b":"x\ny"}"#);
        assert!(r.contains_key("escaped JSON text unescaped"));
        let mut o = Vec::new();
        let r = fix_deep(br#"{"r":"{\"s\":\"ok\",\"n\":[1,2]}","t":"{not json}","k":"[1]"}"#, &mut o, Some(0)).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&o).unwrap();
        assert_eq!(v["r"]["s"], "ok");
        assert_eq!(v["t"], "{not json}");
        assert_eq!(v["k"][0], 1);
        assert_eq!(r["embedded JSON text expanded"], 2);
        let mut o = Vec::new();
        fix_deep(br#""{\"a\":{\"b\":2}}""#, &mut o, Some(0)).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&o).unwrap();
        assert_eq!(v["a"]["b"], 2);
        // plain JSON text in a normal string is untouched by fix()
        let (out, _) = fx(r#"{"r":"{\"s\":1}"}"#);
        assert!(out.contains(r#""{\"s\":1}""#));
    }
}
