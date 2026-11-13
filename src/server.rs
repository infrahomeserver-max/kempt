//! `kempt gui`: a local web UI. A tiny HTTP server on 127.0.0.1 (random token, Host check)
//! exposes a JSON API over memory-mapped documents; the page only ever asks for the rows it shows.

use crate::diff::{self, Kind, Lines, Op, Opts, Stats};
use crate::docs::{Copy, Doc, DocInfo};
use crate::render::{Row, RowKind, View};
use crate::search::{self, Job, Matcher, StepResult, utf16_offset};
use anyhow::Result;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tiny_http::{Header, Method, Response, Server};

const INDEX_HTML: &str = include_str!("../ui/index.html");
const FAVICON_SVG: &str = include_str!("../packaging/kempt.svg");
const FAVICON_PNG: &[u8] = include_bytes!("../packaging/kempt.png");
const MAX_LINE_BYTES: usize = 8000;

// ───────────────────────────── errors / helpers ─────────────────────────────

pub struct ApiError {
    status: u16,
    msg: String,
}
type ApiResult = std::result::Result<Value, ApiError>;

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError { status: 400, msg: format!("{e:#}") }
    }
}
fn bad<T>(msg: impl Into<String>) -> std::result::Result<T, ApiError> {
    Err(ApiError { status: 400, msg: msg.into() })
}

fn gu(p: &Value, k: &str) -> std::result::Result<u64, ApiError> {
    p.get(k).and_then(|v| v.as_u64()).ok_or(ApiError { status: 400, msg: format!("missing or invalid '{k}'") })
}
fn gs<'a>(p: &'a Value, k: &str) -> Option<&'a str> {
    p.get(k).and_then(|v| v.as_str())
}
fn gb(p: &Value, k: &str) -> Option<bool> {
    p.get(k).and_then(|v| v.as_bool())
}

/// a big JSON/XML document squeezed into a handful of lines
fn looks_minified(i: &DocInfo) -> bool {
    matches!(i.kind, "json" | "xml") && i.size > 2000 && i.lines <= 5
}

/// indent for Format / Fix: a number of spaces (1–8) or "tab"
fn indent_of(p: &Value) -> usize {
    match p.get("indent") {
        Some(Value::String(s)) if s == "tab" => crate::jsonfmt::INDENT_TAB,
        Some(v) => v.as_u64().map(|n| n.clamp(1, 8) as usize).unwrap_or(2),
        None => 2,
    }
}

fn info_json(i: &DocInfo) -> Value {
    json!({"id": i.id, "name": i.name, "path": i.path, "lines": i.lines, "size": i.size, "dirty": i.dirty,
           "can_undo": i.can_undo, "can_redo": i.can_redo, "version": i.version, "kind": i.kind})
}

/// lossy text of a line, clipped to a sane length. Returns (text, original byte length if clipped)
fn clip(b: &[u8]) -> (String, Option<usize>) {
    if b.len() > MAX_LINE_BYTES {
        // cut on a character boundary so an emoji (4 bytes) is never split into a replacement character
        let mut n = MAX_LINE_BYTES;
        while n > 0 && (b[n] & 0xC0) == 0x80 {
            n -= 1;
        }
        (String::from_utf8_lossy(&b[..n]).into_owned(), Some(b.len()))
    } else {
        (String::from_utf8_lossy(b).into_owned(), None)
    }
}

fn ranges_json(m: Option<&Matcher>, text: &str) -> Value {
    let Some(m) = m else { return Value::Null };
    let rs = m.ranges(text.as_bytes());
    if rs.is_empty() {
        return Value::Null;
    }
    let ascii = text.is_ascii();
    Value::Array(
        rs.iter()
            .map(|r| {
                let (s, e) = if ascii { (r.start, r.end) } else { (utf16_offset(text, r.start), utf16_offset(text, r.end)) };
                json!([s, e, r.term])
            })
            .collect(),
    )
}

fn matcher_from(p: &Value) -> std::result::Result<Option<Matcher>, ApiError> {
    match p.get("spec") {
        None | Some(Value::Null) => Ok(None),
        Some(s) => {
            let m = Matcher::parse(s)?;
            Ok(if m.is_empty() { None } else { Some(m) })
        }
    }
}

// ───────────────────────────── diff session ─────────────────────────────

struct DiffSession {
    left: Arc<Doc>,
    right: Arc<Doc>,
    lver: u64,
    rver: u64,
    ops: Vec<Op>,
    view: View,
    changes: Vec<usize>,
    stats: Stats,
    identical: bool,
    eol_only: bool,
    timed_out: bool,
    ctx: Option<usize>,
    ws: bool,
    case: bool,
    mask: bool,
    undo: Vec<Arc<Doc>>,
}

impl DiffSession {
    fn new(left: Arc<Doc>, right: Arc<Doc>) -> DiffSession {
        let mut s = DiffSession {
            left,
            right,
            lver: 0,
            rver: 0,
            ops: vec![],
            view: View::build(&[], None),
            changes: vec![],
            stats: Stats::default(),
            identical: true,
            eol_only: false,
            timed_out: false,
            ctx: None,
            ws: false,
            case: false,
            mask: false,
            undo: vec![],
        };
        s.recompute();
        s
    }

    fn recompute(&mut self) {
        let lc = self.left.current();
        let rc = self.right.current();
        self.lver = self.left.version();
        self.rver = self.right.version();
        let a = Lines::with_starts(lc.bytes(), &lc.starts);
        let b = Lines::with_starts(rc.bytes(), &rc.starts);
        let o = Opts { ignore_case: self.case, ignore_ws: self.ws, mask: self.mask, timeout: Some(Duration::from_secs(60)) };
        let res = diff::diff_lines(&a, &b, &o);
        self.stats = diff::stats(&res.ops);
        self.identical = diff::is_identical(&res.ops);
        self.eol_only = self.identical && !o.normalising() && lc.bytes() != rc.bytes();
        self.timed_out = res.timed_out;
        self.ops = res.ops;
        self.view = View::build(&self.ops, self.ctx);
        self.changes = self.view.change_starts();
    }

    fn sync(&mut self) {
        if self.left.version() != self.lver || self.right.version() != self.rver {
            self.recompute();
        }
    }

    fn state(&self) -> Value {
        json!({
            "total": self.view.total(),
            "stats": {"added": self.stats.added, "removed": self.stats.removed, "changed": self.stats.changed, "hunks": self.stats.hunks},
            "changes": self.changes.len(),
            "identical": self.identical, "eol_only": self.eol_only, "timed_out": self.timed_out,
            "hide_unchanged": self.ctx.is_some(), "ws": self.ws, "case": self.case, "mask": self.mask,
            "left": info_json(&self.left.info()), "right": info_json(&self.right.info()),
            "can_undo": !self.undo.is_empty(),
        })
    }

