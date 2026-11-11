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
        }
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
        self.emit(b"\"")?;
        self.lex = Lex::Str { quote, esc: false };
        Ok(())
    }

    fn str_end(&mut self) -> Result<()> {
        self.emit(b"\"")?;
        self.lex = Lex::Normal;
        if self.role == Role::Key {
            self.after_key();
        } else {
            self.value_done();
        }
        Ok(())
    }

    fn str_byte(&mut self, quote: u8, esc: bool, b: u8) -> Result<()> {
        if esc {
            self.lex = Lex::Str { quote, esc: false };
            match b {
                b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' | b'u' => self.emit(&[b'\\', b])?,
                b'\'' => self.emit(b"'")?,
                b'\n' => {} // line continuation
                _ => {
                    bump(&mut self.report, "invalid escapes fixed");
                    self.emit(&[b'\\', b'\\', b])?;
                }
            }
            return Ok(());
        }
        match b {
            b'\\' => self.lex = Lex::Str { quote, esc: true },
            q if q == quote => self.str_end()?,
            b'"' => self.emit(b"\\\"")?, // inside a single-quoted string
            b'\n' => {
                bump(&mut self.report, "raw control characters escaped");
                self.emit(b"\\n")?;
            }
            b'\r' => {
                bump(&mut self.report, "raw control characters escaped");
                self.emit(b"\\r")?;
            }
            b'\t' => {
                bump(&mut self.report, "raw control characters escaped");
                self.emit(b"\\t")?;
            }
            c if c < 0x20 => {
                bump(&mut self.report, "raw control characters escaped");
                self.emit(format!("\\u{:04x}", c).as_bytes())?;
            }
            c => self.emit(&[c])?,
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
pub fn fix<R: Read, W: Write>(mut r: R, w: W, indent: Option<usize>) -> Result<Report> {
    let mut f = Fix::new(w, indent);
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
}
