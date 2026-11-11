//! Documents for the GUI: immutable "working copies" (memory-mapped temp files) with
//! undo/redo, atomic save, and streaming transforms (format, fix, replace, splice).
//!
//! Every mutation writes a *new* working copy and pushes it on a bounded history, so
//! undo is instant and the original file on disk is never touched until Save.

use crate::fmtcmd::{self, Lang};
use crate::index::line_starts;
use crate::search::Matcher;
use crate::source::Source;
use crate::{external, jsonfix, jsonfmt, xmlfmt};
use anyhow::{Context, Result, bail};
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

const MAX_HISTORY: usize = 6;

pub struct Copy {
    pub src: Source,
    pub starts: Vec<u64>,
    pub crlf: bool,
}

impl Copy {
    pub fn from_source(src: Source) -> Copy {
        let starts = line_starts(src.bytes());
        let crlf = starts.first().is_some_and(|_| {
            let b = src.bytes();
            match memchr::memchr(b'\n', b) {
                Some(p) => p > 0 && b[p - 1] == b'\r',
                None => false,
            }
        });
        Copy { src, starts, crlf }
    }

    /// Map an (anonymous, already unlinked) temp file as a working copy.
    pub fn from_file(f: &std::fs::File) -> Result<Copy> {
        let src = Source::from_file(f, vec![])?;
        Ok(Copy::from_source(src))
    }

    pub fn bytes(&self) -> &[u8] {
        self.src.bytes()
    }
    pub fn count(&self) -> usize {
        self.starts.len()
    }
    pub fn size(&self) -> usize {
        self.src.len()
    }
    /// line including its terminator
    pub fn raw(&self, i: usize) -> &[u8] {
        let d = self.bytes();
        let s = self.starts[i] as usize;
        let e = if i + 1 < self.starts.len() { self.starts[i + 1] as usize } else { d.len() };
        &d[s..e]
    }
    /// line without terminator
    pub fn line(&self, i: usize) -> &[u8] {
        let mut l = self.raw(i);
        if l.last() == Some(&b'\n') {
            l = &l[..l.len() - 1];
        }
        if l.last() == Some(&b'\r') {
            l = &l[..l.len() - 1];
        }
        l
    }
    /// terminator of line i ("\n", "\r\n" or "" for an unterminated last line)
    pub fn eol(&self, i: usize) -> &'static [u8] {
        let r = self.raw(i);
        if r.ends_with(b"\r\n") {
            b"\r\n"
        } else if r.ends_with(b"\n") {
            b"\n"
        } else {
            b""
        }
    }
    pub fn default_eol(&self) -> &'static [u8] {
        if self.crlf { b"\r\n" } else { b"\n" }
    }
}

struct Hist {
    copies: Vec<Arc<Copy>>,
    cur: usize,
    saved: Option<usize>,
}

pub struct Doc {
    pub id: u64,
    path: Mutex<Option<PathBuf>>,
    name: Mutex<String>,
    h: Mutex<Hist>,
    version: AtomicU64,
    ops: Mutex<()>,
    /// set when the content (not the name) decides how the file is shown: a delimited .txt, an Excel sheet, "open as text"
    forced: Option<&'static str>,
}

#[derive(Clone, Debug)]
pub struct DocInfo {
    pub id: u64,
    pub name: String,
    pub path: Option<String>,
    pub lines: usize,
    pub size: usize,
    pub dirty: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    pub version: u64,
    pub kind: &'static str,
}

pub fn kind_of(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    if [".csv", ".tsv", ".psv", ".tab"].iter().any(|e| lower.ends_with(e)) {
        return "table";
    }
    kind_as_text(name)
}

/// the kind a file has when it is opened as plain text (never "table")
pub fn kind_as_text(name: &str) -> &'static str {
    match fmtcmd::lang_of_path(Path::new(name)).map(|l| l.0) {
        Some(Lang::Json) => "json",
        Some(Lang::Xml) => "xml",
        Some(Lang::Ext(_)) => "code",
        None => "text",
    }
}

