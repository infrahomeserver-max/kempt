//! Large-file viewer: mmap + background line index, lazy formatting,
//! regex search/filter, follow mode, JSON detail pane, timestamp jump.

use crate::index::LineIndex;
use crate::jsonfmt;
use crate::source::{FormatNote, Source, looks_binary};
use crate::theme::{Painter, Rgb};
use crate::tui::{self, Prompt, PromptResult, Term, fit};
use anyhow::{Result, bail};
use crossterm::event::{Event, KeyCode, KeyModifiers, MouseEventKind};
use regex::bytes::{Regex, RegexBuilder};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct ViewArgs {
    pub path: PathBuf,
    pub raw: bool,
    pub follow: bool,
    pub line: Option<usize>,
    pub filter: Option<String>,
    pub indent: usize,
    pub force: bool,
    pub painter: Painter,
    pub plain: bool,
}

pub fn smart_regex(pat: &str) -> Result<Regex> {
    let ci = !pat.chars().any(|c| c.is_uppercase());
    Ok(RegexBuilder::new(pat).case_insensitive(ci).unicode(false).build()?)
}

pub const ERROR_RE: &str = r"\b(ERROR|ERR|FATAL|CRITICAL|CRIT|SEVERE|PANIC|Exception)\b";
pub const WARN_RE: &str = r"\b(WARN|WARNING|ERROR|ERR|FATAL|CRITICAL|CRIT|SEVERE|PANIC|Exception)\b";

fn level_regex() -> Regex {
    Regex::new(r"\b(TRACE|DEBUG|INFO|NOTICE|WARN|WARNING|ERROR|ERR|FATAL|CRITICAL|CRIT|SEVERE|PANIC)\b").unwrap()
}

fn ts_regex() -> Regex {
    Regex::new(r"^\[?\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}(?:[.,]\d+)?(?:Z|[+-]\d{2}:?\d{2})?\]?").unwrap()
}

struct Filter {
    pattern: String,
    rows: Arc<Mutex<Vec<u32>>>,
    done: Arc<AtomicBool>,
}

struct Search {
    re: Regex,
    pattern: String,
    count: Arc<AtomicU64>,
    count_done: Arc<AtomicBool>,
}

enum Mode {
    Normal,
    Prompt(PromptKind, Prompt),
    Help,
}

#[derive(Clone, Copy, PartialEq)]
enum PromptKind {
    Search,
    Filter,
    Goto,
    Time,
}

struct V {
    path: PathBuf,
    src: Source,
    idx: LineIndex,
    p: Painter,
    note: Option<String>,
    top: usize,
    sel: usize,
    hoff: usize,
    filter: Option<Filter>,
    search: Option<Search>,
    follow: bool,
    can_follow: bool,
    detail: bool,
    detail_off: usize,
    mode: Mode,
    msg: String,
    ino: u64,
    ts_re: Regex,
    lvl_re: Regex,
    indent: usize,
}

fn inode(path: &Path) -> u64 {
    crate::plat::file_id(path)
}

impl V {
    fn nrows(&self) -> usize {
        match &self.filter {
            Some(f) => f.rows.lock().unwrap().len(),
            None => self.idx.count(),
        }
    }

    fn line_of_row(&self, r: usize) -> Option<usize> {
        match &self.filter {
            Some(f) => f.rows.lock().unwrap().get(r).map(|&x| x as usize),
            None => {
                if r < self.idx.count() {
                    Some(r)
                } else {
                    None
                }
            }
        }
    }

    fn line_bytes(&self, line: usize) -> Option<&[u8]> {
        let (s, e) = self.idx.range(line, self.src.bytes())?;
        Some(&self.src.bytes()[s..e])
    }

    fn clamp(&mut self, bh: usize) {
        let n = self.nrows();
        if n == 0 {
            self.top = 0;
            self.sel = 0;
            return;
        }
        self.sel = self.sel.min(n - 1);
        let max_top = n.saturating_sub(bh);
        self.top = self.top.min(max_top);
        if self.sel < self.top {
            self.top = self.sel;
        }
        if self.sel >= self.top + bh {
            self.top = self.sel + 1 - bh;
        }
    }

    fn move_sel(&mut self, delta: isize) {
        let n = self.nrows() as isize;
        if n == 0 {
            return;
        }
        self.sel = (self.sel as isize + delta).clamp(0, n - 1) as usize;
    }

