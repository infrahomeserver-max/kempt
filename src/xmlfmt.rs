//! Streaming XML pretty-printer / minifier.
//!
//! Memory is bounded by the largest single token (tag, comment or text node),
//! never by file size. Tag names are validated for proper nesting.
//!
//! Known limitation: mixed content that *starts* with a child element
//! (`<p><b>x</b> text</p>`) is indented before the first child, because the
//! decision is made without lookahead over the whole element.

use anyhow::{Result, bail};
use std::io::{BufRead, Write};

struct Frame {
    name: Vec<u8>,
    mixed: bool,
    block: bool,
}

struct Out<W: Write> {
    w: W,
    indent: Option<usize>,
    frames: Vec<Frame>,
    at_start: bool,
    spaces: Vec<u8>,
    tabs: Vec<u8>,
}

impl<W: Write> Out<W> {
    fn nl(&mut self, depth: usize) -> std::io::Result<()> {
        if let Some(n) = self.indent {
            self.w.write_all(b"\n")?;
            let (fill, mut total) = if n == crate::jsonfmt::INDENT_TAB { (&self.tabs, depth) } else { (&self.spaces, depth * n) };
            while total > 0 {
                let k = total.min(fill.len());
                self.w.write_all(&fill[..k])?;
                total -= k;
            }
        }
        Ok(())
    }

    /// element-level item (open tag, empty tag, comment, PI, doctype)
    fn block_item(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        let parent_mixed = self.frames.last().map(|f| f.mixed).unwrap_or(false);
        if self.indent.is_some() && !parent_mixed {
            if !self.at_start {
                let d = self.frames.len();
                self.nl(d)?;
            }
        }
        self.w.write_all(bytes)?;
        self.at_start = false;
        if let Some(p) = self.frames.last_mut() {
            p.block = true;
        }
        Ok(())
    }

    fn text(&mut self, bytes: &[u8], is_cdata: bool) -> std::io::Result<()> {
        let ws_only = !is_cdata && bytes.iter().all(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r'));
        if ws_only {
            if let Some(f) = self.frames.last() {
                if f.mixed && self.indent.is_some() {
                    self.w.write_all(bytes)?;
                    self.at_start = false;
                }
            }
            return Ok(());
        }
        match self.frames.last_mut() {
            Some(f) => {
                f.mixed = true;
                self.w.write_all(bytes)?;
            }
            None => {
                // text outside the root element: keep, trimmed
                let t = bytes.trim_ascii();
                if !self.at_start {
                    self.w.write_all(b"\n")?;
                }
                self.w.write_all(t)?;
            }
        }
        self.at_start = false;
        Ok(())
    }
}

fn tag_name(t: &[u8], skip: usize) -> &[u8] {
    let s = &t[skip..];
    let end = s
        .iter()
        .position(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | b'>' | b'/'))
        .unwrap_or(s.len());
    &s[..end]
}

fn read_until_seq<R: BufRead>(r: &mut R, seq: &[u8], buf: &mut Vec<u8>) -> Result<()> {
    let last = *seq.last().unwrap();
    loop {
        let n = r.read_until(last, buf)?;
        if n == 0 {
            bail!("unexpected end of input inside '{}'", String::from_utf8_lossy(&buf[..buf.len().min(20)]));
        }
        if buf.ends_with(seq) {
            return Ok(());
        }
    }
}

/// Read the rest of a start/end tag or DOCTYPE, honouring quotes and `[...]`.
fn read_tag_rest<R: BufRead>(r: &mut R, buf: &mut Vec<u8>, track_brackets: bool) -> Result<()> {
    let mut quote: Option<u8> = None;
    let mut depth: i32 = 0;
    let mut scanned = 0;
    loop {
        let n = r.read_until(b'>', buf)?;
        if n == 0 {
            bail!("unexpected end of input inside a tag");
        }
        for &b in &buf[scanned..] {
            match quote {
                Some(q) => {
                    if b == q {
                        quote = None;
                    }
                }
                None => match b {
                    b'"' | b'\'' => quote = Some(b),
                    b'[' if track_brackets => depth += 1,
                    b']' if track_brackets => depth -= 1,
                    _ => {}
                },
            }
        }
        scanned = buf.len();
        if buf.ends_with(b">") && quote.is_none() && depth <= 0 {
            return Ok(());
        }
    }
}