    fn row_json(&self, r: usize, lc: &Copy, rc: &Copy, m: Option<&Matcher>) -> Value {
        let row = self.view.row(r);
        if row.kind == RowKind::Fold {
            return json!({"i": r, "k": "fold", "f": row.fold});
        }
        let op = &self.ops[row.op];
        let k = match row.kind {
            RowKind::Equal => "eq",
            RowKind::Del => "del",
            RowKind::Ins => "ins",
            _ => "chg",
        };
        let (lt, lx) = row.left.map(|i| clip(lc.line(i))).unzip();
        let (rt, rx) = row.right.map(|i| clip(rc.line(i))).unzip();
        let mut v = json!({"i": r, "k": k, "l": row.left.map(|x| x + 1), "r": row.right.map(|x| x + 1),
                           "lt": lt, "rt": rt, "o": row.off, "op": row.op,
                           "h": if op.kind == Kind::Equal { 0 } else { op.old_len.max(op.new_len) }});
        if let Some(Some(n)) = lx {
            v["lx"] = json!(n);
        }
        if let Some(Some(n)) = rx {
            v["rx"] = json!(n);
        }
        if row.kind == RowKind::Chg {
            let (a, b) = diff::inline_diff(lt.as_deref().unwrap().as_bytes(), rt.as_deref().unwrap().as_bytes());
            v["ls"] = json!(a);
            v["rs"] = json!(b);
        }
        if let Some(m) = m {
            if let Some(t) = &lt {
                v["lm"] = ranges_json(Some(m), t);
            }
            if let Some(t) = &rt {
                v["rm"] = ranges_json(Some(m), t);
            }
        }
        v
    }

    /// Apply one row's change: copy left→right (`to_right`) or right→left.
    fn apply_row(&mut self, r: usize, to_right: bool) -> std::result::Result<(), ApiError> {
        if r >= self.view.total() {
            return bad("row out of range");
        }
        let row: Row = self.view.row(r);
        if matches!(row.kind, RowKind::Equal | RowKind::Fold) {
            return bad("nothing to copy on an unchanged row");
        }
        let op = self.ops[row.op];
        let lc = self.left.current();
        let rc = self.right.current();
        let (target, src, src_idx, tgt_idx): (Arc<Doc>, &Copy, Option<usize>, Option<usize>);
        let (tc, sc): (&Copy, &Copy) = if to_right { (&rc, &lc) } else { (&lc, &rc) };
        if to_right {
            target = self.right.clone();
            src = &lc;
            src_idx = row.left;
            tgt_idx = row.right;
        } else {
            target = self.left.clone();
            src = &rc;
            src_idx = row.right;
            tgt_idx = row.left;
        }
        let _ = sc;
        match (src_idx, tgt_idx) {
            // changed line: overwrite the target line with the source line
            (Some(si), Some(ti)) => {
                let mut ins = src.line(si).to_vec();
                let e = tc.eol(ti);
                ins.extend_from_slice(if e.is_empty() { b"\n" } else { e });
                target.splice(ti, 1, &ins)?;
            }
            // line only on the source side: insert it into the target
            (Some(si), None) => {
                let (base, base_len) = if to_right { (op.new, op.new_len) } else { (op.old, op.old_len) };
                let pos = base + row.off.min(base_len);
                let mut ins = src.line(si).to_vec();
                ins.extend_from_slice(tc.default_eol());
                target.splice(pos, 0, &ins)?;
            }
            // line only on the target side: delete it (adopt the source's absence)
            (None, Some(ti)) => {
                target.splice(ti, 1, b"")?;
            }
            (None, None) => return bad("empty row"),
        }
        self.undo.push(target);
        self.recompute();
        Ok(())
    }

    /// Copy a whole hunk (diff op).
    fn apply_hunk(&mut self, opi: usize, to_right: bool) -> std::result::Result<(), ApiError> {
        let Some(&op) = self.ops.get(opi) else { return bad("hunk out of range") };
        if op.kind == Kind::Equal {
            return bad("nothing to copy: this block is unchanged");
        }
        let lc = self.left.current();
        let rc = self.right.current();
        let (target, src, src_start, src_len, tgt_start, tgt_len, tgt_eol) = if to_right {
            (self.right.clone(), &lc, op.old, op.old_len, op.new, op.new_len, rc.default_eol())
        } else {
            (self.left.clone(), &rc, op.new, op.new_len, op.old, op.old_len, lc.default_eol())
        };
        let mut ins = Vec::new();
        for k in 0..src_len {
            ins.extend_from_slice(src.line(src_start + k));
            ins.extend_from_slice(tgt_eol);
        }
        target.splice(tgt_start, tgt_len, &ins)?;
        self.undo.push(target);
        self.recompute();
        Ok(())
    }

    /// Make one side identical to the other.
    fn apply_all(&mut self, to_right: bool) -> std::result::Result<(), ApiError> {
        let (target, src) = if to_right { (self.right.clone(), self.left.current()) } else { (self.left.clone(), self.right.current()) };
        target.transform(|_, w| {
            w.write_all(src.bytes())?;
            Ok(Some(()))
        })?;
        self.undo.push(target);
        self.recompute();
        Ok(())
    }
}

// ───────────────────────────── app state ─────────────────────────────

pub struct App {
    token: String,
    port: u16,
    next: AtomicU64,
    docs: Mutex<HashMap<u64, Arc<Doc>>>,
    jobs: Mutex<HashMap<u64, Arc<Job>>>,
    doc_jobs: Mutex<HashMap<String, u64>>,
    diffs: Mutex<HashMap<u64, Arc<Mutex<DiffSession>>>>,
    startup: Mutex<Vec<Value>>,
    tables: Mutex<HashMap<u64, Arc<Mutex<crate::table::Table>>>>,
    store: crate::state::Store,
    notes: crate::state::Store,
    calendar: crate::state::Store,
    wake: (Mutex<bool>, std::sync::Condvar),
    own_window: bool,
}

fn expand(p: &str) -> PathBuf {
    if p == "~" || p.starts_with("~/") || p.starts_with("~\\") {
        return crate::plat::home_dir().join(p.trim_start_matches('~').trim_start_matches(['/', '\\']));
    }
    PathBuf::from(p)
}

impl App {
    fn id(&self) -> u64 {
        self.next.fetch_add(1, Ordering::Relaxed)
    }

    fn doc(&self, p: &Value) -> std::result::Result<Arc<Doc>, ApiError> {
        let id = gu(p, "id")?;
        self.docs.lock().unwrap().get(&id).cloned().ok_or(ApiError { status: 404, msg: "document is not open (closed or server restarted)".into() })
    }

    /// the table view of a document (rebuilt if the document changed since it was made)
    fn table(&self, p: &Value) -> std::result::Result<(Arc<Doc>, Arc<Mutex<crate::table::Table>>), ApiError> {
        let d = self.doc(p)?;
        let t = self.tables.lock().unwrap().get(&d.id).cloned().ok_or(ApiError { status: 404, msg: "table view is not open".into() })?;
        {
            let mut g = t.lock().unwrap();
            if g.version != d.version() {
                let c = d.current();
                *g = crate::table::Table::new(&c, d.version(), g.delim, g.header);
            }
        }
        Ok((d, t))
    }

    fn sess(&self, p: &Value) -> std::result::Result<Arc<Mutex<DiffSession>>, ApiError> {
        let id = gu(p, "sid")?;
        self.diffs.lock().unwrap().get(&id).cloned().ok_or(ApiError { status: 404, msg: "comparison is not open".into() })
    }