    fn set_filter(&mut self, pat: &str) {
        if pat.is_empty() {
            self.filter = None;
            self.msg = "filter cleared".into();
            return;
        }
        let re = match smart_regex(pat) {
            Ok(r) => r,
            Err(e) => {
                self.msg = format!("bad regex: {}", e.to_string().lines().last().unwrap_or(""));
                return;
            }
        };
        let rows: Arc<Mutex<Vec<u32>>> = Arc::new(Mutex::new(Vec::new()));
        let done = Arc::new(AtomicBool::new(false));
        if let Some(map) = self.src.map.clone() {
            let (rows2, done2) = (rows.clone(), done.clone());
            std::thread::spawn(move || {
                let data = &map[..];
                let mut line = 0u32;
                let mut pos = 0usize;
                let mut batch: Vec<u32> = Vec::new();
                while pos < data.len() {
                    let end = memchr::memchr(b'\n', &data[pos..]).map(|p| pos + p).unwrap_or(data.len());
                    let mut e = end;
                    if e > pos && data[e - 1] == b'\r' {
                        e -= 1;
                    }
                    if re.is_match(&data[pos..e]) {
                        batch.push(line);
                        if batch.len() >= 4096 {
                            rows2.lock().unwrap().extend_from_slice(&batch);
                            batch.clear();
                        }
                    }
                    line += 1;
                    pos = end + 1;
                }
                rows2.lock().unwrap().extend_from_slice(&batch);
                done2.store(true, Ordering::Release);
            });
        } else {
            done.store(true, Ordering::Release);
        }
        self.filter = Some(Filter { pattern: pat.to_string(), rows, done });
        self.top = 0;
        self.sel = 0;
        self.msg = format!("filter: {pat}");
    }

    fn set_search(&mut self, pat: &str) -> bool {
        match smart_regex(pat) {
            Ok(re) => {
                let count = Arc::new(AtomicU64::new(0));
                let count_done = Arc::new(AtomicBool::new(false));
                if let Some(map) = self.src.map.clone() {
                    let (c2, d2, re2) = (count.clone(), count_done.clone(), re.clone());
                    std::thread::spawn(move || {
                        let mut n = 0u64;
                        for _ in re2.find_iter(&map[..]) {
                            n += 1;
                            if n % 4096 == 0 {
                                c2.store(n, Ordering::Relaxed);
                            }
                        }
                        c2.store(n, Ordering::Relaxed);
                        d2.store(true, Ordering::Release);
                    });
                }
                self.search = Some(Search { re, pattern: pat.to_string(), count, count_done });
                true
            }
            Err(e) => {
                self.msg = format!("bad regex: {}", e.to_string().lines().last().unwrap_or(""));
                false
            }
        }
    }

    /// move selection to next/previous matching row, wrapping around
    fn find(&mut self, forward: bool) {
        let Some(s) = &self.search else {
            self.msg = "no search pattern (press /)".into();
            return;
        };
        let re = s.re.clone();
        let n = self.nrows();
        if n == 0 {
            return;
        }
        for step in 1..=n {
            let r = if forward { (self.sel + step) % n } else { (self.sel + n - step % n) % n };
            if let Some(line) = self.line_of_row(r) {
                if let Some(b) = self.line_bytes(line) {
                    if re.is_match(b) {
                        let wrapped = if forward { r <= self.sel } else { r >= self.sel };
                        self.sel = r;
                        self.msg = if wrapped { "search wrapped".into() } else { String::new() };
                        return;
                    }
                }
            }
        }
        self.msg = "pattern not found".into();
    }

    fn goto_line(&mut self, n: usize) {
        if self.filter.is_some() {
            // nearest filtered row at or after that line
            if let Some(f) = &self.filter {
                let rows = f.rows.lock().unwrap();
                let r = rows.partition_point(|&l| (l as usize) < n.saturating_sub(1));
                drop(rows);
                self.sel = r;
            }
        } else {
            self.sel = n.saturating_sub(1);
        }
    }

