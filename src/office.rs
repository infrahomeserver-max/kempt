//! Read the text of word-processor and presentation files (.docx, .odt, .pptx, .odp, .ods) with the small ZIP
//! reader that already opens Excel files. Text only: formatting, pictures and charts are not shown.

use crate::xlsx::{decode_entities, Zip};
use anyhow::{bail, Result};
use std::path::Path;

pub fn is_office(lower: &str) -> bool {
    [".docx", ".docm", ".dotx", ".odt", ".ott", ".pptx", ".pptm", ".ppsx", ".odp", ".ods"].iter().any(|e| lower.ends_with(e))
}

pub fn is_legacy(lower: &str) -> bool {
    [".doc", ".ppt", ".xls", ".numbers", ".pages", ".key"].iter().any(|e| lower.ends_with(e))
}

/// the plain text of the document; one paragraph per line, table cells separated by tabs
pub fn extract(path: &Path) -> Result<(String, &'static str)> {
    let lower = path.to_string_lossy().to_ascii_lowercase();
    let mut z = Zip::open(path)?;
    if [".docx", ".docm", ".dotx"].iter().any(|e| lower.ends_with(e)) {
        let xml = String::from_utf8_lossy(&z.read("word/document.xml")?).into_owned();
        return Ok((walk(&xml, Kind::Word), "Word document"));
    }
    if [".odt", ".ott", ".odp", ".ods"].iter().any(|e| lower.ends_with(e)) {
        let xml = String::from_utf8_lossy(&z.read("content.xml")?).into_owned();
        let what = if lower.ends_with(".ods") { "OpenDocument spreadsheet" } else if lower.ends_with(".odp") { "OpenDocument presentation" } else { "OpenDocument text" };
        return Ok((walk(&xml, Kind::Odf), what));
    }
    if [".pptx", ".pptm", ".ppsx"].iter().any(|e| lower.ends_with(e)) {
        let mut slides: Vec<(u32, String)> = z
            .names()
            .into_iter()
            .filter_map(|n| n.strip_prefix("ppt/slides/slide").and_then(|r| r.strip_suffix(".xml")).and_then(|d| d.parse::<u32>().ok()).map(|i| (i, n)))
            .collect();
        slides.sort();
        if slides.is_empty() {
            bail!("no slides found in this presentation");
        }
        let mut out = String::new();
        for (i, name) in slides.iter().take(500) {
            let xml = String::from_utf8_lossy(&z.read(name)?).into_owned();
            out.push_str(&format!("──── Slide {i} ────\n"));
            out.push_str(&walk(&xml, Kind::Slide));
            out.push('\n');
        }
        return Ok((out, "PowerPoint presentation"));
    }
    bail!("not a supported office file")
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Word,
    Slide,
    Odf,
}

fn walk(xml: &str, kind: Kind) -> String {
    let (text_tag, para_end): (Option<&str>, &[&str]) = match kind {
        Kind::Word => (Some("w:t"), &["w:p"]),
        Kind::Slide => (Some("a:t"), &["a:p"]),
        Kind::Odf => (None, &["text:p", "text:h"]),
    };
    let mut out = String::with_capacity(xml.len() / 3);
    let mut i = 0;
    let b = xml.as_bytes();
    let mut in_text = text_tag.is_none();
    let mut cell = 0u32; // paragraphs inside a table cell stay on one line
    let mut skip = 0u32; // ODF: styles and the like carry no text
    while i < b.len() {
        if b[i] == b'<' {
            let Some(end) = xml[i..].find('>') else { break };
            let tag = &xml[i + 1..i + end];
            i += end + 1;
            let closing = tag.starts_with('/');
            let selfc = tag.ends_with('/');
            let name = tag.trim_start_matches('/').split(|c: char| c.is_whitespace() || c == '/').next().unwrap_or("");
            if kind == Kind::Odf && (name == "office:automatic-styles" || name == "office:font-face-decls" || name == "office:scripts") {
                if closing { skip = skip.saturating_sub(1) } else if !selfc { skip += 1 }
                continue;
            }
            if let Some(t) = text_tag {
                if name == t {
                    in_text = !closing && !selfc;
                    continue;
                }
            }
            if matches!(name, "w:tc" | "table:table-cell") && !selfc {
                if closing { cell = cell.saturating_sub(1) } else { cell += 1 }
            }
            match name {
                "w:tab" | "text:tab" => out.push('\t'),
                "w:br" | "w:cr" | "a:br" | "text:line-break" => out.push('\n'),
                "text:s" => out.push(' '),
                "w:tc" | "text:table-cell" | "table:table-cell" if closing => {
                    while out.ends_with(' ') { out.pop(); }
                    out.push('\t');
                }
                "w:tr" | "table:table-row" if closing => {
                    while out.ends_with('\t') { out.pop(); }
                    out.push('\n');
                }
                "table:table" if closing => out.push('\n'),
                _ if closing && para_end.contains(&name) => out.push(if cell > 0 { ' ' } else { '\n' }),
                _ => {}
            }
        } else {
            let next = xml[i..].find('<').map_or(b.len(), |p| i + p);
            if in_text && skip == 0 {
                out.push_str(&decode_entities(&xml[i..next]));
            }
            i = next;
        }
    }
    // collapse runs of blank lines
    let mut res = String::with_capacity(out.len());
    let mut blank = 0;
    for l in out.split('\n') {
        if l.trim().is_empty() {
            blank += 1;
            if blank > 1 { continue; }
        } else {
            blank = 0;
        }
        res.push_str(l.trim_end());
        res.push('\n');
    }
    res.trim_end().to_string() + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_paragraphs_and_tables() {
        let x = "<w:document><w:body><w:p><w:r><w:t>Hello</w:t></w:r><w:r><w:t xml:space=\"preserve\"> &amp; bye</w:t></w:r></w:p><w:tbl><w:tr><w:tc><w:p><w:r><w:t>a</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>b</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:body></w:document>";
        assert_eq!(walk(x, Kind::Word), "Hello & bye\na\tb\n");
    }

    #[test]
    fn odf_text() {
        let x = "<office:document-content><office:automatic-styles><style:style>nope</style:style></office:automatic-styles><office:body><text:p>One<text:s/>two</text:p><text:p>Three</text:p></office:body></office:document-content>";
        assert_eq!(walk(x, Kind::Odf), "One two\nThree\n");
    }
}