/// Does a plain .txt / .dat (or extension-less) file look like a table: a tab, pipe, semicolon or comma that splits
/// (nearly) every one of the first lines into the same number of columns? Logs and prose do not qualify.
pub fn looks_delimited(c: &Copy, name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let ext = lower.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    if !matches!(ext, "txt" | "dat" | "data" | "text" | "" ) || c.count() < 2 || c.size() == 0 {
        return false;
    }
    let lines: Vec<&[u8]> = (0..c.count().min(400)).map(|i| c.line(i)).filter(|l| !l.iter().all(|b| b.is_ascii_whitespace())).take(200).collect();
    if lines.len() < 2 || lines.iter().any(|l| l.len() > 20_000) {
        return false;
    }
    for (d, min_cols) in [(b'\t', 2usize), (b'|', 2), (b';', 2), (b',', 3)] {
        let counts: Vec<usize> = lines.iter().map(|l| crate::table::split(l, d, usize::MAX).len()).collect();
        let mut freq: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
        for &n in &counts {
            *freq.entry(n).or_insert(0) += 1;
        }
        let (&k, &hits) = freq.iter().max_by_key(|(k, v)| (**v, **k)).unwrap();
        if k >= min_cols && hits * 10 >= lines.len() * 9 {
            return true;
        }
    }
    false
}

impl Doc {
    pub fn open(id: u64, path: &Path) -> Result<Doc> {
        Doc::open_as(id, path, "auto")
    }

    /// `mode`: "auto" (decided by name and content), "text" (plain text, never a table), "table", "json" or "xml"
    pub fn open_as(id: u64, path: &Path, mode: &str) -> Result<Doc> {
        let is_gz = path.extension().is_some_and(|e| e == "gz");
        let meta = std::fs::metadata(path).with_context(|| format!("cannot open {}", path.display()))?;
        if meta.is_dir() {
            bail!("{} is a directory", path.display());
        }
        // On Windows a memory-mapped file cannot be replaced or deleted (and other programs could not save it),
        // so work on a private copy there; elsewhere map the file directly (instant for any size).
        let src = if cfg!(windows) && !is_gz { Source::open_detached(path)? } else { Source::open(path)? };
        let mut name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        if is_gz {
            name = name.trim_end_matches(".gz").to_string();
        }
        let copy = Arc::new(Copy::from_source(src));
        let forced = if mode == "text" {
            Some(kind_as_text(&name))
        } else if mode == "table" {
            Some("table")
        } else if mode == "json" {
            Some("json")
        } else if mode == "xml" {
            Some("xml")
        } else if kind_of(&name) == "text" && looks_delimited(&copy, &name) {
            Some("table")
        } else {
            None
        };
        Ok(Doc {
            id,
            path: Mutex::new(if is_gz { None } else { Some(path.to_path_buf()) }),
            name: Mutex::new(name),
            h: Mutex::new(Hist { copies: vec![copy], cur: 0, saved: Some(0) }),
            version: AtomicU64::new(1),
            ops: Mutex::new(()),
            forced,
        })
    }

    /// a document made from generated content (an Excel sheet as CSV): no file on disk, so Save asks for a name
    pub fn from_generated(id: u64, name: &str, f: &std::fs::File, kind: &'static str) -> Result<Doc> {
        let copy = Arc::new(Copy::from_file(f)?);
        Ok(Doc {
            id,
            path: Mutex::new(None),
            name: Mutex::new(name.to_string()),
            h: Mutex::new(Hist { copies: vec![copy], cur: 0, saved: Some(0) }),
            version: AtomicU64::new(1),
            ops: Mutex::new(()),
            forced: Some(kind),
        })
    }

    #[cfg(test)]
    pub fn from_bytes(id: u64, name: &str, data: &[u8]) -> Result<Doc> {
        let mut f = tempfile::tempfile()?;
        f.write_all(data)?;
        f.flush()?;
        let copy = Arc::new(Copy::from_file(&f)?);
        Ok(Doc {
            id,
            path: Mutex::new(None),
            name: Mutex::new(name.to_string()),
            h: Mutex::new(Hist { copies: vec![copy], cur: 0, saved: None }),
            version: AtomicU64::new(1),
            ops: Mutex::new(()),
            forced: None,
        })
    }

    pub fn current(&self) -> Arc<Copy> {
        let h = self.h.lock().unwrap();
        h.copies[h.cur].clone()
    }

    pub fn version(&self) -> u64 {
        self.version.load(Ordering::Acquire)
    }

    pub fn path(&self) -> Option<PathBuf> {
        self.path.lock().unwrap().clone()
    }

    pub fn name(&self) -> String {
        self.name.lock().unwrap().clone()
    }