    fn open_path(&self, path: &str) -> std::result::Result<Arc<Doc>, ApiError> {
        self.open_path_as(path, "auto")
    }

    fn open_path_as(&self, path: &str, mode: &str) -> std::result::Result<Arc<Doc>, ApiError> {
        let pb = expand(path.trim());
        let doc = Arc::new(Doc::open_as(self.id(), &pb, mode)?);
        self.docs.lock().unwrap().insert(doc.id, doc.clone());
        Ok(doc)
    }

    /// an Excel workbook: every worksheet becomes its own table document (read-only source, Save As writes CSV)
    fn open_book(&self, path: &str) -> ApiResult {
        let pb = expand(path.trim());
        let mut book = crate::xlsx::Book::open(&pb)?;
        let file = pb.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let total = book.names.len();
        let mut infos = Vec::new();
        for i in 0..total.min(50) {
            let sheet = book.sheet(i)?;
            if sheet.csv.is_empty() && !(i + 1 == total && infos.is_empty()) {
                continue;
            }
            let f = crate::xlsx::sheet_to_file(&sheet)?;
            let doc = Arc::new(Doc::from_generated(self.id(), &format!("{} ({})", sheet.name, file), &f, "table")?);
            self.docs.lock().unwrap().insert(doc.id, doc.clone());
            infos.push(info_json(&doc.info()));
        }
        let first = infos.remove(0);
        Ok(json!({"info": first, "also": infos, "minified": false, "book": {"file": file, "sheets": total, "shown": infos.len() + 1}}))
    }

    /// {id} → existing doc, {path} → open it, {disk: id} → the on-disk version of an open doc
    fn resolve_side(&self, v: &Value) -> std::result::Result<Arc<Doc>, ApiError> {
        if let Some(id) = v.get("id").and_then(|x| x.as_u64()) {
            return self.docs.lock().unwrap().get(&id).cloned().ok_or(ApiError { status: 404, msg: "document is not open".into() });
        }
        if let Some(path) = v.get("path").and_then(|x| x.as_str()) {
            return self.open_path(path);
        }
        if let Some(id) = v.get("disk").and_then(|x| x.as_u64()) {
            let d = self.docs.lock().unwrap().get(&id).cloned().ok_or(ApiError { status: 404, msg: "document is not open".into() })?;
            let Some(p) = d.path() else { return bad("this document has no file on disk yet") };
            return self.open_path(&p.display().to_string());
        }
        bad("a comparison side needs 'id', 'path' or 'disk'")
    }

    fn new_diff(&self, l: Arc<Doc>, r: Arc<Doc>) -> (u64, Value) {
        let sess = DiffSession::new(l, r);
        let st = sess.state();
        let sid = self.id();
        self.diffs.lock().unwrap().insert(sid, Arc::new(Mutex::new(sess)));
        (sid, st)
    }

    fn start_job(&self, key: String, copy: Arc<Copy>, m: Matcher) -> u64 {
        let job = Arc::new(Job::new(copy.count() as u64));
        let id = self.id();
        {
            let mut dj = self.doc_jobs.lock().unwrap();
            let mut jobs = self.jobs.lock().unwrap();
            if let Some(old) = dj.insert(key, id) {
                if let Some(j) = jobs.remove(&old) {
                    j.cancel.store(true, Ordering::Relaxed);
                }
            }
            jobs.insert(id, job.clone());
        }
        std::thread::spawn(move || search::run_scan(&job, copy.bytes(), &m));
        id
    }

    fn job(&self, p: &Value) -> std::result::Result<Arc<Job>, ApiError> {
        let id = gu(p, "job")?;
        self.jobs.lock().unwrap().get(&id).cloned().ok_or(ApiError { status: 404, msg: "search was superseded".into() })
    }
}

// ───────────────────────────── API ─────────────────────────────

fn report_json(r: &crate::jsonfix::Report) -> Value {
    Value::Object(r.iter().map(|(k, v)| (k.to_string(), json!(v))).collect())
}

fn table_meta(t: &crate::table::Table) -> Value {
    json!({"delim": (t.delim as char).to_string(), "header": t.header, "ncols": t.ncols, "names": t.names, "nrows": t.nrows, "shown": t.shown(),
           "sort": t.sort.first().map(|&(c, d, m)| json!({"col": c, "desc": d, "mode": m.name()})),
           "sorts": t.sort.iter().map(|&(c, d, m)| json!({"col": c, "desc": d, "mode": m.name()})).collect::<Vec<_>>(),
           "filters": t.filters.iter().map(|f| json!({"col": f.col, "op": f.op.name(), "v": f.v, "v2": f.v2, "ci": f.ci, "list": f.list})).collect::<Vec<_>>(),
           "dup": t.dup.as_ref().map(|d| json!({"cols": d.spec.cols, "col": d.spec.cols.first(), "ci": d.spec.ci, "rows": d.rows.len(), "groups": d.groups}))})
}