    fn ts_key<'a>(&self, line: &'a [u8]) -> Option<&'a [u8]> {
        let s = line.strip_prefix(b"[").unwrap_or(line);
        if s.first().is_some_and(|c| c.is_ascii_digit()) { Some(s) } else { None }
    }

    fn goto_time(&mut self, target: &str) {
        if self.filter.is_some() {
            self.msg = "clear the filter first (timestamp jump works on the full file)".into();
            return;
        }
        let t = target.trim().as_bytes().to_vec();
        if t.is_empty() {
            return;
        }
        let n = self.idx.count();
        let (mut lo, mut hi) = (0usize, n);
        while lo < hi {
            let mid = (lo + hi) / 2;
            let mut found = None;
            for j in mid..(mid + 200).min(hi) {
                if let Some(b) = self.line_bytes(j) {
                    if let Some(k) = self.ts_key(b) {
                        found = Some((j, k.to_vec()));
                        break;
                    }
                }
            }
            match found {
                None => hi = mid,
                Some((j, k)) => {
                    let m = k.len().min(t.len());
                    if k[..m] < t[..m] {
                        lo = j + 1;
                    } else {
                        hi = mid;
                    }
                }
            }
        }
        // the bound can land on continuation lines (stack traces etc.) of the previous entry: skip them
        while lo < n && self.line_bytes(lo).and_then(|b| self.ts_key(b)).is_none() {
            lo += 1;
        }
        if lo >= n {
            self.msg = "no entries at or after that time".into();
            self.sel = n.saturating_sub(1);
        } else {
            self.sel = lo;
            self.msg = format!("jumped to line {}", lo + 1);
        }
    }

    /// follow mode / file growth. Returns true when something changed.
    fn refresh(&mut self) -> bool {
        if !self.can_follow {
            return false;
        }
        let Ok(md) = std::fs::metadata(&self.path) else { return false };
        let len = md.len() as usize;
        let cur = self.src.len();
        let ino = inode(&self.path);
        if ino != self.ino || len < cur {
            // rotated or truncated: reload from scratch
            if let Ok(s) = Source::open(&self.path) {
                self.idx.reset();
                self.filter = None;
                self.src = s;
                self.ino = ino;
                if let Some(m) = self.src.map.clone() {
                    self.idx.spawn(m);
                } else {
                    self.idx.sh.done.store(true, Ordering::Release);
                }
                self.sel = 0;
                self.top = 0;
                self.msg = "file rotated/truncated — reloaded".into();
                return true;
            }
            return false;
        }
        if len > cur && self.idx.is_done() {
            if let Ok(s) = Source::open(&self.path) {
                self.src = s;
                self.idx.extend(self.src.bytes());
                if self.filter.is_some() {
                    self.msg = "file grew — re-apply filter (&) to include new lines".into();
                }
                if self.follow {
                    self.sel = usize::MAX;
                }
                return true;
            }
        }
        false
    }

    // ───────────── rendering ─────────────

    fn style_line(&self, bytes: &[u8], hoff: usize, width: usize, base_bg: Option<Rgb>) -> String {
        let t = &self.p.t;
        let p = &self.p;
        // limit work for pathological very long lines
        let cut = bytes.len().min(hoff + width + 512);
        let slice = &bytes[..cut];
        // regions (byte ranges) with a style id
        let mut regions: Vec<(usize, usize, u8)> = Vec::new();
        if let Some(m) = self.ts_re.find(slice) {
            regions.push((m.start(), m.end(), 1));
        }
        let head = &slice[..slice.len().min(160)];
        if let Some(m) = self.lvl_re.find(head) {
            let id = match &head[m.start()..m.end()] {
                b"ERROR" | b"ERR" | b"FATAL" | b"CRITICAL" | b"CRIT" | b"SEVERE" | b"PANIC" => 2,
                b"WARN" | b"WARNING" => 3,
                b"INFO" | b"NOTICE" => 4,
                _ => 5,
            };
            regions.push((m.start(), m.end(), id));
        }
        if let Some(s) = &self.search {
            for m in s.re.find_iter(slice) {
                regions.push((m.start(), m.end(), 6));
            }
        }
        // per-byte style map for the visible slice (regions are few)
        let mut style = vec![0u8; slice.len()];
        for (a, b, id) in &regions {
            for s in &mut style[*a..*b] {
                *s = *id;
            }
        }
        let text = String::from_utf8_lossy(slice);
        // lossy conversion may change byte offsets only for invalid UTF-8; map via char_indices over original when valid
        let valid = matches!(std::str::from_utf8(slice), Ok(_));
        let mut out = String::new();
        let apply = |out: &mut String, id: u8| {
            out.push_str(p.reset());
            if let Some(bg) = base_bg {
                out.push_str(&p.bg(bg));
            }
            match id {
                1 => out.push_str(&p.fg(t.ts_fg)),
                2 => {
                    out.push_str(&p.fg(t.lvl_error));
                    out.push_str(p.bold());
                }
                3 => {
                    out.push_str(&p.fg(t.lvl_warn));
                    out.push_str(p.bold());
                }
                4 => out.push_str(&p.fg(t.lvl_info)),
                5 => out.push_str(&p.fg(t.lvl_debug)),
                6 => {
                    out.push_str(&p.bg(t.match_bg));
                    out.push_str(&p.fg(Rgb(255, 255, 255)));
                    out.push_str(p.bold());
                }
                _ => out.push_str(&p.fg(t.fg)),
            }
        };
        let mut cur = 255u8;
        let mut col = 0usize;
        let mut emitted = 0usize;
        let mut byte_pos = 0usize;
        for ch in text.chars() {
            let id = if valid { style.get(byte_pos).copied().unwrap_or(0) } else { 0 };
            byte_pos += ch.len_utf8();
            let (rep, n): (&str, usize) = match ch {
                '\t' => ("    ", 4),
                c if (c as u32) < 32 || c as u32 == 127 => ("·", 1),
                _ => ("", 1),
            };
            // handle a possibly multi-column char (tab) cell by cell
            for k in 0..n {
                if col >= hoff {
                    if emitted >= width {
                        out.push_str(p.reset());
                        return out;
                    }
                    if id != cur {
                        apply(&mut out, id);
                        cur = id;
                    }
                    if rep.is_empty() {
                        out.push(ch);
                    } else if ch == '\t' {
                        out.push(' ');
                    } else {
                        out.push_str(rep);
                    }
                    emitted += 1;
                }
                col += 1;
                let _ = k;
            }
        }
        // pad (so selected-row background extends)
        if base_bg.is_some() {
            if cur != 0 {
                apply(&mut out, 0);
            }
            for _ in emitted..width {
                out.push(' ');
            }
        }
        out.push_str(p.reset());
        out
    }

    fn render_row(&self, r: usize, num_w: usize, text_w: usize) -> String {
        let t = &self.p.t;
        let p = &self.p;
        let selected = r == self.sel;
        let bg = if selected { Some(t.sel_bg) } else { None };
        match self.line_of_row(r) {
            None => format!("{}~{}", p.fg(t.dim_fg), p.reset()),
            Some(line) => {
                let mut s = String::new();
                if let Some(b) = bg {
                    s.push_str(&p.bg(b));
                }
                s.push_str(&p.fg(t.dim_fg));
                s.push_str(&format!("{:>w$} ", line + 1, w = num_w));
                s.push_str(p.reset());
                match self.line_bytes(line) {
                    Some(b) => s.push_str(&self.style_line(b, self.hoff, text_w, bg)),
                    None => s.push_str(&format!("{}…{}", p.fg(t.dim_fg), p.reset())),
                }
                s
            }
        }
    }

    fn detail_lines(&self, width: usize) -> Vec<String> {
        let t = &self.p.t;
        let p = &self.p;
        let Some(line) = self.line_of_row(self.sel) else { return vec![] };
        let Some(b) = self.line_bytes(line) else { return vec![] };
        let mut out = Vec::new();
        let trimmed = b.trim_ascii();
        let mut pretty = Vec::new();
        if trimmed.first().is_some_and(|c| *c == b'{' || *c == b'[') && jsonfmt::format(trimmed, &mut pretty, Some(self.indent)).is_ok() {
            let text = String::from_utf8_lossy(&pretty).into_owned();
            for l in text.lines() {
                out.push(color_json_line(p, t, l, width));
            }
        } else {
            // not JSON: show the full line wrapped
            let text = String::from_utf8_lossy(b).into_owned();
            let chars: Vec<char> = text.chars().collect();
            if chars.is_empty() {
                out.push(String::new());
            }
            for chunk in chars.chunks(width.max(10)) {
                out.push(format!("{}{}{}", p.fg(t.fg), chunk.iter().collect::<String>(), p.reset()));
            }
        }
        out
    }

    fn status(&self, w: usize) -> String {
        let t = &self.p.t;
        let p = &self.p;
        let n = self.nrows();
        let total_lines = self.idx.count();
        let mut parts: Vec<String> = Vec::new();
        parts.push(format!("{}", self.path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()));
        let pos = if n == 0 { "empty".to_string() } else { format!("{}/{}", self.line_of_row(self.sel).map(|l| l + 1).unwrap_or(0), total_lines) };
        parts.push(pos);
        if !self.idx.is_done() {
            parts.push(format!("indexing {:.0}%", self.idx.progress(self.src.len()) * 100.0));
        }
        if let Some(f) = &self.filter {
            parts.push(format!("filter /{}/ {} lines{}", f.pattern, n, if f.done.load(Ordering::Acquire) { "" } else { " …" }));
        }
        if let Some(s) = &self.search {
            let c = s.count.load(Ordering::Relaxed);
            parts.push(format!("/{}/ {}{} match{}", s.pattern, c, if s.count_done.load(Ordering::Acquire) { "" } else { "+" }, if c == 1 { "" } else { "es" }));
        }
        if self.follow {
            parts.push("FOLLOW".into());
        }
        if let Some(nt) = &self.note {
            parts.push(nt.clone());
        }
        if !self.msg.is_empty() {
            parts.push(self.msg.clone());
        }
        let left = format!(" {} ", parts.join(" │ "));
        let hint = " / search · & filter · e/w levels · F follow · ⏎ json · ? help ";
        let pad = w.saturating_sub(left.chars().count() + hint.chars().count());
        let line = if pad > 0 { format!("{left}{}{hint}", " ".repeat(pad)) } else { fit(&left, w) };
        format!("{}{}{}{}", p.bg(t.bar_bg), p.fg(t.bar_fg), line, p.reset())
    }

    fn draw(&mut self) -> Result<()> {
        let (w, h) = tui::size();
        let t = self.p.t;
        let p = self.p;
        let mut lines: Vec<String> = Vec::with_capacity(h);

        if matches!(self.mode, Mode::Help) {
            let help = [
                "  kempt view — keys",
                "",
                "  j k ↑ ↓          move             space/b PgDn/PgUp   page",
                "  g / G            top / bottom     :  or  Ctrl-G       go to line number",
                "  ← → h l          scroll sideways  0                   reset horizontal scroll",
                "  /                search (regex, smart-case);   n / N  next / previous match",
                "  &                filter lines by regex (empty input clears)",
                "  e / w            quick filter: errors only / warnings+errors (press again to clear)",
                "  t                jump to a timestamp, e.g.  2026-11-03 10:15",
                "  Enter            toggle detail pane (pretty-prints JSON log lines);  J / K scroll it",
                "  F                follow the file as it grows (tail -f), survives rotation",
                "  q / Esc          quit",
                "",
                "  any key closes this help",
            ];
            for i in 0..h.saturating_sub(1) {
                lines.push(format!("{}{}", p.fg(t.fg), fit(help.get(i).copied().unwrap_or(""), w)));
            }
            lines.push(self.status(w));
            return tui::draw(&lines);
        }

        let detail_lines = if self.detail { self.detail_lines(w) } else { vec![] };
        let dh = if self.detail { (detail_lines.len() + 1).clamp(3, (h / 2).max(3)) } else { 0 };
        let bh = h.saturating_sub(1 + dh).max(1);
        self.clamp(bh);
        let total_lines = self.idx.count().max(1);
        let num_w = total_lines.to_string().len().max(3);
        let text_w = w.saturating_sub(num_w + 1);
        for i in 0..bh {
            lines.push(self.render_row(self.top + i, num_w, text_w));
        }
        if self.detail {
            let sep = format!("{}{}{}{}", p.bg(t.bar_bg), p.fg(t.bar_fg), fit(" detail — selected line (J/K scroll, ⏎ close) ", w), p.reset());
            lines.push(sep);
            let avail = dh.saturating_sub(1);
            let maxoff = detail_lines.len().saturating_sub(avail);
            let off = self.detail_off.min(maxoff);
            for i in 0..avail {
                lines.push(detail_lines.get(off + i).cloned().unwrap_or_default());
            }
        }
        match &self.mode {
            Mode::Prompt(_, pr) => {
                lines.push(format!("{}{}{}{}", p.bg(t.bar_bg), p.fg(t.bar_fg), fit(&format!(" {}{}█", pr.label, pr.text), w), p.reset()));
            }
            _ => lines.push(self.status(w)),
        }
        tui::draw(&lines)
    }
}

