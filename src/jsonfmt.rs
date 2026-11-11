//! Streaming JSON formatter / validator / minifier.
//!
//! Constant memory: the input is processed in chunks and never parsed into a
//! tree. Handles multiple concatenated top-level values (NDJSON / JSONL).

use anyhow::{Result, bail};
use std::io::{Read, Write};

#[derive(Clone, Copy, PartialEq, Debug)]
enum St {
    Value,      // a value is required (top-level, after ':' or after ',' in array)
    ValueOrEnd, // just after '[' : value or ']'
    KeyOrEnd,   // just after '{' : key or '}'
    Key,        // after ',' in object
    Colon,
    CommaOrEnd,
}

thread_local! {
    static EXPAND: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// For the current thread: make `format` expand JSON that is stored inside string values
/// (`"parameters": "{\"a\":1}"`) into real, indented JSON. Off by default because it changes the data's shape.
pub fn set_expand(on: bool) {
    EXPAND.with(|e| e.set(on));
}

/// how deep embedded JSON is followed (JSON inside JSON inside JSON...)
const EXPAND_DEPTH: u8 = 4;
/// the longest embedded string that is examined; longer ones are written unchanged
const EXPAND_MAX: usize = 8 << 20;

pub struct Fmt<W: Write> {
    expand: u8,
    cap: Option<Vec<u8>>,
    first: bool,
    w: W,
    indent: Option<usize>,
    stack: Vec<u8>,
    st: St,
    in_str: bool,
    esc: bool,
    str_is_key: bool,
    bare: Vec<u8>,
    bare_off: u64,
    just_opened: bool,
    spaces: Vec<u8>,
    tabs: Vec<u8>,
}

/// pass this as the indent width to indent with tab characters
pub const INDENT_TAB: usize = 255;

const MAX_BARE: usize = 1 << 20;

fn valid_number(b: &[u8]) -> bool {
    let mut i = 0;
    let n = b.len();
    if i < n && b[i] == b'-' {
        i += 1;
    }
    if i >= n {
        return false;
    }
    if b[i] == b'0' {
        i += 1;
    } else if b[i].is_ascii_digit() {
        while i < n && b[i].is_ascii_digit() {
            i += 1;
        }
    } else {
        return false;
    }
    if i < n && b[i] == b'.' {
        i += 1;
        let s = i;
        while i < n && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == s {
            return false;
        }
    }
    if i < n && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < n && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        let s = i;
        while i < n && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == s {
            return false;
        }
    }
    i == n
}

impl<W: Write> Fmt<W> {
    pub fn new(w: W, indent: Option<usize>) -> Self {
        Fmt {
            expand: if EXPAND.with(|e| e.get()) { EXPAND_DEPTH } else { 0 },
            cap: None,
            first: false,
            w,
            indent,
            stack: Vec::new(),
            st: St::Value,
            in_str: false,
            esc: false,
            str_is_key: false,
            bare: Vec::new(),
            bare_off: 0,
            just_opened: false,
            spaces: vec![b' '; 256],
            tabs: vec![b'\t'; 256],
        }
    }

    /// string bytes go straight out, or into the capture buffer while an embedded-JSON candidate is being read
    fn sw(&mut self, b: &[u8]) -> std::io::Result<()> {
        if let Some(c) = self.cap.as_mut() {
            c.extend_from_slice(b);
            if c.len() > EXPAND_MAX {
                let c = self.cap.take().unwrap();
                self.w.write_all(b"\"")?;
                self.w.write_all(&c)?;
            }
            return Ok(());
        }
        self.w.write_all(b)
    }

    /// the closing quote: write it, or - if this string holds a JSON object / array - write that, formatted
    fn end_string(&mut self) -> Result<()> {
        let Some(raw) = self.cap.take() else {
            self.w.write_all(b"\"")?;
            return Ok(());
        };
        let depth = self.stack.len();
        if let Some(text) = embedded_json(&raw) {
            let mut sub = Fmt::new(Vec::new(), self.indent);
            sub.expand = self.expand - 1;
            if sub.feed(text.as_bytes(), 0).and_then(|_| sub.finish(text.len() as u64)).is_ok() {
                let mut out = std::mem::take(&mut sub.w);
                while out.last() == Some(&b'\n') {
                    out.pop();
                }
                for (k, line) in out.split(|c| *c == b'\n').enumerate() {
                    if k > 0 {
                        self.nl(depth)?;
                    }
                    self.w.write_all(line)?;
                }
                return Ok(());
            }
        }
        self.w.write_all(b"\"")?;
        self.w.write_all(&raw)?;
        self.w.write_all(b"\"")?;
        Ok(())
    }