fn api(app: &Arc<App>, name: &str, p: &Value) -> ApiResult {
    crate::jsonfmt::set_expand(gb(p, "expand").unwrap_or(false));
    match name {
        "startup" => Ok(json!({"tabs": *app.startup.lock().unwrap()})),

        "fs" => {
            let raw = gs(p, "path").unwrap_or("");
            let home = crate::plat::home_dir().display().to_string();
            let mut dir = if raw.is_empty() { PathBuf::from(&home) } else { expand(raw) };
            if dir.is_file() {
                dir = dir.parent().map(|d| d.to_path_buf()).unwrap_or(dir);
            }
            let dir = crate::plat::canonical(&dir).map_err(|e| ApiError { status: 400, msg: format!("cannot open {}: {e}", dir.display()) })?;
            let mut entries: Vec<(String, bool, u64)> = Vec::new();
            for e in std::fs::read_dir(&dir).map_err(|e| ApiError { status: 400, msg: format!("cannot list {}: {e}", dir.display()) })?.flatten().take(20000) {
                let md = e.metadata().ok();
                let is_dir = e.path().is_dir();
                entries.push((e.file_name().to_string_lossy().into_owned(), is_dir, md.map(|m| m.len()).unwrap_or(0)));
            }
            entries.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.to_lowercase().cmp(&b.0.to_lowercase())));
            Ok(json!({"path": dir.display().to_string(), "parent": dir.parent().map(|p| p.display().to_string()), "home": home,
                      "entries": entries.iter().map(|(n, d, s)| json!({"n": n, "d": d, "s": s})).collect::<Vec<_>>()}))
        }

        "open" => {
            let path = gs(p, "path").ok_or(ApiError { status: 400, msg: "missing 'path'".into() })?;
            let lower = path.trim().to_ascii_lowercase();
            if [".xlsx", ".xlsm", ".xltx", ".xltm", ".xlsb"].iter().any(|e| lower.ends_with(e)) {
                return app.open_book(path);
            }
            if lower.ends_with(".xls") || lower.ends_with(".ods") || lower.ends_with(".numbers") {
                return bad("old .xls / .ods / .numbers spreadsheets cannot be read. Save the file as .xlsx or .csv in your spreadsheet program first.");
            }
            let d = app.open_path_as(path, if gb(p, "as_text").unwrap_or(false) { "text" } else { gs(p, "as").unwrap_or("auto") })?;
            let i = d.info();
            Ok(json!({"info": info_json(&i), "minified": looks_minified(&i)}))
        }

        // create an empty file with the chosen name (the extension decides how it is coloured and formatted) and open it
        "file/new" => {
            let dir = expand(gs(p, "dir").unwrap_or("").trim());
            let name = gs(p, "name").unwrap_or("").trim().to_string();
            if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', '\0']) || name.len() > 200 {
                return bad("type a file name such as notes.txt (no slashes)");
            }
            let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
            let starter: &str = match ext.as_str() {
                "json" => "{\n  \n}\n",
                "html" | "htm" => "<!doctype html>\n<html lang=\"en\">\n<head>\n  <meta charset=\"utf-8\">\n  <title></title>\n</head>\n<body>\n  \n</body>\n</html>\n",
                "xml" => "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<root>\n  \n</root>\n",
                "sh" => "#!/usr/bin/env bash\nset -euo pipefail\n\n",
                "py" => "#!/usr/bin/env python3\n\n",
                _ => "",
            };
            // memory only: nothing is written until the first Save (which asks where)
            if gb(p, "memory").unwrap_or(false) {
                let doc = Arc::new(Doc::untitled(app.id(), &name, starter.as_bytes())?);
                app.docs.lock().unwrap().insert(doc.id, doc.clone());
                return Ok(json!({"info": info_json(&doc.info()), "path": ""}));
            }
            if !dir.is_dir() {
                return bad(format!("the folder {} does not exist", dir.display()));
            }
            let target = dir.join(&name);
            let mut f = match std::fs::OpenOptions::new().write(true).create_new(true).open(&target) {
                Ok(f) => f,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return bad(format!("{name} already exists in that folder")),
                Err(e) => return bad(format!("cannot create {}: {e}", target.display())),
            };
            use std::io::Write as _;
            f.write_all(starter.as_bytes()).map_err(|e| ApiError { status: 500, msg: e.to_string() })?;
            drop(f);
            let d = app.open_path(&target.display().to_string())?;
            Ok(json!({"info": info_json(&d.info()), "path": target.display().to_string()}))
        }

        // replace the whole text (what the editor sends when you stop typing / save)
        "doc/set" => {
            let d = app.doc(p)?;
            let text = gs(p, "text").ok_or(ApiError { status: 400, msg: "missing 'text'".into() })?;
            let cur = d.current();
            let mut out: Vec<u8> = Vec::with_capacity(text.len() + 8);
            if cur.bytes().starts_with(&[0xEF, 0xBB, 0xBF]) {
                out.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
            }
            if cur.default_eol() == b"\r\n" && !text.contains('\r') {
                out.extend_from_slice(text.replace('\n', "\r\n").as_bytes());
            } else {
                out.extend_from_slice(text.as_bytes());
            }
            let changed = d.transform_ex(true, |_, w| { w.write_all(&out)?; Ok(Some(())) })?.is_some();
            Ok(json!({"changed": changed, "info": info_json(&d.info())}))
        }

        "info" => Ok(info_json(&app.doc(p)?.info())),

        // ---- settings that survive restarts (a small JSON file; see state.rs)
        "state/get" => Ok(app.store.get()),
        "notes/get" => {
            let mut v = app.notes.get();
            v["caps"] = json!({"os": crate::service::notify_available(), "sound": crate::service::sound_available()});
            Ok(v)
        }
        "notes/set" => {
            let mut patch = p.get("patch").cloned().unwrap_or(Value::Null);
            // `n` means "the background reminder thread already announced this"; only the server may set it, and only while the due time is unchanged
            let cur = app.notes.get();
            let before: HashMap<String, (Value, bool)> = cur["items"].as_array().map(|a| a.iter().filter_map(|i| Some((i["id"].as_str()?.to_string(), (i["due"].clone(), i["n"] == true)))).collect()).unwrap_or_default();
            if let Some(items) = patch.get_mut("items").and_then(Value::as_array_mut) {
                for it in items.iter_mut() {
                    if let Some(o) = it.as_object_mut() {
                        o.remove("n");
                        if let Some((due, true)) = o.get("id").and_then(Value::as_str).and_then(|id| before.get(id)) {
                            if *due == o.get("due").cloned().unwrap_or(Value::Null) {
                                o.insert("n".into(), json!(true));
                            }
                        }
                    }
                }
            }
            let r = app.notes.set(&patch).map_err(|e| ApiError { status: 500, msg: e });
            app.wake.1.notify_all();
            r
        }
        // ---- calendar events (see calendar.rs): checked and reduced to the known fields before they are stored
        "calendar/get" => Ok(app.calendar.get()),
        "calendar/set" => {
            let mut patch = Map::new();
            if let Some(ev) = p.get("patch").and_then(|x| x.get("events")) {
                let clean = crate::calendar::clean_events(ev).map_err(|msg| ApiError { status: 400, msg })?;
                patch.insert("events".into(), Value::Array(clean));
            }
            if let Some(cfg) = p.get("patch").and_then(|x| x.get("cfg")).and_then(Value::as_object) {
                let view = cfg.get("view").and_then(Value::as_str).filter(|v| matches!(*v, "day" | "week" | "month" | "year" | "schedule")).unwrap_or("month");
                let ws = cfg.get("ws").and_then(Value::as_u64).filter(|d| *d <= 1 || *d == 6).unwrap_or(1);
                patch.insert("cfg".into(), json!({"view": view, "ws": ws}));
            }
            let r = app.calendar.set(&Value::Object(patch)).map_err(|e| ApiError { status: 500, msg: e });
            app.wake.1.notify_all();
            r
        }
        "service/autostart" => {
            if let Some(on) = p.get("on").and_then(Value::as_bool) {
                crate::service::set_autostart(on).map_err(|e| ApiError { status: 500, msg: e.to_string() })?;
            }
            Ok(json!({"on": crate::service::autostart_on()}))
        }
        "update/check" => crate::service::update_check(p.get("repo").and_then(|v| v.as_str()).unwrap_or("")).map_err(|msg| ApiError { status: 502, msg }),
        "service/browsers" => Ok(json!({"installed": crate::service::installed_browsers()})),
        "service/quit" => {
            crate::service::clear_instance();
            std::thread::spawn(|| {
                std::thread::sleep(Duration::from_millis(150));
                std::process::exit(0);
            });
            Ok(json!({"ok": true}))
        }
        "state/set" => app.store.set(p.get("patch").unwrap_or(&Value::Null)).map_err(|e| ApiError { status: 500, msg: e }),

        // ---- table view
        "table/open" => {
            let d = app.doc(p)?;
            let c = d.current();
            let delim = crate::table::parse_delim(gs(p, "delim").unwrap_or("auto"), &c, &d.name());
            let header = gb(p, "header").unwrap_or(true);
            let t = crate::table::Table::new(&c, d.version(), delim, header);
            let out = table_meta(&t);
            app.tables.lock().unwrap().insert(d.id, Arc::new(Mutex::new(t)));
            Ok(out)
        }
        "table/rows" => {
            let (d, t) = app.table(p)?;
            let t = t.lock().unwrap();
            let c = d.current();
            let rows = t.rows(&c, gu(p, "from")? as usize, (gu(p, "count")? as usize).min(500), !gb(p, "full").unwrap_or(false));
            Ok(json!({"rows": rows, "shown": t.shown(), "version": d.version()}))
        }
        "table/sort" => {
            let (d, t) = app.table(p)?;
            let mut t = t.lock().unwrap();
            // {"keys": [{col, desc, mode}, ...]} (most important first), or one key as {col, desc, mode}; col null = no sort
            let key = |v: &Value| -> Option<(usize, bool, crate::tsort::Mode)> {
                let c = v.get("col").and_then(|x| x.as_u64())? as usize;
                (c < t.ncols).then(|| (c, gb(v, "desc").unwrap_or(false), crate::tsort::Mode::parse(gs(v, "mode").unwrap_or("auto"))))
            };
            let keys: Vec<_> = match p.get("keys").and_then(|k| k.as_array()) {
                Some(a) => a.iter().filter_map(&key).collect(),
                None => key(p).into_iter().collect(),
            };
            t.set_sort(&d.current(), keys);
            crate::plat::trim_memory();
            Ok(table_meta(&t))
        }
        "table/dups" => {
            let (d, t) = app.table(p)?;
            let mut t = t.lock().unwrap();
            if gb(p, "off").unwrap_or(false) {
                t.clear_dups(&d.current());
            } else {
                let mut cols: Vec<usize> = match p.get("cols").and_then(|v| v.as_array()) {
                    Some(a) => a.iter().filter_map(|v| v.as_u64()).map(|v| v as usize).collect(),
                    None => p.get("col").and_then(|v| v.as_u64()).map(|v| v as usize).into_iter().collect(),
                };
                cols.retain(|&c| c < t.ncols);
                cols.sort_unstable();
                cols.dedup();
                t.find_dups(&d.current(), crate::table::DupSpec { cols, ci: gb(p, "ci").unwrap_or(false) });
            }
            crate::plat::trim_memory();
            Ok(table_meta(&t))
        }
        "table/filter" => {
            let (d, t) = app.table(p)?;
            let mut t = t.lock().unwrap();
            let mut fs = Vec::new();
            for f in p.get("filters").and_then(|v| v.as_array()).map(|a| a.as_slice()).unwrap_or(&[]) {
                let col = gu(f, "col")? as usize;
                if col >= t.ncols {
                    return bad("no such column");
                }
                let Some(op) = crate::table::FOp::parse(gs(f, "op").unwrap_or("")) else { return bad("unknown filter operator") };
                let flt = crate::table::Filter {
                    col,
                    op,
                    v: gs(f, "v").unwrap_or("").to_string(),
                    v2: gs(f, "v2").unwrap_or("").to_string(),
                    ci: gb(f, "ci").unwrap_or(true),
                    list: f.get("list").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default(),
                };
                if let Some(e) = flt.check() {
                    return bad(&e);
                }
                fs.push(flt);
            }
            t.set_filters(&d.current(), fs);
            crate::plat::trim_memory();
            Ok(table_meta(&t))
        }
        "table/values" => {
            let (d, t) = app.table(p)?;
            let t = t.lock().unwrap();
            let col = gu(p, "col")? as usize;
            if col >= t.ncols {
                return bad("no such column");
            }
            let (v, truncated, total) = crate::table::distinct(&d.current(), &t, col, gu(p, "limit").unwrap_or(500).min(2000) as usize);
            Ok(json!({"values": v.iter().map(|(s, n)| json!({"v": s, "n": n})).collect::<Vec<_>>(), "truncated": truncated, "total": total}))
        }
        "table/stats" => {
            let (d, t) = app.table(p)?;
            let t = t.lock().unwrap();
            let col = gu(p, "col")? as usize;
            if col >= t.ncols {
                return bad("no such column");
            }
            let s = crate::table::col_stats(&d.current(), &t, col);
            crate::plat::trim_memory();
            Ok(json!({"count": s.count, "blank": s.blank, "unique": s.unique, "dup_rows": s.dup_rows,
                      "numeric": s.numeric.map(|(n, sum, mn, mx)| json!({"n": n, "sum": sum, "min": mn, "max": mx, "avg": sum / n as f64}))}))
        }

        // whole text for the rendered viewers (Markdown, HTML, JSON tree); refused above `max` bytes
        "doc/text" => {
            let d = app.doc(p)?;
            let c = d.current();
            let max = gu(p, "max").unwrap_or(16 << 20).min(64 << 20) as usize;
            if c.size() > max {
                return bad(&format!("this file is {} MB; the viewer is limited to {} MB. Use the text view.", c.size() >> 20, max >> 20));
            }
            let b = c.bytes();
            let b = b.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(b);
            Ok(json!({"text": String::from_utf8_lossy(b), "size": c.size()}))
        }
        "lines" => {
            let d = app.doc(p)?;
            let m = matcher_from(p)?;
            let c = d.current();
            let from = gu(p, "from")? as usize;
            let count = (gu(p, "count")? as usize).min(500);
            let end = (from + count).min(c.count());
            let mut out = Vec::with_capacity(end.saturating_sub(from));
            for i in from..end {
                let (t, x) = clip(c.line(i));
                let mut v = json!({"n": i, "t": t});
                if let Some(n) = x {
                    v["x"] = json!(n);
                }
                let r = ranges_json(m.as_ref(), v["t"].as_str().unwrap());
                if !r.is_null() {
                    v["m"] = r;
                }
                out.push(v);
            }
            Ok(json!({"lines": out, "total": c.count(), "version": d.version()}))
        }

        // ---- find
        "search/start" => {
            let d = app.doc(p)?;
            let m = Matcher::parse(p.get("spec").unwrap_or(&Value::Null))?;
            if m.is_empty() {
                return Ok(json!({"job": null}));
            }
            let c = d.current();
            let total = c.count();
            let job = app.start_job(format!("doc:{}", d.id), c, m);
            Ok(json!({"job": job, "total_lines": total}))
        }
        "search/status" => {
            let j = app.job(p)?;
            Ok(json!({"count": j.count(), "occurrences": j.occurrences.load(Ordering::Relaxed), "scanned": j.scanned_lines.load(Ordering::Relaxed),
                      "total": j.total_lines, "done": j.done.load(Ordering::Acquire), "truncated": j.truncated.load(Ordering::Relaxed)}))
        }
        "search/step" => {
            let j = app.job(p)?;
            let from = p.get("from").and_then(|v| v.as_i64()).unwrap_or(-1);
            let dir = p.get("dir").and_then(|v| v.as_i64()).unwrap_or(1) as i32;
            let t0 = Instant::now();
            loop {
                match j.step(from, dir) {
                    StepResult::Found { line, index, wrapped } => return Ok(json!({"line": line, "index": index, "wrapped": wrapped, "total": j.count()})),
                    StepResult::None => return Ok(json!({"none": true})),
                    StepResult::Pending => {
                        if t0.elapsed() > Duration::from_secs(3) {
                            return Ok(json!({"pending": true}));
                        }
                        std::thread::sleep(Duration::from_millis(15));
                    }
                }
            }
        }
        "search/ruler" => {
            let j = app.job(p)?;
            let b = (gu(p, "buckets")? as usize).clamp(1, 4096);
            Ok(json!({"counts": j.ruler(b)}))
        }
        "search/rank" => {
            let j = app.job(p)?;
            let line = p.get("line").and_then(|v| v.as_i64()).unwrap_or(0);
            Ok(json!({"rank": j.rank(line), "total": j.count()}))
        }

        // ---- editing
        "doc/replace" => {
            let d = app.doc(p)?;
            let spec = p.get("spec").unwrap_or(&Value::Null);
            let m = Matcher::parse(spec)?;
            if m.is_empty() {
                return bad("type something to find first");
            }
            let rep = gs(p, "replacement").unwrap_or("");
            let literal = !gb(spec, "regex").unwrap_or(false);
            let (lines, occ) = if gs(p, "scope") == Some("line") {
                let line = gu(p, "line")? as usize;
                let o = d.replace_line(line, &m, rep, literal)?;
                (if o > 0 { 1 } else { 0 }, o)
            } else {
                d.replace_all(&m, rep, literal)?
            };
            Ok(json!({"lines_changed": lines, "occurrences": occ, "info": info_json(&d.info())}))
        }
        "doc/format" => {
            let d = app.doc(p)?;
            let changed = d.format(indent_of(p))?;
            Ok(json!({"changed": changed, "info": info_json(&d.info())}))
        }
        "doc/fix" => {
            let d = app.doc(p)?;
            let before = d.version();
            let rep = d.fix(indent_of(p))?;
            Ok(json!({"report": report_json(&rep), "changed": d.version() != before, "info": info_json(&d.info())}))
        }
        "doc/undo" => {
            let d = app.doc(p)?;
            Ok(json!({"ok": d.undo(), "info": info_json(&d.info())}))
        }
        "doc/redo" => {
            let d = app.doc(p)?;
            Ok(json!({"ok": d.redo(), "info": info_json(&d.info())}))
        }
        "doc/save" => {
            let d = app.doc(p)?;
            let to = gs(p, "path").filter(|s| !s.trim().is_empty()).map(|s| expand(s.trim()));
            let path = d.save(to)?;
            Ok(json!({"path": path.display().to_string(), "info": info_json(&d.info())}))
        }
        "doc/close" => {
            if let Ok(id) = gu(p, "id") {
                app.tables.lock().unwrap().remove(&id);
            }
            let id = gu(p, "id")?;
            app.docs.lock().unwrap().remove(&id);
            if let Some(j) = app.doc_jobs.lock().unwrap().remove(&format!("doc:{id}")) {
                if let Some(job) = app.jobs.lock().unwrap().remove(&j) {
                    job.cancel.store(true, Ordering::Relaxed);
                }
            }
            Ok(json!({"ok": true}))
        }

        // ---- compare
        "diff/new" => {
            let l = app.resolve_side(p.get("left").unwrap_or(&Value::Null))?;
            let r = app.resolve_side(p.get("right").unwrap_or(&Value::Null))?;
            let (sid, state) = app.new_diff(l, r);
            Ok(json!({"sid": sid, "state": state}))
        }
        "diff/state" => {
            let s = app.sess(p)?;
            let mut s = s.lock().unwrap();
            s.sync();
            Ok(s.state())
        }
        "diff/rows" => {
            let s = app.sess(p)?;
            let m = matcher_from(p)?;
            let mut s = s.lock().unwrap();
            s.sync();
            let lc = s.left.current();
            let rc = s.right.current();
            let from = gu(p, "from")? as usize;
            let count = (gu(p, "count")? as usize).min(500);
            let end = (from + count).min(s.view.total());
            let rows: Vec<Value> = (from..end).map(|r| s.row_json(r, &lc, &rc, m.as_ref())).collect();
            Ok(json!({"rows": rows, "total": s.view.total(), "lver": s.lver, "rver": s.rver}))
        }
        "diff/nav" => {
            let s = app.sess(p)?;
            let mut s = s.lock().unwrap();
            s.sync();
            let row = p.get("row").and_then(|v| v.as_i64()).unwrap_or(-1);
            let dir = p.get("dir").and_then(|v| v.as_i64()).unwrap_or(1);
            let ch = &s.changes;
            if ch.is_empty() {
                return Ok(json!({"none": true}));
            }
            let (idx, wrapped) = if dir >= 0 {
                let i = ch.partition_point(|&c| (c as i64) <= row);
                if i < ch.len() { (i, false) } else { (0, true) }
            } else {
                let i = ch.partition_point(|&c| (c as i64) < row);
                if i > 0 { (i - 1, false) } else { (ch.len() - 1, true) }
            };
            Ok(json!({"row": ch[idx], "index": idx, "total": ch.len(), "wrapped": wrapped}))
        }
        "diff/rank" => {
            let s = app.sess(p)?;
            let s = s.lock().unwrap();
            let row = p.get("row").and_then(|v| v.as_i64()).unwrap_or(0);
            Ok(json!({"rank": s.changes.partition_point(|&c| (c as i64) <= row), "total": s.changes.len()}))
        }
        "diff/ruler" => {
            let s = app.sess(p)?;
            let mut s = s.lock().unwrap();
            s.sync();
            let b = (gu(p, "buckets")? as usize).clamp(1, 4096);
            Ok(json!({"cells": s.view.ruler(b)}))
        }
        "diff/opts" => {
            let s = app.sess(p)?;
            let mut s = s.lock().unwrap();
            if let Some(h) = gb(p, "hide_unchanged") {
                s.ctx = if h { Some(gu(p, "ctx").unwrap_or(3) as usize) } else { None };
            }
            if let Some(v) = gb(p, "ws") {
                s.ws = v;
            }
            if let Some(v) = gb(p, "case") {
                s.case = v;
            }
            if let Some(v) = gb(p, "mask") {
                s.mask = v;
            }
            s.recompute();
            Ok(s.state())
        }
        "diff/apply" => {
            let s = app.sess(p)?;
            let mut s = s.lock().unwrap();
            s.sync();
            let to_right = gs(p, "dir") == Some("right");
            s.apply_row(gu(p, "row")? as usize, to_right)?;
            Ok(s.state())
        }
        "diff/edit" => {
            // type over one line of one side (the cell the user double-clicked)
            let s = app.sess(p)?;
            let mut s = s.lock().unwrap();
            s.sync();
            let doc = if gs(p, "side") == Some("right") { s.right.clone() } else { s.left.clone() };
            let line = gu(p, "line")? as usize;
            let Some(text) = gs(p, "text") else { return bad("missing text") };
            if text.contains('\n') || text.contains('\r') {
                return bad("a single line cannot contain a line break");
            }
            let cur = doc.current();
            if line == 0 || line > cur.count() {
                return bad("line out of range");
            }
            let mut ins = text.as_bytes().to_vec();
            ins.extend_from_slice(cur.eol(line - 1));
            doc.splice(line - 1, 1, &ins)?;
            s.sync();
            Ok(s.state())
        }
        "diff/apply_hunk" => {
            let s = app.sess(p)?;
            let mut s = s.lock().unwrap();
            s.sync();
            let to_right = gs(p, "dir") == Some("right");
            s.apply_hunk(gu(p, "op")? as usize, to_right)?;
            Ok(s.state())
        }
        "diff/apply_all" => {
            let s = app.sess(p)?;
            let mut s = s.lock().unwrap();
            s.sync();
            s.apply_all(gs(p, "dir") == Some("right"))?;
            Ok(s.state())
        }
        "diff/undo" => {
            let s = app.sess(p)?;
            let mut s = s.lock().unwrap();
            s.sync();
            match s.undo.pop() {
                Some(d) => {
                    d.undo();
                    s.recompute();
                    Ok(s.state())
                }
                None => bad("nothing to undo here"),
            }
        }
        "diff/find" => {
            let s = app.sess(p)?;
            let m = matcher_from(p)?;
            let Some(m) = m else { return Ok(json!({"none": true})) };
            let mut s = s.lock().unwrap();
            s.sync();
            let lc = s.left.current();
            let rc = s.right.current();
            let total = s.view.total();
            if total == 0 {
                return Ok(json!({"none": true}));
            }
            let from = p.get("row").and_then(|v| v.as_i64()).unwrap_or(-1);
            let dir = p.get("dir").and_then(|v| v.as_i64()).unwrap_or(1);
            for step in 1..=total as i64 {
                let r = if dir >= 0 { (from + step).rem_euclid(total as i64) } else { (from - step).rem_euclid(total as i64) } as usize;
                let row = s.view.row(r);
                if row.kind == RowKind::Fold {
                    continue;
                }
                let hit = row.left.is_some_and(|i| m.line_matches(lc.line(i))) || row.right.is_some_and(|i| m.line_matches(rc.line(i)));
                if hit {
                    let wrapped = if dir >= 0 { (r as i64) <= from } else { (r as i64) >= from };
                    return Ok(json!({"row": r, "wrapped": wrapped}));
                }
            }
            Ok(json!({"none": true}))
        }
        "diff/close" => {
            app.diffs.lock().unwrap().remove(&gu(p, "sid")?);
            Ok(json!({"ok": true}))
        }
        "quit" => {
            std::thread::spawn(|| {
                std::thread::sleep(Duration::from_millis(200));
                std::process::exit(0);
            });
            Ok(json!({"ok": true}))
        }
        _ => Err(ApiError { status: 404, msg: format!("unknown API '{name}'") }),
    }
}

