//! Opening files as byte slices: mmap, with transparent gzip and
//! format-on-open (streaming JSON/XML) into temp files.

use crate::{fmtcmd, jsonfmt, xmlfmt};
use anyhow::{Context, Result};
use std::fs::File;
use std::io::{BufReader, BufWriter, Write};
use std::path::Path;
use std::sync::Arc;

pub struct Source {
    pub map: Option<Arc<memmap2::Mmap>>,
    /// keeps temp files alive for the lifetime of the Source
    _tmp: Vec<tempfile::NamedTempFile>,
}

static EMPTY: [u8; 0] = [];

impl Source {
    pub fn bytes(&self) -> &[u8] {
        match &self.map {
            Some(m) => &m[..],
            None => &EMPTY,
        }
    }

    pub fn len(&self) -> usize {
        self.bytes().len()
    }

    pub fn from_file(f: &File, tmp: Vec<tempfile::NamedTempFile>) -> Result<Source> {
        let map = if f.metadata()?.len() == 0 { None } else { Some(Arc::new(unsafe { memmap2::Mmap::map(f)? })) };
        Ok(Source { map, _tmp: tmp })
    }

    /// Copy the file into an anonymous temp file and map that, leaving the original unlocked.
    pub fn open_detached(path: &Path) -> Result<Source> {
        let f = File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
        let mut tmp = tempfile::tempfile()?;
        {
            let mut r = BufReader::with_capacity(1 << 20, f);
            let mut w = BufWriter::with_capacity(1 << 20, &mut tmp);
            std::io::copy(&mut r, &mut w).with_context(|| format!("cannot read {}", path.display()))?;
            w.flush()?;
        }
        Self::from_file(&tmp, vec![])
    }

    /// Plain open (transparently gunzips *.gz to a temp file).
    pub fn open(path: &Path) -> Result<Source> {
        let f = File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
        if path.extension().is_some_and(|e| e == "gz") {
            let mut tmp = tempfile::tempfile()?;
            {
                let mut dec = flate2::read::MultiGzDecoder::new(BufReader::with_capacity(1 << 20, f));
                let mut w = BufWriter::with_capacity(1 << 20, &mut tmp);
                std::io::copy(&mut dec, &mut w).with_context(|| format!("cannot decompress {}", path.display()))?;
                w.flush()?;
            }
            return Self::from_file(&tmp, vec![]);
        }
        Self::from_file(&f, vec![])
    }

    /// Open and, for JSON/XML, stream-format into a temp file so the result is human-readable.
    pub fn open_formatted(path: &Path, indent: usize) -> Result<(Source, FormatNote)> {
        let base = Source::open(path)?;
        // name used to decide the language: strip a trailing .gz
        let mut name = path.to_path_buf();
        if name.extension().is_some_and(|e| e == "gz") {
            name.set_extension("");
        }
        let is_json = match fmtcmd::lang_of_path(&name).map(|l| l.0) {
            Some(fmtcmd::Lang::Json) => true,
            Some(fmtcmd::Lang::Xml) => false,
            _ => return Ok((base, FormatNote::NotApplicable)),
        };
        let mut tmp = tempfile::NamedTempFile::new()?;
        let res = {
            let mut w = BufWriter::with_capacity(1 << 20, tmp.as_file_mut());
            let r = if is_json {
                jsonfmt::format(base.bytes(), &mut w, Some(indent))
            } else {
                xmlfmt::format(BufReader::new(base.bytes()), &mut w, Some(indent))
            };
            r.and_then(|_| w.flush().map_err(Into::into))
        };
        match res {
            Ok(()) => {
                let f2 = tmp.reopen()?;
                Ok((Self::from_file(&f2, vec![tmp])?, FormatNote::Formatted))
            }
            // malformed: fall back to raw so the user can still read (and find) the problem
            Err(e) => Ok((base, FormatNote::Failed(format!("{e:#}")))),
        }
    }
}

pub enum FormatNote {
    Formatted,
    NotApplicable,
    Failed(String),
}

pub fn looks_binary(data: &[u8]) -> bool {
    let n = data.len().min(8192);
    memchr::memchr(0, &data[..n]).is_some()
}