    fn nl(&mut self, depth: usize) -> std::io::Result<()> {
        if let Some(n) = self.indent {
            self.w.write_all(b"\n")?;
            let (fill, mut total) = if n == INDENT_TAB { (&self.tabs, depth) } else { (&self.spaces, depth * n) };
            while total > 0 {
                let k = total.min(fill.len());
                self.w.write_all(&fill[..k])?;
                total -= k;
            }
        }
        Ok(())
    }

    fn begin_item(&mut self) -> std::io::Result<()> {
        if self.just_opened {
            self.just_opened = false;
            let d = self.stack.len();
            self.nl(d)?;
        }
        Ok(())
    }

    fn value_done(&mut self) -> std::io::Result<()> {
        if self.stack.is_empty() {
            self.w.write_all(b"\n")?;
            self.st = St::Value;
        } else {
            self.st = St::CommaOrEnd;
        }
        Ok(())
    }

    fn flush_bare(&mut self) -> Result<()> {
        if self.bare.is_empty() {
            return Ok(());
        }
        let ok = matches!(self.bare.as_slice(), b"true" | b"false" | b"null") || valid_number(&self.bare);
        if !ok {
            bail!(
                "invalid token '{}' at byte {}",
                String::from_utf8_lossy(&self.bare[..self.bare.len().min(40)]),
                self.bare_off
            );
        }
        self.w.write_all(&self.bare)?;
        self.bare.clear();
        self.value_done()?;
        Ok(())
    }

    fn unexpected(&self, b: u8, off: u64) -> anyhow::Error {
        let want = match self.st {
            St::Value => "a value",
            St::ValueOrEnd => "a value or ']'",
            St::KeyOrEnd => "a key or '}'",
            St::Key => "a key",
            St::Colon => "':'",
            St::CommaOrEnd => "',' or a closing bracket",
        };
        anyhow::anyhow!("unexpected '{}' at byte {} (expected {})", (b as char).escape_default(), off, want)
    }

    pub fn feed(&mut self, chunk: &[u8], base: u64) -> Result<()> {
        let mut i = 0;
        let n = chunk.len();
        while i < n {
            if self.in_str {
                if self.first {
                    self.first = false;
                    if matches!(chunk[i], b'{' | b'[') {
                        self.cap = Some(Vec::new());
                    } else {
                        self.w.write_all(b"\"")?;
                    }
                }
                if self.esc {
                    self.sw(&chunk[i..i + 1])?;
                    self.esc = false;
                    i += 1;
                    continue;
                }
                match memchr::memchr2(b'"', b'\\', &chunk[i..]) {
                    None => {
                        self.sw(&chunk[i..])?;
                        return Ok(());
                    }
                    Some(p) => {
                        let end = i + p;
                        if chunk[end] == b'\\' {
                            self.sw(&chunk[i..=end])?;
                            self.esc = true;
                        } else {
                            self.sw(&chunk[i..end])?;
                            self.end_string()?;
                            self.in_str = false;
                            if self.str_is_key {
                                self.st = St::Colon;
                            } else {
                                self.value_done()?;
                            }
                        }
                        i = end + 1;
                        continue;
                    }
                }
            }
            let b = chunk[i];
            let off = base + i as u64;
            match b {
                b' ' | b'\t' | b'\n' | b'\r' => {
                    self.flush_bare()?;
                }
                b'"' => {
                    self.flush_bare()?;
                    if !matches!(self.st, St::Value | St::ValueOrEnd | St::KeyOrEnd | St::Key) {
                        return Err(self.unexpected(b, off));
                    }
                    self.begin_item()?;
                    self.str_is_key = matches!(self.st, St::KeyOrEnd | St::Key);
                    self.in_str = true;
                    self.first = self.expand > 0 && !self.str_is_key;
                    if !self.first {
                        self.w.write_all(b"\"")?;
                    }
                }
                b'{' | b'[' => {
                    self.flush_bare()?;
                    if !matches!(self.st, St::Value | St::ValueOrEnd) {
                        return Err(self.unexpected(b, off));
                    }
                    self.begin_item()?;
                    self.w.write_all(&[b])?;
                    self.stack.push(b);
                    self.just_opened = true;
                    self.st = if b == b'{' { St::KeyOrEnd } else { St::ValueOrEnd };
                }
                b'}' | b']' => {
                    self.flush_bare()?;
                    let open = if b == b'}' { b'{' } else { b'[' };
                    let empty_ok = (b == b'}' && self.st == St::KeyOrEnd) || (b == b']' && self.st == St::ValueOrEnd);
                    if self.stack.last() != Some(&open) || !(self.st == St::CommaOrEnd || empty_ok) {
                        return Err(self.unexpected(b, off));
                    }
                    self.stack.pop();
                    if self.just_opened {
                        self.just_opened = false;
                    } else {
                        let d = self.stack.len();
                        self.nl(d)?;
                    }
                    self.w.write_all(&[b])?;
                    self.value_done()?;
                }
                b',' => {
                    self.flush_bare()?;
                    if self.st != St::CommaOrEnd {
                        return Err(self.unexpected(b, off));
                    }
                    self.w.write_all(b",")?;
                    let d = self.stack.len();
                    self.nl(d)?;
                    self.st = if self.stack.last() == Some(&b'{') { St::Key } else { St::Value };
                }
                b':' => {
                    self.flush_bare()?;
                    if self.st != St::Colon {
                        return Err(self.unexpected(b, off));
                    }
                    self.w.write_all(if self.indent.is_some() { b": " } else { b":" })?;
                    self.st = St::Value;
                }
                _ => {
                    if self.bare.is_empty() {
                        if !matches!(self.st, St::Value | St::ValueOrEnd) {
                            return Err(self.unexpected(b, off));
                        }
                        self.begin_item()?;
                        self.bare_off = off;
                    }
                    if self.bare.len() >= MAX_BARE {
                        bail!("token too long at byte {}", self.bare_off);
                    }
                    self.bare.push(b);
                }
            }
            i += 1;
        }
        Ok(())
    }