// ───────────────────────────── HTTP plumbing ─────────────────────────────

fn hdr(k: &str, v: &str) -> Header {
    Header::from_bytes(k.as_bytes(), v.as_bytes()).unwrap()
}

fn respond_json(req: tiny_http::Request, status: u16, v: &Value) {
    let body = v.to_string();
    let r = Response::from_string(body)
        .with_status_code(status)
        .with_header(hdr("Content-Type", "application/json; charset=utf-8"))
        .with_header(hdr("X-Content-Type-Options", "nosniff"))
        .with_header(hdr("Cache-Control", "no-store"));
    let _ = req.respond(r);
}

/// Compares two secrets without stopping at the first differing byte.
fn same_secret(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

fn header_value(req: &tiny_http::Request, name: &str) -> Option<String> {
    req.headers().iter().find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name)).map(|h| h.value.as_str().to_string())
}

fn query_param(url: &str, key: &str) -> Option<String> {
    let q = url.split_once('?')?.1;
    q.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        if k == key { Some(v.to_string()) } else { None }
    })
}

fn handle(app: &Arc<App>, mut req: tiny_http::Request) {
    // DNS-rebinding protection: only accept our own loopback host names
    let host_ok = header_value(&req, "Host").is_some_and(|h| h == format!("127.0.0.1:{}", app.port) || h == format!("localhost:{}", app.port));
    if !host_ok {
        return respond_json(req, 403, &json!({"error": "bad host"}));
    }
    let url = req.url().to_string();
    let path = url.split('?').next().unwrap_or("").to_string();

    if *req.method() == Method::Get && (path == "/" || path == "/index.html") {
        if !query_param(&url, "t").is_some_and(|t| same_secret(&t, &app.token)) {
            let r = Response::from_string("kempt: open the exact URL printed in the terminal (it contains the access token).").with_status_code(403);
            let _ = req.respond(r);
            return;
        }
        // embed the saved settings so the page starts in the right theme with no flash (`<` escaped: safe inside <script>)
        let state = app.store.get().to_string().replace('<', "\\u003c");
        let page = INDEX_HTML.replacen("/*@STATE@*/{}/*@END@*/", &state, 1);
        let r = Response::from_string(page)
            .with_header(hdr("Content-Type", "text/html; charset=utf-8"))
            .with_header(hdr("Cache-Control", "no-store"))
            .with_header(hdr("X-Frame-Options", "DENY"))
            .with_header(hdr("Referrer-Policy", "no-referrer"))
            .with_header(hdr("Content-Security-Policy", "default-src 'self'; style-src 'self' 'unsafe-inline'; script-src 'self' 'unsafe-inline'; connect-src 'self'; img-src 'self' data:; font-src 'self' data:; object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'"))
            .with_header(hdr("X-Content-Type-Options", "nosniff"));
        let _ = req.respond(r);
        return;
    }

    if *req.method() == Method::Get && matches!(path.as_str(), "/favicon.svg" | "/favicon.png" | "/favicon.ico") {
        // the tab icon: no token needed (browsers fetch it without one), nothing sensitive in it
        let r = if path == "/favicon.svg" {
            Response::from_data(FAVICON_SVG.as_bytes().to_vec()).with_header(hdr("Content-Type", "image/svg+xml"))
        } else {
            Response::from_data(FAVICON_PNG.to_vec()).with_header(hdr("Content-Type", "image/png"))
        };
        let _ = req.respond(r.with_header(hdr("Cache-Control", "max-age=86400")).with_header(hdr("X-Content-Type-Options", "nosniff")));
        return;
    }

    if *req.method() == Method::Get && path == "/kempt-calendar.js" {
        // the calendar module: plain code, nothing private in it
        let r = Response::from_string(include_str!("../ui/kempt-calendar.js"))
            .with_header(hdr("Content-Type", "text/javascript; charset=utf-8"))
            .with_header(hdr("Cache-Control", "no-store"))
            .with_header(hdr("X-Content-Type-Options", "nosniff"));
        let _ = req.respond(r);
        return;
    }

    if *req.method() == Method::Get && path == "/ping" {
        let _ = req.respond(Response::from_string("kempt"));
        return;
    }
    let Some(name) = path.strip_prefix("/api/") else {
        return respond_json(req, 404, &json!({"error": "not found"}));
    };
    if *req.method() != Method::Post {
        return respond_json(req, 405, &json!({"error": "POST only"}));
    }
    if !header_value(&req, "X-Token").is_some_and(|t| same_secret(&t, &app.token)) {
        return respond_json(req, 403, &json!({"error": "missing or wrong token"}));
    }
    let mut body = String::new();
    if req.as_reader().take(12 << 20).read_to_string(&mut body).is_err() {
        return respond_json(req, 400, &json!({"error": "bad request body"}));
    }
    let params: Value = if body.trim().is_empty() { json!({}) } else { serde_json::from_str(&body).unwrap_or(Value::Null) };
    if params.is_null() {
        return respond_json(req, 400, &json!({"error": "invalid JSON body"}));
    }
    let name = name.to_string();
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| api(app, &name, &params)));
    match res {
        Ok(Ok(v)) => respond_json(req, 200, &v),
        Ok(Err(e)) => respond_json(req, e.status, &json!({"error": e.msg})),
        Err(_) => respond_json(req, 500, &json!({"error": "internal error (see the terminal)"})),
    }
}