    pub fn info(&self) -> DocInfo {
        let h = self.h.lock().unwrap();
        let c = &h.copies[h.cur];
        let name = self.name();
        DocInfo {
            id: self.id,
            kind: self.forced.unwrap_or_else(|| kind_of(&name)),
            name,
            path: self.path().map(|p| p.display().to_string()),
            lines: c.count(),
            size: c.size(),
            dirty: h.saved != Some(h.cur),
            can_undo: h.cur > 0,
            can_redo: h.cur + 1 < h.copies.len(),
            version: self.version(),
        }
    }

    fn push(&self, c: Copy) {
        let mut h = self.h.lock().unwrap();
        let keep = h.cur + 1;
        h.copies.truncate(keep);
        if h.saved.is_some_and(|s| s >= keep) {
            h.saved = None;
        }
        h.copies.push(Arc::new(c));
        h.cur += 1;
        if h.copies.len() > MAX_HISTORY {
            h.copies.remove(0);
            h.cur -= 1;
            h.saved = match h.saved {
                Some(0) | None => None,
                Some(s) => Some(s - 1),
            };
        }
        self.version.fetch_add(1, Ordering::AcqRel);
    }

    pub fn undo(&self) -> bool {
        let _g = self.ops.lock().unwrap();
        let mut h = self.h.lock().unwrap();
        if h.cur == 0 {
            return false;
        }
        h.cur -= 1;
        self.version.fetch_add(1, Ordering::AcqRel);
        true
    }

    pub fn redo(&self) -> bool {
        let _g = self.ops.lock().unwrap();
        let mut h = self.h.lock().unwrap();
        if h.cur + 1 >= h.copies.len() {
            return false;
        }
        h.cur += 1;
        self.version.fetch_add(1, Ordering::AcqRel);
        true
    }

    /// Run `f` over the current copy, writing a new working copy. `f` returns Some(result) to
    /// commit, or None to discard (nothing changed).
    pub fn transform<R>(&self, f: impl FnOnce(&Copy, &mut dyn Write) -> Result<Option<R>>) -> Result<Option<R>> {
        self.transform_ex(false, f)
    }

    /// Like `transform`, but with `skip_identical` a result byte-identical to the current
    /// copy is discarded (no new version, document stays clean).
    pub fn transform_ex<R>(&self, skip_identical: bool, f: impl FnOnce(&Copy, &mut dyn Write) -> Result<Option<R>>) -> Result<Option<R>> {
        let _g = self.ops.lock().unwrap();
        let cur = self.current();
        let mut tmp = tempfile::tempfile().context("cannot create temp file (disk full?)")?;
        let res = {
            let mut w = BufWriter::with_capacity(1 << 20, &mut tmp);
            let r = f(&cur, &mut w)?;
            w.flush().context("write failed (disk full?)")?;
            r
        };
        match res {
            None => Ok(None),
            Some(r) => {
                let new = Copy::from_file(&tmp)?;
                if skip_identical && new.bytes() == cur.bytes() {
                    return Ok(None);
                }
                self.push(new);
                Ok(Some(r))
            }
        }
    }

    pub fn save(&self, to: Option<PathBuf>) -> Result<PathBuf> {
        let _g = self.ops.lock().unwrap();
        let target = match (&to, self.path()) {
            (Some(t), _) => t.clone(),
            (None, Some(p)) => p,
            (None, None) => bail!("this document has no file path yet — use Save As"),
        };
        let target = if target.exists() { crate::plat::canonical(&target)? } else { target };
        let dir = target.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
        let cur = self.current();
        let mut tmp = tempfile::Builder::new().prefix(".kempt-save-").tempfile_in(dir).with_context(|| format!("cannot write in {}", dir.display()))?;
        {
            let mut w = BufWriter::with_capacity(1 << 20, tmp.as_file_mut());
            w.write_all(cur.bytes())?;
            w.flush().context("write failed (disk full?)")?;
        }
        if let Ok(meta) = std::fs::metadata(&target) {
            let _ = std::fs::set_permissions(tmp.path(), meta.permissions());
        }
        tmp.persist(&target).map_err(|e| anyhow::anyhow!("cannot save to {}: {}", target.display(), e.error))?;
        {
            let mut h = self.h.lock().unwrap();
            h.saved = Some(h.cur);
        }
        *self.path.lock().unwrap() = Some(target.clone());
        *self.name.lock().unwrap() = target.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        self.version.fetch_add(1, Ordering::AcqRel);
        Ok(target)
    }

    // ───────────── transforms ─────────────