/// very small JSON syntax colouriser for pretty-printed lines
fn color_json_line(p: &Painter, t: &crate::theme::Theme, line: &str, width: usize) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut shown = 0;
    while i < chars.len() && shown < width {
        let c = chars[i];
        if c == '"' {
            let start = i;
            i += 1;
            while i < chars.len() {
                if chars[i] == '\\' {
                    i += 2;
                    continue;
                }
                if chars[i] == '"' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            let i2 = i.min(chars.len());
            let mut j = i2;
            while j < chars.len() && chars[j] == ' ' {
                j += 1;
            }
            let is_key = j < chars.len() && chars[j] == ':';
            let col = if is_key { t.ts_fg } else { t.lvl_info };
            let s: String = chars[start..i2].iter().collect();
            shown += s.chars().count();
            out.push_str(&format!("{}{}{}", p.fg(col), s, p.reset()));
        } else if c.is_ascii_digit() || c == '-' || c.is_ascii_alphabetic() {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || matches!(chars[i], '-' | '.' | '+')) {
                i += 1;
            }
            let s: String = chars[start..i].iter().collect();
            shown += s.chars().count();
            out.push_str(&format!("{}{}{}", p.fg(t.lvl_warn), s, p.reset()));
        } else {
            out.push(c);
            shown += 1;
            i += 1;
        }
    }
    out
}