/// Announces reminders from the program itself (desktop notification + sound), so they work with no browser open.
/// It sleeps until the next due time; saving a note wakes it. `n` marks an item as already announced.
fn reminder_loop(app: Arc<App>) {
    let now_ms = || std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
    loop {
        let now = now_ms();
        let doc = app.notes.get();
        let mut due: Vec<(String, Value, String)> = vec![];
        let mut next: Option<i64> = None;
        for it in doc["items"].as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
            let (Some(id), Some(d)) = (it["id"].as_str(), it["due"].as_f64()) else { continue };
            if it["done"] == true || it["n"] == true {
                continue;
            }
            if (d as i64) <= now {
                due.push((id.to_string(), it["due"].clone(), it["t"].as_str().unwrap_or("").to_string()));
            } else {
                next = Some(next.map_or(d as i64, |n| n.min(d as i64)));
            }
        }
        if !due.is_empty() {
            let flagged = app.notes.modify(|m| {
                if let Some(a) = m.get_mut("items").and_then(Value::as_array_mut) {
                    for it in a.iter_mut() {
                        if due.iter().any(|(id, d, _)| it["id"].as_str() == Some(id) && it["due"] == *d) {
                            it["n"] = json!(true);
                        }
                    }
                }
            });
            if flagged.is_ok() {
                let body = if due.len() == 1 { due[0].2.clone() } else { format!("{} reminders: {}", due.len(), due.iter().take(3).map(|d| d.2.as_str()).collect::<Vec<_>>().join(", ")) };
                crate::service::notify("Kempt reminder", &body);
                if app.store.get()["sound"] != false {
                    crate::service::play_alert();
                }
            }
            continue;
        }
        let wait = next.map_or(30_000, |n| (n - now).clamp(1, 30_000)) as u64;
        let (m, c) = &app.wake;
        let g = m.lock().unwrap();
        let _ = c.wait_timeout(g, Duration::from_millis(wait));
    }
}

