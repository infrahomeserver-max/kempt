//! `kempt format`: format / check / minify files, atomically, in constant memory
//! for JSON, JSONL and XML.

use crate::{external, jsonfmt, xmlfmt};
use anyhow::{Context, Result, bail};
use rayon::prelude::*;
use std::fs::File;
use std::io::{self, BufReader, BufWriter, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

#[derive(Clone)]
pub struct FmtOpts {
    pub check: bool,
    pub minify: bool,
    pub indent: usize,
    pub output: Option<String>,
    pub max_size_mb: u64,
    pub force: bool,
    pub lang: Option<String>,
    pub quiet: bool,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Lang {
    Json,
    Xml,
    Ext(&'static str), // tool name
}

pub fn lang_of_ext(ext: &str) -> Option<(Lang, Option<&'static external::ExtTool>)> {
    let e = ext.to_ascii_lowercase();
    match e.as_str() {
        "json" | "jsonl" | "ndjson" | "geojson" | "jsonc" => Some((Lang::Json, None)),
        "xml" | "svg" | "xsd" | "xsl" | "xslt" | "rss" | "atom" | "plist" | "csproj" | "pom" | "wsdl" | "kml" | "gpx" => Some((Lang::Xml, None)),
        _ => external::tool_for_ext(&e).map(|t| (Lang::Ext(t.name), Some(t))),
    }
}

pub fn lang_of_path(p: &Path) -> Option<(Lang, Option<&'static external::ExtTool>)> {
    lang_of_ext(p.extension()?.to_str()?)
}

/// Writer that checks output against an existing file, streaming (no mmap, O(chunk) memory),
/// aborting on the first mismatch.
struct CompareWriter {
    file: File,
    len: u64,
    pos: u64,
    scratch: Vec<u8>,
}

impl CompareWriter {
    fn new(path: &Path) -> io::Result<Self> {
        let file = File::open(path)?;
        let len = file.metadata()?.len();
        Ok(CompareWriter { file, len, pos: 0, scratch: Vec::new() })
    }
    fn matched_all(&self) -> bool {
        self.pos == self.len
    }
}

#[derive(Debug)]
struct Mismatch;
impl std::fmt::Display for Mismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "mismatch")
    }
}
impl std::error::Error for Mismatch {}

impl Write for CompareWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.pos + buf.len() as u64 > self.len {
            return Err(io::Error::other(Mismatch));
        }
        if self.scratch.len() < buf.len() {
            self.scratch.resize(buf.len(), 0);
        }
        self.file.read_exact(&mut self.scratch[..buf.len()])?;
        if &self.scratch[..buf.len()] != buf {
            return Err(io::Error::other(Mismatch));
        }
        self.pos += buf.len() as u64;
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn is_mismatch(e: &anyhow::Error) -> bool {
    e.chain().any(|c| {
        c.downcast_ref::<io::Error>().and_then(|io| io.get_ref()).is_some_and(|inner| inner.downcast_ref::<Mismatch>().is_some())
    })
}

struct Progress<R> {
    inner: R,
    total: u64,
    done: u64,
    last: Instant,
    start: Instant,
    label: String,
    enabled: bool,
}

impl<R: Read> Read for Progress<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.done += n as u64;
        if self.enabled && self.last.elapsed().as_millis() > 400 {
            self.last = Instant::now();
            let secs = self.start.elapsed().as_secs_f64().max(0.001);
            eprint!(
                "\r  {} {:>3}%  {:>6.0} MB/s ",
                self.label,
                (self.done * 100 / self.total.max(1)).min(100),
                self.done as f64 / 1e6 / secs
            );
        }
        Ok(n)
    }
}

impl<R> Drop for Progress<R> {
    fn drop(&mut self) {
        if self.enabled && self.last != self.start {
            eprint!("\r{}\r", " ".repeat(60));
        }
    }
}