    pub fn finish(&mut self, total: u64) -> Result<()> {
        self.flush_bare()?;
        if self.in_str {
            bail!("unterminated string at end of input (byte {})", total);
        }
        if !self.stack.is_empty() {
            bail!("unexpected end of input: {} unclosed bracket(s)", self.stack.len());
        }
        self.w.flush()?;
        Ok(())
    }
}

/// the text inside a JSON string literal (raw = its bytes between the quotes) if it is one JSON object or array
fn embedded_json(raw: &[u8]) -> Option<String> {
    let mut lit = Vec::with_capacity(raw.len() + 2);
    lit.push(b'"');
    lit.extend_from_slice(raw);
    lit.push(b'"');
    let s: String = serde_json::from_slice(&lit).ok()?;
    let t = s.trim();
    if !(t.starts_with('{') && t.ends_with('}') || t.starts_with('[') && t.ends_with(']')) {
        return None;
    }
    serde_json::from_str::<serde_json::Value>(t).ok()?;
    Some(t.to_string())
}

/// Format (indent = Some(n)) or minify (indent = None) a JSON stream.
pub fn format<R: Read, W: Write>(mut r: R, w: W, indent: Option<usize>) -> Result<()> {
    let mut f = Fmt::new(w, indent);
    let mut buf = vec![0u8; 1 << 20];
    let mut base: u64 = 0;
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
        f.feed(chunk, off)?;
        base += n as u64;
    }
    f.finish(base)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(s: &str, indent: Option<usize>) -> Result<String> {
        let mut out = Vec::new();
        format(s.as_bytes(), &mut out, indent)?;
        Ok(String::from_utf8(out).unwrap())
    }

    #[test]
    fn basic_pretty() {
        let o = fmt(r#"{"a":1,"b":[1,2,{"c":null}],"d":{}}"#, Some(2)).unwrap();
        assert_eq!(o, "{\n  \"a\": 1,\n  \"b\": [\n    1,\n    2,\n    {\n      \"c\": null\n    }\n  ],\n  \"d\": {}\n}\n");
    }

    #[test]
    fn indent_widths_and_tabs() {
        let src = r#"{"a":[1,{"b":2}]}"#;
        assert_eq!(fmt(src, Some(4)).unwrap(), "{\n    \"a\": [\n        1,\n        {\n            \"b\": 2\n        }\n    ]\n}\n");
        assert_eq!(fmt(src, Some(INDENT_TAB)).unwrap(), "{\n\t\"a\": [\n\t\t1,\n\t\t{\n\t\t\t\"b\": 2\n\t\t}\n\t]\n}\n");
    }

    #[test]
    fn minify() {
        let o = fmt("{ \"a\" : [ 1 , 2 ] ,\n \"b\" : \"x y\" }", None).unwrap();
        assert_eq!(o, "{\"a\":[1,2],\"b\":\"x y\"}\n");
    }

    #[test]
    fn strings_with_escapes_and_brackets() {
        let src = r#"{"k\"ey":"va\\lue {not} [real], \" :","u":"é"}"#;
        let o = fmt(src, None).unwrap();
        assert_eq!(o.trim_end(), src);
    }

    #[test]
    fn ndjson_multiple_values() {
        let o = fmt("{\"a\":1}\n{\"a\":2}\n[1]\n3 \"s\"", None).unwrap();
        assert_eq!(o, "{\"a\":1}\n{\"a\":2}\n[1]\n3\n\"s\"\n");
    }

    #[test]
    fn scalars_and_numbers() {
        let o = fmt("[-0.5e+10, 0, 12, true,false , null, 1E5]", None).unwrap();
        assert_eq!(o, "[-0.5e+10,0,12,true,false,null,1E5]\n");
    }

    #[test]
    fn empty_and_nested_empty() {
        assert_eq!(fmt("", Some(2)).unwrap(), "");
        assert_eq!(fmt("  \n ", Some(2)).unwrap(), "");
        assert_eq!(fmt("[[],{}]", Some(2)).unwrap(), "[\n  [],\n  {}\n]\n");
    }

    #[test]
    fn idempotent() {
        let src = r#"{"a":[{"b":1},{"c":[1,2,3]}],"d":"e"}"#;
        let a = fmt(src, Some(4)).unwrap();
        let b = fmt(&a, Some(4)).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn chunk_boundaries() {
        // feed one byte at a time through a tiny reader
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
        let src = r#"{"a\\":"x\"y","n":-12.5e3,"t":[true,false,null],"u":"\\"}"#;
        let mut out = Vec::new();
        format(OneByte(src.as_bytes()), &mut out, None).unwrap();
        assert_eq!(String::from_utf8(out).unwrap().trim_end(), src);
    }

    #[test]
    fn errors() {
        for bad in ["{", "[1,]", "{\"a\" 1}", "{\"a\":}", "[1 2]", "tru", "{\"a\":1,}", "]", "\"abc", "{1:2}", "01", "1.", "-", "[,1]", "{\"a\":1}}"] {
            assert!(fmt(bad, Some(2)).is_err(), "should fail: {bad}");
        }
    }

    #[test]
    fn bom_stripped() {
        let mut out = Vec::new();
        format(&b"\xEF\xBB\xBF{\"a\":1}"[..], &mut out, None).unwrap();
        assert_eq!(out, b"{\"a\":1}\n");
    }

    #[test]
    fn non_ascii_passthrough() {
        let o = fmt("{\"k\":\"héllo 世界 🚀\"}", None).unwrap();
        assert_eq!(o, "{\"k\":\"héllo 世界 🚀\"}\n");
    }

    #[test]
    fn expands_embedded_json_when_asked() {
        let src = r#"{"id":1,"parameters":"{\"format\":\"%Y\",\"tz\":\"UTC\",\"deep\":\"[1,{\\\"k\\\":2}]\"}","note":"{not json","s":"plain","arr":"[1,2]"}"#;
        set_expand(false);
        let off = fmt(src, Some(2)).unwrap();
        assert!(off.contains(r#""parameters": "{\"format\""#), "{off}");
        set_expand(true);
        let on = fmt(src, Some(2)).unwrap();
        set_expand(false);
        assert!(on.contains("\"parameters\": {\n    \"format\": \"%Y\",\n    \"tz\": \"UTC\",\n    \"deep\": [\n      1,\n      {\n        \"k\": 2\n      }\n    ]\n  },"), "{on}");
        assert!(on.contains(r#""note": "{not json""#) && on.contains(r#""s": "plain""#), "{on}");
        assert!(on.contains("\"arr\": [\n    1,\n    2\n  ]"), "{on}");
        // minify keeps it on one line
        set_expand(true);
        let min = fmt(src, None).unwrap();
        set_expand(false);
        assert!(min.contains(r#""parameters":{"format":"%Y","tz":"UTC","deep":[1,{"k":2}]}"#), "{min}");
        // chunk boundaries do not matter
        set_expand(true);
        let mut out = Vec::new();
        let mut f = Fmt::new(&mut out, Some(2));
        for (k, b) in src.as_bytes().chunks(3).enumerate() {
            f.feed(b, (k * 3) as u64).unwrap();
        }
        f.finish(src.len() as u64).unwrap();
        set_expand(false);
        assert_eq!(String::from_utf8(out).unwrap(), on);
    }
}