fn random_token() -> String {
    let mut b = [0u8; 16];
    crate::plat::random_bytes(&mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub struct GuiOpts {
    pub files: Vec<PathBuf>,
    pub port: u16,
    pub no_open: bool,
    pub as_tabs: bool,
    pub background: bool,
    pub app_window: bool,
    pub view: Option<String>,
}

/// Start the server (blocking). Prints the URL; opens the browser unless `no_open`.
pub fn run(o: GuiOpts) -> Result<()> {
    // one Kempt per user: a second `kempt gui` just opens the running one (with the files you gave)
    if let Some(i) = crate::service::running_instance() {
        if !o.background {
            let mut url = format!("http://127.0.0.1:{}/?t={}", i.port, i.token);
            for f in &o.files {
                let abs = crate::plat::canonical(f).unwrap_or_else(|_| f.clone());
                url.push_str(&format!("&open={}", crate::service::pct(&abs.display().to_string())));
            }
            if let Some(v) = &o.view {
                url.push_str(&format!("&view={v}"));
            }
            println!("Kempt is already running; opening it.");
            if o.no_open {
                println!("{url}");
            }
            if !o.no_open {
                let own = o.app_window || crate::state::Store::new().get()["appwin"] == true;
                crate::service::open_ui(&url, own);
            }
        }
        return Ok(());
    }
    let server = Server::http(("127.0.0.1", o.port)).map_err(|e| anyhow::anyhow!("cannot start the server: {e}"))?;
    let port = server.server_addr().to_ip().map(|a| a.port()).unwrap_or(o.port);
    let app = Arc::new(App {
        token: random_token(),
        port,
        next: AtomicU64::new(1),
        docs: Mutex::new(HashMap::new()),
        jobs: Mutex::new(HashMap::new()),
        doc_jobs: Mutex::new(HashMap::new()),
        diffs: Mutex::new(HashMap::new()),
        startup: Mutex::new(vec![]),
        tables: Mutex::new(HashMap::new()),
        store: crate::state::Store::new(),
        notes: crate::state::Store::notes(),
        calendar: crate::state::Store::calendar(),
        wake: (Mutex::new(false), std::sync::Condvar::new()),
        own_window: o.app_window || crate::state::Store::new().get()["appwin"] == true,
    });

    // files given on the command line (an Excel workbook becomes one table tab per sheet)
    let is_book = |f: &std::path::Path| {
        let l = f.display().to_string().to_ascii_lowercase();
        [".xlsx", ".xlsm", ".xltx", ".xltm", ".xlsb"].iter().any(|e| l.ends_with(e))
    };
    let mut opened: Vec<Arc<Doc>> = Vec::new();
    let mut tabs = Vec::new();
    let any_book = o.files.iter().any(|f| is_book(f));
    for f in &o.files {
        if is_book(f) {
            let r = app.open_book(&f.display().to_string()).map_err(|e| anyhow::anyhow!("{}", e.msg))?;
            tabs.push(json!({"kind": "doc", "info": r["info"], "minified": false}));
            for i in r["also"].as_array().cloned().unwrap_or_default() {
                tabs.push(json!({"kind": "doc", "info": i, "minified": false}));
            }
        } else {
            let d = app.open_path(&f.display().to_string()).map_err(|e| anyhow::anyhow!("{}", e.msg))?;
            opened.push(d);
        }
    }
    if opened.len() == 2 && !o.as_tabs && !any_book {
        let (sid, state) = app.new_diff(opened[0].clone(), opened[1].clone());
        tabs.push(json!({"kind": "diff", "sid": sid, "state": state}));
    } else {
        for d in &opened {
            let i = d.info();
            tabs.push(json!({"kind": "doc", "info": info_json(&i), "minified": looks_minified(&i)}));
        }
    }
    *app.startup.lock().unwrap() = tabs;

    let url = format!("http://127.0.0.1:{port}/?t={}", app.token);
    crate::service::write_instance(port, &app.token);
    if !o.background {
        println!("kempt GUI is running:\n\n    {url}\n");
        println!("Press Ctrl+C to stop.  (Remote machine? forward the port: ssh -L {port}:127.0.0.1:{port} <host>)");
        if !o.no_open {
            crate::service::open_ui(&o.view.as_ref().map_or(url.clone(), |v| format!("{url}&view={v}")), app.own_window);
        }
    }
    {
        let a = app.clone();
        std::thread::spawn(move || reminder_loop(a));
    }
    if std::env::var_os("KEMPT_NO_TRAY").is_none() {
        crate::service::start_tray(url.clone(), app.own_window);
    }

    let server = Arc::new(server);
    let mut handles = Vec::new();
    for _ in 0..8 {
        let s = server.clone();
        let a = app.clone();
        handles.push(std::thread::spawn(move || {
            for req in s.incoming_requests() {
                handle(&a, req);
            }
        }));
    }
    for h in handles {
        let _ = h.join();
    }
    Ok(())
}