fn run_tui(mut v: V, first_line: Option<usize>) -> Result<()> {
    let _term = Term::enter()?;
    if let Some(l) = first_line {
        v.goto_line(l);
    }
    let mut dirty = true;
    let mut was_busy = false;
    loop {
        // keep sel valid for follow (usize::MAX means "jump to end")
        if v.sel == usize::MAX {
            v.sel = v.nrows().saturating_sub(1);
        }
        if v.refresh() {
            dirty = true;
            if v.sel == usize::MAX {
                v.sel = v.nrows().saturating_sub(1);
            }
        }
        // background work (indexing / filtering / match counting) updates counters: keep redrawing
        // while it runs, plus once more when it finishes so the final numbers are shown
        let busy = !v.idx.is_done()
            || v.filter.as_ref().is_some_and(|f| !f.done.load(Ordering::Acquire))
            || v.search.as_ref().is_some_and(|s| !s.count_done.load(Ordering::Acquire));
        if busy || was_busy {
            dirty = true;
        }
        was_busy = busy;
        if dirty {
            v.draw()?;
            dirty = false;
        }
        let Some(ev) = tui::poll(Duration::from_millis(120))? else { continue };
        dirty = true;
        let (_, h) = tui::size();
        let page = (h.saturating_sub(3)).max(1) as isize;
        match ev {
            Event::Resize(..) => {}
            Event::Mouse(m) => match m.kind {
                MouseEventKind::ScrollDown => {
                    v.follow = false;
                    v.move_sel(3)
                }
                MouseEventKind::ScrollUp => {
                    v.follow = false;
                    v.move_sel(-3)
                }
                _ => {}
            },
            Event::Key(k) if k.kind != crossterm::event::KeyEventKind::Release => {
                match &mut v.mode {
                    Mode::Help => {
                        v.mode = Mode::Normal;
                        continue;
                    }
                    Mode::Prompt(kind, pr) => {
                        let kind = *kind;
                        match pr.key(&k) {
                            PromptResult::Pending => {}
                            PromptResult::Cancel => v.mode = Mode::Normal,
                            PromptResult::Submit(text) => {
                                v.mode = Mode::Normal;
                                match kind {
                                    PromptKind::Search => {
                                        if !text.is_empty() && v.set_search(&text) {
                                            v.find(true);
                                        }
                                    }
                                    PromptKind::Filter => v.set_filter(&text),
                                    PromptKind::Goto => match text.trim().parse::<usize>() {
                                        Ok(n) => v.goto_line(n),
                                        Err(_) => v.msg = "not a line number".into(),
                                    },
                                    PromptKind::Time => v.goto_time(&text),
                                }
                            }
                        }
                        continue;
                    }
                    Mode::Normal => {}
                }
                v.msg.clear();
                match k.code {
                    KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => break,
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Char('j') | KeyCode::Down => {
                        v.follow = false;
                        v.move_sel(1)
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        v.follow = false;
                        v.move_sel(-1)
                    }
                    KeyCode::Char(' ') | KeyCode::PageDown | KeyCode::Char('f') => {
                        v.follow = false;
                        v.move_sel(page)
                    }
                    KeyCode::Char('b') | KeyCode::PageUp => {
                        v.follow = false;
                        v.move_sel(-page)
                    }
                    KeyCode::Char('g') | KeyCode::Home => {
                        v.follow = false;
                        v.sel = 0
                    }
                    KeyCode::Char('G') | KeyCode::End => v.sel = v.nrows().saturating_sub(1),
                    KeyCode::Right | KeyCode::Char('l') => v.hoff += 8,
                    KeyCode::Left | KeyCode::Char('h') => v.hoff = v.hoff.saturating_sub(8),
                    KeyCode::Char('0') => v.hoff = 0,
                    KeyCode::Char('/') => v.mode = Mode::Prompt(PromptKind::Search, Prompt::new("/", "")),
                    KeyCode::Char('?') => v.mode = Mode::Help,
                    KeyCode::Char('&') => {
                        let cur = v.filter.as_ref().map(|f| f.pattern.clone()).unwrap_or_default();
                        v.mode = Mode::Prompt(PromptKind::Filter, Prompt::new("filter regex (empty clears): ", &cur));
                    }
                    KeyCode::Char(':') => v.mode = Mode::Prompt(PromptKind::Goto, Prompt::new("go to line: ", "")),
                    KeyCode::Char('t') => v.mode = Mode::Prompt(PromptKind::Time, Prompt::new("go to time (e.g. 2026-11-03 10:15): ", "")),
                    KeyCode::Char('n') => v.find(true),
                    KeyCode::Char('N') => v.find(false),
                    KeyCode::Char('e') => {
                        if v.filter.as_ref().is_some_and(|f| f.pattern == ERROR_RE) {
                            v.set_filter("")
                        } else {
                            v.set_filter(ERROR_RE)
                        }
                    }
                    KeyCode::Char('w') => {
                        if v.filter.as_ref().is_some_and(|f| f.pattern == WARN_RE) {
                            v.set_filter("")
                        } else {
                            v.set_filter(WARN_RE)
                        }
                    }
                    KeyCode::Char('F') => {
                        if v.can_follow {
                            v.follow = !v.follow;
                            if v.follow {
                                v.sel = usize::MAX;
                            }
                        } else {
                            v.msg = "follow is unavailable for formatted/compressed views (use --raw)".into();
                        }
                    }
                    KeyCode::Enter => {
                        v.detail = !v.detail;
                        v.detail_off = 0;
                    }
                    KeyCode::Char('J') => v.detail_off += 3,
                    KeyCode::Char('K') => v.detail_off = v.detail_off.saturating_sub(3),
                    _ => {}
                }
                if matches!(k.code, KeyCode::Char('?')) && matches!(v.mode, Mode::Prompt(..)) {
                    v.mode = Mode::Help;
                }
                if !matches!(k.code, KeyCode::Char('J') | KeyCode::Char('K')) && v.detail {
                    v.detail_off = 0;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Non-interactive: write the (formatted) file to stdout, optionally filtered. Used when stdout is a pipe.
fn dump(src: &Source, filter: Option<&Regex>, out: &mut dyn Write) -> Result<()> {
    let data = src.bytes();
    match filter {
        None => out.write_all(data)?,
        Some(re) => {
            let mut pos = 0;
            while pos < data.len() {
                let end = memchr::memchr(b'\n', &data[pos..]).map(|p| pos + p + 1).unwrap_or(data.len());
                let line = &data[pos..end];
                let core = line.strip_suffix(b"\n").unwrap_or(line);
                if re.is_match(core) {
                    out.write_all(line)?;
                    if !line.ends_with(b"\n") {
                        out.write_all(b"\n")?;
                    }
                }
                pos = end;
            }
        }
    }
    Ok(())
}

pub fn run(a: ViewArgs) -> Result<()> {
    let is_gz = a.path.extension().is_some_and(|e| e == "gz");
    // follow needs the live file, so it implies raw
    let want_format = !a.raw && !a.follow;
    let (src, note) = if want_format { Source::open_formatted(&a.path, a.indent)? } else { (Source::open(&a.path)?, FormatNote::NotApplicable) };
    if looks_binary(src.bytes()) && !a.force {
        bail!("{} looks like a binary file (use --force to view anyway)", a.path.display());
    }
    let note_txt = match note {
        FormatNote::Formatted => Some("formatted".to_string()),
        FormatNote::Failed(e) => Some(format!("not formatted: {e}")),
        FormatNote::NotApplicable => None,
    };

    // piped output: just emit
    if a.plain || !(std::io::IsTerminal::is_terminal(&std::io::stdout()) && std::io::IsTerminal::is_terminal(&std::io::stdin())) {
        let re = match &a.filter {
            Some(f) => Some(smart_regex(f)?),
            None => None,
        };
        if let Some(n) = &note_txt {
            if n.starts_with("not formatted") {
                eprintln!("note: {n}");
            }
        }
        let stdout = std::io::stdout();
        let mut w = std::io::BufWriter::with_capacity(1 << 20, stdout.lock());
        dump(&src, re.as_ref(), &mut w)?;
        w.flush()?;
        return Ok(());
    }

    let idx = LineIndex::new();
    match src.map.clone() {
        Some(m) => {
            idx.spawn(m);
        }
        None => idx.sh.done.store(true, Ordering::Release),
    }
    let can_follow = !is_gz && !want_format;
    let mut v = V {
        path: a.path.clone(),
        src,
        idx,
        p: a.painter,
        note: note_txt,
        top: 0,
        sel: 0,
        hoff: 0,
        filter: None,
        search: None,
        follow: a.follow,
        can_follow,
        detail: false,
        detail_off: 0,
        mode: Mode::Normal,
        msg: String::new(),
        ino: inode(&a.path),
        ts_re: ts_regex(),
        lvl_re: level_regex(),
        indent: a.indent,
    };
    if let Some(f) = &a.filter {
        v.set_filter(f);
    }
    if a.follow {
        v.sel = usize::MAX;
    }
    run_tui(v, a.line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{ColorMode, ThemeKind};

    fn mk(data: &[u8]) -> (V, tempfile::NamedTempFile) {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(data).unwrap();
        f.flush().unwrap();
        let src = Source::open(f.path()).unwrap();
        let idx = LineIndex::new();
        if let Some(m) = src.map.clone() {
            idx.spawn(m).join().unwrap();
        } else {
            idx.sh.done.store(true, Ordering::Release);
        }
        let v = V {
            path: f.path().to_path_buf(),
            src,
            idx,
            p: Painter::new(ColorMode::None, ThemeKind::Dark),
            note: None,
            top: 0,
            sel: 0,
            hoff: 0,
            filter: None,
            search: None,
            follow: false,
            can_follow: true,
            detail: false,
            detail_off: 0,
            mode: Mode::Normal,
            msg: String::new(),
            ino: inode(f.path()),
            ts_re: ts_regex(),
            lvl_re: level_regex(),
            indent: 2,
        };
        (v, f)
    }

    #[test]
    fn search_finds_and_wraps() {
        let (mut v, _f) = mk(b"alpha\nbeta\ngamma\nbeta again\nomega\n");
        assert!(v.set_search("beta"));
        v.find(true);
        assert_eq!(v.sel, 1);
        v.find(true);
        assert_eq!(v.sel, 3);
        v.find(true);
        assert_eq!(v.sel, 1);
        assert_eq!(v.msg, "search wrapped");
        v.find(false);
        assert_eq!(v.sel, 3);
    }

    #[test]
    fn smart_case() {
        let (mut v, _f) = mk(b"Hello\nhello\n");
        v.set_search("hello"); // lowercase => case-insensitive
        v.sel = 1;
        v.find(true);
        assert_eq!(v.sel, 0);
        v.set_search("Hello"); // has uppercase => case-sensitive
        v.sel = 1;
        v.find(true);
        assert_eq!(v.sel, 0);
        v.set_search("HELLO");
        v.find(true);
        assert_eq!(v.msg, "pattern not found");
    }

    #[test]
    fn filter_collects_matching_lines() {
        let (mut v, _f) = mk(b"INFO a\nERROR b\nINFO c\nERROR d\n");
        v.set_filter(ERROR_RE);
        for _ in 0..200 {
            if v.filter.as_ref().unwrap().done.load(Ordering::Acquire) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(v.nrows(), 2);
        assert_eq!(v.line_of_row(0), Some(1));
        assert_eq!(v.line_of_row(1), Some(3));
        assert_eq!(v.line_bytes(1).unwrap(), b"ERROR b");
    }

    #[test]
    fn goto_time_binary_search() {
        let mut s = String::new();
        for m in 0..60 {
            s.push_str(&format!("2026-11-03 10:{:02}:00 INFO tick {m}\n", m));
            s.push_str("  continuation line without timestamp\n");
        }
        let (mut v, _f) = mk(s.as_bytes());
        v.goto_time("2026-11-03 10:30");
        assert_eq!(v.line_bytes(v.sel).unwrap(), b"2026-11-03 10:30:00 INFO tick 30");
        v.goto_time("2026-11-03 09:00");
        assert_eq!(v.sel, 0);
        v.goto_time("2026-11-04");
        assert!(v.msg.contains("no entries"));
    }

    #[test]
    fn follow_extends_index_and_detects_truncate() {
        let (mut v, mut f) = mk(b"one\ntwo\n");
        assert_eq!(v.idx.count(), 2);
        f.write_all(b"three\nfour").unwrap();
        f.flush().unwrap();
        assert!(v.refresh());
        assert_eq!(v.idx.count(), 4);
        assert_eq!(v.line_bytes(3).unwrap(), b"four");
        f.write_all(b"-more\nfive\n").unwrap();
        f.flush().unwrap();
        assert!(v.refresh());
        assert_eq!(v.idx.count(), 5);
        assert_eq!(v.line_bytes(3).unwrap(), b"four-more");
        // truncate (Windows refuses to shrink a file that is memory-mapped, so the other program could not do it either)
        #[cfg(not(windows))]
        {
            f.as_file().set_len(4).unwrap();
            assert!(v.refresh());
            assert!(v.msg.contains("reloaded"));
        }
    }

    #[test]
    fn styled_line_has_expected_text_and_clipping() {
        let (v, _f) = mk(b"x\n");
        let s = v.style_line(b"2026-11-03 10:00:00 ERROR something bad", 0, 100, None);
        assert_eq!(s, "2026-11-03 10:00:00 ERROR something bad");
        let s = v.style_line(b"abcdef\tg", 2, 5, None);
        assert_eq!(s, "cdef ");
    }
}