    /// Pretty-print (JSON / XML via streaming formatters; other languages via external tools).
    pub fn format(&self, indent: usize) -> Result<bool> {
        let name = self.name();
        let lang = fmtcmd::lang_of_path(Path::new(&name)).map(|l| l.0);
        let r = self.transform_ex(true, |cur, w| {
            match lang {
                Some(Lang::Json) => jsonfmt::format(cur.bytes(), &mut *w, Some(indent))?,
                Some(Lang::Xml) => xmlfmt::format(BufReader::new(cur.bytes()), &mut *w, Some(indent))?,
                Some(Lang::Ext(_)) => {
                    let ext = Path::new(&name).extension().unwrap().to_string_lossy().to_ascii_lowercase();
                    let tool = external::tool_for_ext(&ext).unwrap();
                    // external tools read from a file: dump the copy to a temp file first
                    use std::io::Seek;
                    let mut inp = tempfile::tempfile()?;
                    inp.write_all(cur.bytes())?;
                    inp.flush()?;
                    inp.seek(std::io::SeekFrom::Start(0))?;
                    let out = external::run(tool, &name, indent, Stdio::from(inp))?;
                    w.write_all(&out)?;
                }
                None => bail!("no formatter for this file type ({name})"),
            }
            Ok(Some(()))
        })?;
        Ok(r.is_some())
    }

    /// Repair + format JSON or XML. Returns the report of what was fixed (empty = nothing to fix).
    pub fn fix(&self, indent: usize) -> Result<jsonfix::Report> {
        let name = self.name();
        let lang = fmtcmd::lang_of_path(Path::new(&name)).map(|l| l.0);
        let cur = self.current();
        // sniff when the extension doesn't tell us
        let lang = match lang {
            Some(l @ (Lang::Json | Lang::Xml)) => l,
            _ => {
                let head = cur.bytes().iter().copied().find(|b| !b.is_ascii_whitespace());
                match head {
                    Some(b'{') | Some(b'[') => Lang::Json,
                    Some(b'<') => Lang::Xml,
                    _ => bail!("Fix supports JSON and XML structures (this file looks like neither)"),
                }
            }
        };
        let r = self.transform_ex(true, |cur, w| {
            let rep = match lang {
                Lang::Json => jsonfix::fix(cur.bytes(), &mut *w, Some(indent))?,
                _ => xmlfmt::format_opts(BufReader::new(cur.bytes()), &mut *w, Some(indent), true)?,
            };
            Ok(Some(rep))
        })?;
        Ok(r.unwrap_or_default())
    }

    /// Replace in every line satisfying the query. Returns (lines_changed, occurrences).
    pub fn replace_all(&self, m: &Matcher, rep: &str, literal: bool) -> Result<(u64, u64)> {
        self.transform(|cur, w| {
            let d = cur.bytes();
            let mut changed = 0u64;
            let mut occ = 0u64;
            let mut pos = 0usize;
            let mut out_needed = false;
            // first pass is combined with output; if nothing changed we discard the copy
            while pos < d.len() {
                let end = memchr::memchr(b'\n', &d[pos..]).map(|p| pos + p + 1).unwrap_or(d.len());
                let raw = &d[pos..end];
                let (content, eol): (&[u8], &[u8]) = if raw.ends_with(b"\r\n") {
                    (&raw[..raw.len() - 2], b"\r\n")
                } else if raw.ends_with(b"\n") {
                    (&raw[..raw.len() - 1], b"\n")
                } else {
                    (raw, b"")
                };
                match m.replace_line(content, rep, literal) {
                    Some(new) => {
                        changed += 1;
                        occ += m.count_occurrences(content) as u64;
                        out_needed = true;
                        w.write_all(&new)?;
                        w.write_all(eol)?;
                    }
                    None => w.write_all(raw)?,
                }
                pos = end;
            }
            Ok(if out_needed { Some((changed, occ)) } else { None })
        })
        .map(|o| o.unwrap_or((0, 0)))
    }

    /// Replace only on line `line` (0-based). Returns occurrences replaced.
    pub fn replace_line(&self, line: usize, m: &Matcher, rep: &str, literal: bool) -> Result<u64> {
        let cur = self.current();
        if line >= cur.count() {
            bail!("line {} is out of range", line + 1);
        }
        let content = cur.line(line);
        let Some(new) = m.replace_line(content, rep, literal) else { return Ok(0) };
        let occ = m.count_occurrences(content) as u64;
        let mut ins = new;
        ins.extend_from_slice(cur.eol(line));
        self.splice(line, 1, &ins)?;
        Ok(occ)
    }