fn read_byte<R: BufRead>(r: &mut R) -> Result<Option<u8>> {
    let b = {
        let buf = r.fill_buf()?;
        if buf.is_empty() {
            return Ok(None);
        }
        buf[0]
    };
    r.consume(1);
    Ok(Some(b))
}

pub fn format<R: BufRead, W: Write>(r: R, w: W, indent: Option<usize>) -> Result<()> {
    format_opts(r, w, indent, false).map(|_| ())
}

fn bump(rep: &mut crate::jsonfix::Report, k: &'static str) {
    *rep.entry(k).or_insert(0) += 1;
}

/// Escape a stray `&` (one that doesn't start an entity) in lenient mode.
fn escape_amp<'a>(t: &'a [u8], rep: &mut crate::jsonfix::Report) -> std::borrow::Cow<'a, [u8]> {
    if !t.contains(&b'&') {
        return std::borrow::Cow::Borrowed(t);
    }
    let mut out = Vec::with_capacity(t.len() + 8);
    let mut i = 0;
    let mut changed = false;
    while i < t.len() {
        if t[i] == b'&' {
            // valid entity: &name; or &#123; or &#x1F;
            let mut j = i + 1;
            while j < t.len() && j - i <= 12 && (t[j].is_ascii_alphanumeric() || t[j] == b'#') {
                j += 1;
            }
            if j < t.len() && t[j] == b';' && j > i + 1 {
                out.extend_from_slice(&t[i..=j]);
                i = j + 1;
                continue;
            }
            out.extend_from_slice(b"&amp;");
            changed = true;
            bump(rep, "stray & escaped as &amp;");
        } else {
            out.push(t[i]);
        }
        i += 1;
    }
    if changed { std::borrow::Cow::Owned(out) } else { std::borrow::Cow::Borrowed(t) }
}

const VOID: [&str; 14] = ["br", "hr", "img", "input", "meta", "link", "area", "base", "col", "embed", "param", "source", "track", "wbr"];