/// Stream the formatted version of `path` into `out` (for any supported language).
fn produce(path: &Path, lang: Lang, o: &FmtOpts, out: &mut dyn Write) -> Result<()> {
    let ind = if o.minify { None } else { Some(o.indent) };
    match lang {
        Lang::Json | Lang::Xml => {
            let f = File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
            let total = f.metadata().map(|m| m.len()).unwrap_or(0);
            let prog = Progress {
                inner: f,
                total,
                done: 0,
                last: Instant::now(),
                start: Instant::now(),
                label: path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
                enabled: total > 50 << 20 && io::stderr().is_terminal() && !o.quiet,
            };
            let mut w = BufWriter::with_capacity(1 << 20, out);
            let r = if lang == Lang::Json {
                jsonfmt::format(prog, &mut w, ind)
            } else {
                xmlfmt::format(BufReader::with_capacity(1 << 20, prog), &mut w, ind)
            };
            r?;
            w.flush()?;
            Ok(())
        }
        Lang::Ext(_) => {
            if o.minify {
                bail!("--minify is only supported for JSON and XML");
            }
            let tool = external::tool_for_ext(&path.extension().unwrap().to_string_lossy().to_ascii_lowercase()).unwrap();
            let f = File::open(path)?;
            let bytes = external::run(tool, &path.to_string_lossy(), o.indent, Stdio::from(f))?;
            out.write_all(&bytes)?;
            Ok(())
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Unchanged,
    Changed,
    Skipped(String),
}

pub fn format_file(path: &Path, o: &FmtOpts) -> Result<Outcome> {
    let Some((lang, _)) = lang_of_path(path) else {
        return Ok(Outcome::Skipped("unsupported file type".into()));
    };
    let size = std::fs::metadata(path).with_context(|| format!("cannot stat {}", path.display()))?.len();
    if matches!(lang, Lang::Ext(_)) && !o.force && size > o.max_size_mb << 20 {
        return Ok(Outcome::Skipped(format!(
            "{:.0} MB exceeds --max-size {} MB for non-streaming formatter (use --force)",
            size as f64 / 1048576.0,
            o.max_size_mb
        )));
    }

    // pass 1: is it already formatted? (streamed comparison, early exit on first mismatch)
    let mut cw = CompareWriter::new(path).with_context(|| format!("cannot open {}", path.display()))?;
    let same = match produce(path, lang, o, &mut cw) {
        Ok(()) => cw.matched_all(),
        Err(e) if is_mismatch(&e) => false,
        Err(e) => return Err(e.context(path.display().to_string())),
    };
    drop(cw); // release the read handle: on Windows a file that is still open cannot be replaced
    if same {
        return Ok(Outcome::Unchanged);
    }
    if o.check {
        return Ok(Outcome::Changed);
    }

    // pass 2: write atomically (temp file in the same dir, then rename).
    // Resolve symlinks first so the link itself is never replaced by a regular file.
    let real = crate::plat::canonical(path).with_context(|| format!("cannot resolve {}", path.display()))?;
    let path = real.as_path();
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let mut tmp = tempfile::Builder::new().prefix(".kempt-").tempfile_in(dir).with_context(|| format!("cannot create temp file in {}", dir.display()))?;
    {
        let mut w = BufWriter::with_capacity(1 << 20, tmp.as_file_mut());
        produce(path, lang, o, &mut w).map_err(|e| e.context(path.display().to_string()))?;
        w.flush().with_context(|| "write failed (disk full?)")?;
    }
    if let Ok(meta) = std::fs::metadata(path) {
        let _ = std::fs::set_permissions(tmp.path(), meta.permissions());
    }
    tmp.persist(path).map_err(|e| anyhow::anyhow!("cannot replace {}: {}", path.display(), e.error))?;
    Ok(Outcome::Changed)
}

fn collect(paths: &[String]) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for p in paths {
        let pb = PathBuf::from(p);
        if pb.is_dir() {
            for e in walkdir::WalkDir::new(&pb).into_iter().filter_entry(|e| {
                let n = e.file_name().to_string_lossy();
                e.depth() == 0 || !(n.starts_with('.') || n == "node_modules" || n == "target" || n == "__pycache__")
            }) {
                let e = e?;
                if e.file_type().is_file() && lang_of_path(e.path()).is_some() {
                    out.push(e.into_path());
                }
            }
        } else if pb.is_file() {
            out.push(pb);
        } else {
            bail!("no such file or directory: {p}");
        }
    }
    out.sort();
    Ok(out)
}

fn run_stdin(o: &FmtOpts) -> Result<i32> {
    let Some(l) = &o.lang else { bail!("reading from stdin requires --lang (json, xml, python, ...)") };
    let ext = match l.as_str() {
        "python" => "py",
        "javascript" => "js",
        "typescript" => "ts",
        "shell" => "sh",
        "markdown" => "md",
        x => x,
    };
    let (lang, tool) = lang_of_ext(ext).ok_or_else(|| anyhow::anyhow!("unknown --lang '{l}'"))?;
    let ind = if o.minify { None } else { Some(o.indent) };
    let stdout = io::stdout();
    let mut w = BufWriter::with_capacity(1 << 20, stdout.lock());
    match lang {
        Lang::Json => jsonfmt::format(io::stdin().lock(), &mut w, ind)?,
        Lang::Xml => xmlfmt::format(BufReader::with_capacity(1 << 20, io::stdin().lock()), &mut w, ind)?,
        Lang::Ext(_) => {
            let bytes = external::run(tool.unwrap(), &format!("stdin.{ext}"), o.indent, Stdio::inherit())?;
            w.write_all(&bytes)?;
        }
    }
    w.flush()?;
    Ok(0)
}

pub fn run(paths: Vec<String>, o: FmtOpts) -> Result<i32> {
    if paths.len() == 1 && paths[0] == "-" {
        return run_stdin(&o);
    }
    let files = collect(&paths)?;
    if files.is_empty() {
        bail!("no supported files found");
    }

    // -o: single input, write result elsewhere (or '-' for stdout)
    if let Some(out) = &o.output {
        if files.len() != 1 {
            bail!("-o/--output needs exactly one input file");
        }
        let (lang, _) = lang_of_path(&files[0]).ok_or_else(|| anyhow::anyhow!("unsupported file type"))?;
        if out == "-" {
            let stdout = io::stdout();
            let mut w = stdout.lock();
            produce(&files[0], lang, &o, &mut w)?;
        } else {
            let dir = Path::new(out).parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
            let mut tmp = tempfile::Builder::new().prefix(".kempt-").tempfile_in(dir)?;
            produce(&files[0], lang, &o, tmp.as_file_mut())?;
            tmp.persist(out).map_err(|e| anyhow::anyhow!("{}", e.error))?;
        }
        return Ok(0);
    }

    let changed = AtomicUsize::new(0);
    let errors = AtomicUsize::new(0);
    let skipped = AtomicUsize::new(0);
    let multi = files.len() > 1;
    let handle = |p: &PathBuf| {
        match format_file(p, &o) {
            Ok(Outcome::Unchanged) => {
                if !o.quiet && multi {
                    eprintln!("  unchanged  {}", p.display());
                }
            }
            Ok(Outcome::Changed) => {
                changed.fetch_add(1, Ordering::Relaxed);
                if !o.quiet {
                    eprintln!("  {}  {}", if o.check { "would reformat" } else { "formatted" }, p.display());
                }
            }
            Ok(Outcome::Skipped(why)) => {
                skipped.fetch_add(1, Ordering::Relaxed);
                eprintln!("  skipped    {}  ({why})", p.display());
            }
            Err(e) => {
                errors.fetch_add(1, Ordering::Relaxed);
                eprintln!("  error      {e:#}");
            }
        };
    };
    if multi {
        files.par_iter().for_each(handle);
    } else {
        files.iter().for_each(handle);
    }
    let (c, e, s) = (changed.load(Ordering::Relaxed), errors.load(Ordering::Relaxed), skipped.load(Ordering::Relaxed));
    if !o.quiet {
        eprintln!(
            "{} file(s): {} {}, {} unchanged, {} skipped, {} error(s)",
            files.len(),
            c,
            if o.check { "need formatting" } else { "reformatted" },
            files.len() - c - e - s,
            s,
            e
        );
    }
    Ok(if e > 0 { 2 } else if o.check && c > 0 { 1 } else { 0 })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> FmtOpts {
        FmtOpts { check: false, minify: false, indent: 2, output: None, max_size_mb: 20, force: false, lang: None, quiet: true }
    }

    #[test]
    fn format_in_place_atomic_and_idempotent() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.json");
        std::fs::write(&p, r#"{"a":[1,2],"b":{}}"#).unwrap();
        assert_eq!(format_file(&p, &opts()).unwrap(), Outcome::Changed);
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "{\n  \"a\": [\n    1,\n    2\n  ],\n  \"b\": {}\n}\n");
        assert_eq!(format_file(&p, &opts()).unwrap(), Outcome::Unchanged);
        // no stray temp files
        assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 1);
    }

    #[test]
    fn invalid_json_leaves_file_untouched() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("bad.json");
        std::fs::write(&p, r#"{"a":1,}"#).unwrap();
        assert!(format_file(&p, &opts()).is_err());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), r#"{"a":1,}"#);
        assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 1);
    }

    #[test]
    fn check_mode_does_not_modify() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.xml");
        std::fs::write(&p, "<a><b/></a>").unwrap();
        let mut o = opts();
        o.check = true;
        assert_eq!(format_file(&p, &o).unwrap(), Outcome::Changed);
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "<a><b/></a>");
    }

    #[test]
    fn empty_file_ok() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("e.json");
        std::fs::write(&p, "").unwrap();
        assert_eq!(format_file(&p, &opts()).unwrap(), Outcome::Unchanged);
    }

    #[test]
    fn minify_roundtrip() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("m.json");
        std::fs::write(&p, "{\n  \"a\": 1\n}\n").unwrap();
        let mut o = opts();
        o.minify = true;
        format_file(&p, &o).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "{\"a\":1}\n");
    }

    #[cfg(unix)]
    #[test]
    fn symlink_target_formatted_link_kept() {
        let d = tempfile::tempdir().unwrap();
        let real = d.path().join("real.json");
        let link = d.path().join("link.json");
        std::fs::write(&real, r#"{"a":1}"#).unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert_eq!(format_file(&link, &opts()).unwrap(), Outcome::Changed);
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read_to_string(&real).unwrap(), "{\n  \"a\": 1\n}\n");
    }

    #[test]
    fn unsupported_skipped() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("x.bin");
        std::fs::write(&p, "zz").unwrap();
        assert!(matches!(format_file(&p, &opts()).unwrap(), Outcome::Skipped(_)));
    }
}