    /// Replace `remove_n` lines starting at `start` with `insert` (raw bytes; should end with an
    /// EOL unless empty). Missing final newline of the file is preserved.
    pub fn splice(&self, start: usize, remove_n: usize, insert: &[u8]) -> Result<()> {
        self.transform(|cur, w| {
            let d = cur.bytes();
            let n = cur.count();
            let start = start.min(n);
            let end_line = (start + remove_n).min(n);
            let s_off = if start < n { cur.starts[start] as usize } else { d.len() };
            let e_off = if end_line < n { cur.starts[end_line] as usize } else { d.len() };
            let orig_final_nl = d.is_empty() || d.ends_with(b"\n");
            let mut ins = insert.to_vec();
            w.write_all(&d[..s_off])?;
            if s_off == d.len() && !d.is_empty() && !orig_final_nl && !ins.is_empty() {
                w.write_all(cur.default_eol())?; // terminate the old last line
            }
            if e_off == d.len() && !orig_final_nl && !ins.is_empty() {
                // keep "no newline at end of file"
                if ins.ends_with(b"\r\n") {
                    ins.truncate(ins.len() - 2);
                } else if ins.ends_with(b"\n") {
                    ins.pop();
                }
            }
            w.write_all(&ins)?;
            w.write_all(&d[e_off..])?;
            Ok(Some(()))
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(data: &[u8], name: &str) -> Doc {
        Doc::from_bytes(1, name, data).unwrap()
    }
    fn text(d: &Doc) -> String {
        String::from_utf8(d.current().bytes().to_vec()).unwrap()
    }

    #[test]
    fn copy_lines_and_eols() {
        let d = doc(b"a\r\nb\nc", "x.txt");
        let c = d.current();
        assert_eq!(c.count(), 3);
        assert_eq!(c.line(0), b"a");
        assert_eq!(c.eol(0), b"\r\n");
        assert_eq!(c.eol(1), b"\n");
        assert_eq!(c.eol(2), b"");
        assert!(c.crlf);
    }

    #[test]
    fn splice_replace_insert_delete() {
        let d = doc(b"a\nb\nc\n", "x.txt");
        d.splice(1, 1, b"B\n").unwrap();
        assert_eq!(text(&d), "a\nB\nc\n");
        d.splice(1, 0, b"new\n").unwrap();
        assert_eq!(text(&d), "a\nnew\nB\nc\n");
        d.splice(0, 1, b"").unwrap();
        assert_eq!(text(&d), "new\nB\nc\n");
        d.splice(3, 0, b"end\n").unwrap();
        assert_eq!(text(&d), "new\nB\nc\nend\n");
        d.splice(0, 100, b"").unwrap();
        assert_eq!(text(&d), "");
    }

    #[test]
    fn splice_preserves_missing_final_newline() {
        let d = doc(b"a\nb", "x.txt");
        d.splice(1, 1, b"B\n").unwrap();
        assert_eq!(text(&d), "a\nB"); // still no final newline
        let d = doc(b"a\nb", "x.txt");
        d.splice(2, 0, b"c\n").unwrap(); // append after an unterminated last line
        assert_eq!(text(&d), "a\nb\nc");
        let d = doc(b"a\nb\n", "x.txt");
        d.splice(2, 0, b"c\n").unwrap();
        assert_eq!(text(&d), "a\nb\nc\n");
    }

    #[test]
    fn undo_redo_dirty() {
        let d = doc(b"a\n", "x.txt");
        assert!(!d.info().can_undo);
        d.splice(0, 1, b"b\n").unwrap();
        let i = d.info();
        assert!(i.dirty && i.can_undo && !i.can_redo);
        assert!(d.undo());
        assert_eq!(text(&d), "a\n");
        assert!(d.info().can_redo);
        assert!(d.redo());
        assert_eq!(text(&d), "b\n");
        d.undo();
        d.splice(0, 1, b"c\n").unwrap(); // new branch drops redo
        assert!(!d.info().can_redo);
        assert!(!d.undo() || d.undo() == false);
    }

    #[test]
    fn history_is_bounded() {
        let d = doc(b"0\n", "x.txt");
        for i in 1..20 {
            d.splice(0, 1, format!("{i}\n").as_bytes()).unwrap();
        }
        let mut n = 0;
        while d.undo() {
            n += 1;
        }
        assert!(n <= MAX_HISTORY - 1, "{n}");
        assert!(d.info().dirty);
    }

    #[test]
    fn replace_all_and_line() {
        let d = doc(b"foo bar\nbaz\nfoo\n", "x.txt");
        let m = Matcher::new("foo", false, false, false, crate::search::Mode::Or).unwrap();
        let (lines, occ) = d.replace_all(&m, "X", true).unwrap();
        assert_eq!((lines, occ), (2, 2));
        assert_eq!(text(&d), "X bar\nbaz\nX\n");
        let none = Matcher::new("zzz", false, false, false, crate::search::Mode::Or).unwrap();
        let before = d.version();
        assert_eq!(d.replace_all(&none, "Y", true).unwrap(), (0, 0));
        assert_eq!(d.version(), before, "no-op must not create a version");
        let m2 = Matcher::new("bar baz", false, false, false, crate::search::Mode::Or).unwrap();
        assert_eq!(d.replace_line(0, &m2, "Q", true).unwrap(), 1);
        assert_eq!(text(&d), "Q bar\nbaz\nX\n".replace("Q bar", "X Q"));
    }

    #[test]
    fn replace_keeps_crlf() {
        let d = doc(b"a foo\r\nb\r\n", "x.txt");
        let m = Matcher::new("foo", false, false, false, crate::search::Mode::Or).unwrap();
        d.replace_all(&m, "bar", true).unwrap();
        assert_eq!(text(&d), "a bar\r\nb\r\n");
    }

    #[test]
    fn format_and_fix_json() {
        let d = doc(br#"{"a":1,"b":[1,2]}"#, "x.json");
        assert!(d.format(2).unwrap());
        assert!(text(&d).starts_with("{\n  \"a\": 1,"));
        let d = doc(b"{a:1, 'b':[1,2,],}", "x.json");
        let rep = d.fix(2).unwrap();
        assert!(!rep.is_empty());
        assert_eq!(text(&d), "{\n  \"a\": 1,\n  \"b\": [\n    1,\n    2\n  ]\n}\n");
        // valid JSON: an empty report; the unformatted text is laid out (a new version)
        let d = doc(br#"{"a":1}"#, "y.json");
        assert!(d.fix(2).unwrap().is_empty());
        // already clean + formatted: nothing changes, document stays clean
        let d = doc(b"{\n  \"a\": 1\n}\n", "y.json");
        let (v, dirty) = (d.version(), d.info().dirty);
        assert!(d.fix(2).unwrap().is_empty());
        assert_eq!(d.version(), v);
        assert_eq!(d.info().dirty, dirty);
        assert!(!d.format(2).unwrap());
    }

    #[test]
    fn fix_sniffs_and_rejects() {
        let d = doc(b"{'a':1,}", "data.txt");
        assert!(d.fix(2).is_ok());
        let d = doc(b"hello world", "data.txt");
        assert!(d.fix(2).is_err());
        let d = doc(b"<a><b>t", "x.xml");
        let rep = d.fix(2).unwrap();
        assert!(rep.contains_key("unclosed elements auto-closed"));
    }

    #[test]
    fn save_atomic_and_clean() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f.txt");
        std::fs::write(&p, "a\n").unwrap();
        let d = Doc::open(1, &p).unwrap();
        d.splice(0, 1, b"b\n").unwrap();
        assert!(d.info().dirty);
        // the file on disk is untouched until Save
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "a\n");
        d.save(None).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "b\n");
        assert!(!d.info().dirty);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        // save as
        let q = dir.path().join("g.txt");
        d.save(Some(q.clone())).unwrap();
        assert_eq!(std::fs::read_to_string(&q).unwrap(), "b\n");
        assert_eq!(d.name(), "g.txt");
        // undo makes it dirty again
        d.undo();
        assert!(d.info().dirty);
    }

    #[test]
    fn gz_opens_without_path() {
        use std::io::Write as _;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("l.log.gz");
        let mut e = flate2::write::GzEncoder::new(std::fs::File::create(&p).unwrap(), flate2::Compression::fast());
        e.write_all(b"x\ny\n").unwrap();
        e.finish().unwrap();
        let d = Doc::open(1, &p).unwrap();
        assert_eq!(d.info().lines, 2);
        assert_eq!(d.name(), "l.log");
        assert!(d.path().is_none());
        assert!(d.save(None).is_err());
    }
}