/// Lenient start-tag cleanup: quote bare attribute values, give valueless attributes a value,
/// self-close HTML void elements. Returns the repaired tag.
fn fix_start_tag(tag: &[u8], rep: &mut crate::jsonfix::Report) -> Vec<u8> {
    let self_closed = tag.ends_with(b"/>");
    let end = if self_closed { tag.len() - 2 } else { tag.len() - 1 };
    let body = &tag[1..end];
    let name_end = body.iter().position(|b| b.is_ascii_whitespace()).unwrap_or(body.len());
    let mut out = Vec::with_capacity(tag.len() + 8);
    out.push(b'<');
    out.extend_from_slice(&body[..name_end]);
    let mut i = name_end;
    let mut changed = false;
    while i < body.len() {
        while i < body.len() && body[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= body.len() {
            break;
        }
        if body[i] == b'/' {
            i += 1;
            continue;
        }
        let ns = i;
        while i < body.len() && !body[i].is_ascii_whitespace() && body[i] != b'=' {
            i += 1;
        }
        let aname = &body[ns..i];
        let mut j = i;
        while j < body.len() && body[j].is_ascii_whitespace() {
            j += 1;
        }
        out.push(b' ');
        out.extend_from_slice(aname);
        if j < body.len() && body[j] == b'=' {
            j += 1;
            while j < body.len() && body[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < body.len() && (body[j] == b'"' || body[j] == b'\'') {
                let q = body[j];
                let vs = j;
                j += 1;
                while j < body.len() && body[j] != q {
                    j += 1;
                }
                let end_q = (j + 1).min(body.len());
                out.extend_from_slice(b"=");
                out.extend_from_slice(&body[vs..end_q]);
                if j >= body.len() {
                    out.push(q); // unterminated quote
                    changed = true;
                }
                i = end_q;
            } else {
                let vs = j;
                while j < body.len() && !body[j].is_ascii_whitespace() {
                    j += 1;
                }
                out.extend_from_slice(b"=\"");
                for &c in &body[vs..j] {
                    if c == b'"' {
                        out.extend_from_slice(b"&quot;");
                    } else {
                        out.push(c);
                    }
                }
                out.push(b'"');
                changed = true;
                bump(rep, "unquoted attribute values quoted");
                i = j;
            }
        } else {
            out.extend_from_slice(b"=\"");
            out.extend_from_slice(aname);
            out.push(b'"');
            changed = true;
            bump(rep, "valueless attributes given a value");
            i = j;
        }
    }
    let lname = String::from_utf8_lossy(&body[..name_end]).to_ascii_lowercase();
    if self_closed || VOID.contains(&lname.as_str()) {
        if !self_closed {
            bump(rep, "void elements self-closed (<br> -> <br/>)");
            changed = true;
        }
        out.extend_from_slice(b"/>");
    } else {
        out.push(b'>');
    }
    if changed { out } else { tag.to_vec() }
}

/// Read one markup token starting right after '<'.
fn read_markup<R: BufRead>(r: &mut R, tag_start: u64) -> Result<Vec<u8>> {
    let mut tag: Vec<u8> = vec![b'<'];
    let c = match read_byte(r)? {
        Some(c) => c,
        None => bail!("unexpected end of input after '<' at byte {}", tag_start),
    };
    tag.push(c);
    match c {
        b'?' => read_until_seq(r, b"?>", &mut tag)?,
        b'!' => {
            let d = match read_byte(r)? {
                Some(d) => d,
                None => bail!("unexpected end of input after '<!' at byte {}", tag_start),
            };
            tag.push(d);
            match d {
                b'-' => read_until_seq(r, b"-->", &mut tag)?,
                b'[' => read_until_seq(r, b"]]>", &mut tag)?,
                _ => read_tag_rest(r, &mut tag, true)?,
            }
        }
        b'>' => bail!("empty tag at byte {}", tag_start),
        _ => read_tag_rest(r, &mut tag, false)?,
    }
    Ok(tag)
}

/// Format XML. With `lenient`, repairs problems instead of failing and reports what it fixed.
pub fn format_opts<R: BufRead, W: Write>(mut r: R, w: W, indent: Option<usize>, lenient: bool) -> Result<crate::jsonfix::Report> {
    let mut rep = crate::jsonfix::Report::new();
    let mut o = Out { w, indent, frames: Vec::new(), at_start: true, spaces: vec![b' '; 256], tabs: vec![b'\t'; 256] };
    let mut buf: Vec<u8> = Vec::new();
    let mut off: u64 = 0;
    let mut first = true;

    loop {
        buf.clear();
        let n = r.read_until(b'<', &mut buf)?;
        if n == 0 {
            break;
        }
        off += n as u64;
        let found_lt = buf.last() == Some(&b'<');
        let mut text: &[u8] = if found_lt { &buf[..buf.len() - 1] } else { &buf[..] };
        if first {
            first = false;
            if text.starts_with(&[0xEF, 0xBB, 0xBF]) {
                text = &text[3..];
            }
        }
        if !text.is_empty() {
            if lenient {
                let t = escape_amp(text, &mut rep);
                o.text(&t, false)?;
            } else {
                o.text(text, false)?;
            }
        }
        if !found_lt {
            break;
        }
        let tag_start = off - 1;

        // ---- read one markup token
        let tag = match read_markup(&mut r, tag_start) {
            Ok(t) => t,
            Err(e) => {
                if lenient {
                    bump(&mut rep, "truncated final tag dropped");
                    break;
                }
                return Err(e);
            }
        };
        off += (tag.len() - 1) as u64;

        // ---- classify and emit
        if tag.starts_with(b"<?") || tag.starts_with(b"<!--") || (tag.starts_with(b"<!") && !tag.starts_with(b"<![")) {
            o.block_item(&tag)?;
        } else if tag.starts_with(b"<![") {
            o.text(&tag, true)?;
        } else if tag.starts_with(b"</") {
            let name = tag_name(&tag, 2).to_vec();
            if lenient {
                match o.frames.iter().rposition(|f| f.name == name) {
                    None => {
                        bump(&mut rep, "stray closing tags dropped");
                        continue;
                    }
                    Some(k) => {
                        while o.frames.len() > k + 1 {
                            let nm = o.frames.last().unwrap().name.clone();
                            let mut t = b"</".to_vec();
                            t.extend_from_slice(&nm);
                            t.push(b'>');
                            close_top(&mut o, &t)?;
                            bump(&mut rep, "unclosed elements auto-closed");
                        }
                    }
                }
            } else {
                match o.frames.last() {
                    None => bail!("closing tag </{}> without opening tag at byte {}", String::from_utf8_lossy(&name), tag_start),
                    Some(f) if f.name != name => bail!(
                        "mismatched closing tag at byte {}: expected </{}>, found </{}>",
                        tag_start,
                        String::from_utf8_lossy(&f.name),
                        String::from_utf8_lossy(&name)
                    ),
                    _ => {}
                }
            }
            close_top(&mut o, &tag)?;
        } else if tag.ends_with(b"/>") {
            if lenient {
                let t = fix_start_tag(&tag, &mut rep);
                o.block_item(&t)?;
            } else {
                o.block_item(&tag)?;
            }
        } else {
            let t = if lenient { fix_start_tag(&tag, &mut rep) } else { tag.clone() };
            if t.ends_with(b"/>") {
                o.block_item(&t)?;
            } else {
                let name = tag_name(&t, 1).to_vec();
                o.block_item(&t)?;
                o.frames.push(Frame { name, mixed: false, block: false });
            }
        }
    }

    if lenient {
        while let Some(f) = o.frames.last() {
            let mut t = b"</".to_vec();
            t.extend_from_slice(&f.name);
            t.push(b'>');
            close_top(&mut o, &t)?;
            bump(&mut rep, "unclosed elements auto-closed");
        }
    }
    if let Some(f) = o.frames.last() {
        bail!("unexpected end of input: element <{}> is not closed", String::from_utf8_lossy(&f.name));
    }
    if !o.at_start {
        o.w.write_all(b"\n")?;
    }
    o.w.flush()?;
    Ok(rep)
}

fn close_top<W: Write>(o: &mut Out<W>, tag: &[u8]) -> Result<()> {
    let frame = o.frames.pop().expect("frame present");
    if o.indent.is_some() && frame.block && !frame.mixed {
        let d = o.frames.len();
        o.nl(d)?;
    }
    o.w.write_all(tag)?;
    o.at_start = false;
    Ok(())
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
    fn pretty_nested() {
        let o = fmt("<a><b>1</b><c><d/></c></a>", Some(2)).unwrap();
        assert_eq!(o, "<a>\n  <b>1</b>\n  <c>\n    <d/>\n  </c>\n</a>\n");
    }

    #[test]
    fn indent_four_and_tab() {
        assert_eq!(fmt("<a><b>1</b></a>", Some(4)).unwrap(), "<a>\n    <b>1</b>\n</a>\n");
        assert_eq!(fmt("<a><b><c/></b></a>", Some(crate::jsonfmt::INDENT_TAB)).unwrap(), "<a>\n\t<b>\n\t\t<c/>\n\t</b>\n</a>\n");
    }

    #[test]
    fn minify_strips_whitespace() {
        let o = fmt("<a>\n  <b>1</b>\n  <c> x </c>\n</a>\n", None).unwrap();
        assert_eq!(o, "<a><b>1</b><c> x </c></a>\n");
    }

    #[test]
    fn prolog_comment_cdata_attrs() {
        let src = "<?xml version=\"1.0\"?><!-- hi --><r a=\"x>y\" b='1'><![CDATA[ <raw> ]]><e/></r>";
        let o = fmt(src, Some(2)).unwrap();
        assert!(o.starts_with("<?xml version=\"1.0\"?>\n<!-- hi -->\n<r a=\"x>y\" b='1'>"));
        assert!(o.contains("<![CDATA[ <raw> ]]>"));
        assert!(o.trim_end().ends_with("</r>"));
    }

    #[test]
    fn doctype_with_internal_subset() {
        let src = "<!DOCTYPE r [<!ELEMENT r (#PCDATA)>]><r>t</r>";
        let o = fmt(src, Some(2)).unwrap();
        assert_eq!(o, "<!DOCTYPE r [<!ELEMENT r (#PCDATA)>]>\n<r>t</r>\n");
    }

    #[test]
    fn idempotent() {
        let src = "<root><item id=\"1\"><name>A</name><tags><t>x</t><t>y</t></tags></item><item id=\"2\"/></root>";
        let a = fmt(src, Some(2)).unwrap();
        let b = fmt(&a, Some(2)).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn mixed_content_preserved() {
        let o = fmt("<p>hello <b>bold</b> world</p>", Some(2)).unwrap();
        assert_eq!(o, "<p>hello <b>bold</b> world</p>\n");
    }

    #[test]
    fn errors() {
        for bad in ["<a>", "<a></b>", "</a>", "<a", "<a><!-- x </a>", "<"] {
            assert!(fmt(bad, Some(2)).is_err(), "should fail: {bad}");
        }
    }

    fn lenient(s: &str) -> (String, crate::jsonfix::Report) {
        let mut out = Vec::new();
        let r = format_opts(s.as_bytes(), &mut out, Some(2), true).unwrap();
        (String::from_utf8(out).unwrap(), r)
    }

    #[test]
    fn lenient_unclosed_and_mismatched() {
        let (o, r) = lenient("<a><b><c>x</a>");
        assert_eq!(o, "<a>\n  <b>\n    <c>x</c>\n  </b>\n</a>\n");
        assert!(r["unclosed elements auto-closed"] >= 2);
        let (o, _) = lenient("<a><b>t</b></x></a>");
        assert_eq!(o, "<a>\n  <b>t</b>\n</a>\n");
        let (o, _) = lenient("<a><b>t");
        assert_eq!(o, "<a>\n  <b>t</b>\n</a>\n");
    }

    #[test]
    fn lenient_amp_attrs_void() {
        let (o, r) = lenient("<a x=1 disabled y='z'>AT&T &amp; R&D</a>");
        assert_eq!(o, "<a x=\"1\" disabled=\"disabled\" y='z'>AT&amp;T &amp; R&amp;D</a>\n");
        assert_eq!(r["stray & escaped as &amp;"], 2);
        let (o, _) = lenient("<p>a<br>b<img src=x></p>");
        assert!(o.contains("<br/>") && o.contains("<img src=\"x\"/>"), "{o}");
    }

    #[test]
    fn lenient_truncated_tag_and_valid_input_untouched() {
        let (o, r) = lenient("<a><b>t</b><c attr=\"x");
        assert!(o.starts_with("<a>\n  <b>t</b>") && o.trim_end().ends_with("</a>"), "{o}");
        assert!(r.contains_key("truncated final tag dropped"));
        let src = "<r a=\"1\">\n  <i>t</i>\n  <e/>\n</r>\n";
        let (o, r) = lenient(src);
        assert_eq!(o, src);
        assert!(r.is_empty(), "{r:?}");
    }

    #[test]
    fn empty_input() {
        assert_eq!(fmt("", Some(2)).unwrap(), "");
    }

    #[test]
    fn well_formed_after_format() {
        let src = "<a x=\"1\"><b>t&amp;u</b><c><![CDATA[x]]></c></a>";
        let o = fmt(src, Some(2)).unwrap();
        let m = fmt(&o, None).unwrap();
        assert_eq!(m.trim_end(), src);
    }
}
