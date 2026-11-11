//! `kempt diff`: text, format-then-diff and structural (JSON) comparison.

use crate::diff::{self, Lines, Opts};
use crate::fmtcmd::{self, Lang};
use crate::sdiff::{self, Change};
use crate::source::{Source, looks_binary};
use crate::theme::{ColorMode, Painter, ThemeKind};
use crate::{diffview, jsonfmt, render, xmlfmt};
use anyhow::{Context, Result, bail};
use std::fs::File;
use std::io::{BufReader, BufWriter, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Mode {
    Auto,
    Text,
    Json,
    Format,
}

impl std::str::FromStr for Mode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "auto" => Ok(Mode::Auto),
            "text" => Ok(Mode::Text),
            "json" | "structural" => Ok(Mode::Json),
            "format" => Ok(Mode::Format),
            _ => Err("mode must be auto, text, json or format".into()),
        }
    }
}

pub struct DiffArgs {
    pub a: PathBuf,
    pub b: PathBuf,
    pub mode: Mode,
    pub ignore_ws: bool,
    pub ignore_case: bool,
    pub mask: bool,
    pub context: usize,
    pub side_by_side: bool,
    pub width: Option<usize>,
    pub stat: bool,
    pub patch: Option<PathBuf>,
    pub html: Option<PathBuf>,
    pub theme: ThemeKind,
    pub no_color: bool,
    pub force_color: bool,
    pub plain: bool,
    pub tui: bool,
    pub struct_limit_mb: u64,
    pub timeout_secs: u64,
    pub indent: usize,
    pub quiet: bool,
}

fn strip_gz(p: &Path) -> PathBuf {
    let mut q = p.to_path_buf();
    if q.extension().is_some_and(|e| e == "gz") {
        q.set_extension("");
    }
    q
}

fn resolve_mode(a: &DiffArgs) -> Mode {
    if a.mode != Mode::Auto {
        return a.mode;
    }
    let la = fmtcmd::lang_of_path(&strip_gz(&a.a)).map(|l| l.0);
    let lb = fmtcmd::lang_of_path(&strip_gz(&a.b)).map(|l| l.0);
    let ext_is = |p: &Path, e: &[&str]| strip_gz(p).extension().is_some_and(|x| e.contains(&x.to_string_lossy().to_ascii_lowercase().as_str()));
    let jsonl = ext_is(&a.a, &["jsonl", "ndjson"]) || ext_is(&a.b, &["jsonl", "ndjson"]);
    match (la, lb) {
        (Some(Lang::Json), Some(Lang::Json)) if !jsonl => Mode::Json,
        (Some(Lang::Xml), Some(Lang::Xml)) => Mode::Format,
        _ => Mode::Text,
    }
}

fn format_to_temp(src: &Source, lang: Lang, indent: usize, label: &str) -> Result<(Source, tempfile::NamedTempFile)> {
    let mut tmp = tempfile::NamedTempFile::new()?;
    {
        let mut w = BufWriter::with_capacity(1 << 20, tmp.as_file_mut());
        let r = match lang {
            Lang::Json => jsonfmt::format(src.bytes(), &mut w, Some(indent)),
            Lang::Xml => xmlfmt::format(BufReader::new(src.bytes()), &mut w, Some(indent)),
            _ => bail!("format mode supports JSON and XML only"),
        };
        r.with_context(|| format!("cannot format {label}"))?;
        w.flush()?;
    }
    let f = tmp.reopen()?;
    Ok((Source::from_file(&f, vec![])?, tmp))
}

pub fn run(args: DiffArgs) -> Result<i32> {
    let mode = resolve_mode(&args);
    let a_name = args.a.display().to_string();
    let b_name = args.b.display().to_string();
    let sa = Source::open(&args.a)?;
    let sb = Source::open(&args.b)?;

    // identical bytes: fast path
    if sa.bytes() == sb.bytes() {
        if !args.quiet {
            println!("Files are identical ({} bytes)", sa.len());
        }
        return Ok(0);
    }

    let color_mode = crate::theme::stdout_mode(args.no_color, args.force_color);
    let p = Painter::new(color_mode, args.theme);

    if (looks_binary(sa.bytes()) || looks_binary(sb.bytes())) && mode != Mode::Json {
        println!("Binary files {a_name} and {b_name} differ");
        return Ok(1);
    }

    if mode == Mode::Json {
        let limit = args.struct_limit_mb << 20;
        if sa.len() as u64 > limit || sb.len() as u64 > limit {
            if !args.quiet {
                eprintln!("note: files exceed {} MB; using format-then-diff instead of structural diff", args.struct_limit_mb);
            }
            return text_diff(&args, Mode::Format, sa, sb, &a_name, &b_name, p);
        }
        return structural(&args, &sa, &sb, &a_name, &b_name, p);
    }
    text_diff(&args, mode, sa, sb, &a_name, &b_name, p)
}

fn parse_json(s: &Source, label: &str) -> Result<serde_json::Value> {
    let mut b = s.bytes();
    if b.starts_with(&[0xEF, 0xBB, 0xBF]) {
        b = &b[3..];
    }
    serde_json::from_slice(b).with_context(|| format!("{label}: invalid JSON (for JSONL/multi-value files use --mode text or --mode format)"))
}

fn structural(args: &DiffArgs, sa: &Source, sb: &Source, a_name: &str, b_name: &str, p: Painter) -> Result<i32> {
    let va = parse_json(sa, a_name)?;
    let vb = parse_json(sb, b_name)?;
    let changes = sdiff::diff(&va, &vb);
    let (mut add, mut rem, mut chg, mut ty) = (0, 0, 0, 0);
    for c in &changes {
        match c {
            Change::Added(..) => add += 1,
            Change::Removed(..) => rem += 1,
            Change::Changed(..) => chg += 1,
            Change::TypeChanged(..) => ty += 1,
        }
    }
    if changes.is_empty() {
        if !args.quiet {
            println!("Files are structurally identical (same data; only formatting or key order differ)");
        }
        return Ok(0);
    }
    let t = &p.t;
    let out = std::io::stdout();
    let mut w = BufWriter::new(out.lock());
    if !args.stat {
        if p.on() {
            writeln!(w, "{}{}--- {}{}\n{}{}+++ {}{}", p.bold(), p.fg(t.del_strong), a_name, p.reset(), p.bold(), p.fg(t.add_strong), b_name, p.reset())?;
        } else {
            writeln!(w, "--- {a_name}\n+++ {b_name}")?;
        }
        for c in &changes {
            let path = c.path();
            let paint = |bg, fg_strong, s: &str| format!("{}{}{}{}{}", p.bg(bg), p.fg(fg_strong), p.bold(), s, p.reset());
            match c {
                Change::Added(_, v) => writeln!(w, "{} {}  {}", paint(t.add_bg, t.add_strong, "+"), path, paint(t.add_bg, t.fg, &sdiff::brief(v, 160)))?,
                Change::Removed(_, v) => writeln!(w, "{} {}  {}", paint(t.del_bg, t.del_strong, "-"), path, paint(t.del_bg, t.fg, &sdiff::brief(v, 160)))?,
                Change::Changed(_, x, y) => writeln!(
                    w,
                    "{} {}  {} → {}",
                    format_args!("{}{}~{}", p.fg(t.chg_gutter), p.bold(), p.reset()),
                    path,
                    paint(t.del_strong, t.fg, &sdiff::brief(x, 80)),
                    paint(t.add_strong, t.fg, &sdiff::brief(y, 80))
                )?,
                Change::TypeChanged(_, x, y) => writeln!(
                    w,
                    "{} {}  {} → {}   (type changed)",
                    format_args!("{}{}⇄{}", p.fg(t.moved_fg), p.bold(), p.reset()),
                    path,
                    paint(t.del_strong, t.fg, &sdiff::brief(x, 80)),
                    paint(t.add_strong, t.fg, &sdiff::brief(y, 80))
                )?,
            }
        }
    }
    writeln!(
        w,
        "{}{} added{}, {}{} removed{}, {}{} changed{}, {}{} type-changed{}  (structural; key order and formatting ignored)",
        p.fg(t.add_strong), add, p.reset(), p.fg(t.del_strong), rem, p.reset(), p.fg(t.chg_gutter), chg, p.reset(), p.fg(t.moved_fg), ty, p.reset()
    )?;
    w.flush()?;
    Ok(if changes.is_empty() { 0 } else { 1 })
}

fn text_diff(args: &DiffArgs, mode: Mode, sa: Source, sb: Source, a_name: &str, b_name: &str, p: Painter) -> Result<i32> {
    // format-then-diff: normalise layout of both files first
    let (sa, sb, _keep) = if mode == Mode::Format {
        let la = fmtcmd::lang_of_path(&strip_gz(&args.a)).map(|l| l.0);
        let lb = fmtcmd::lang_of_path(&strip_gz(&args.b)).map(|l| l.0);
        match (la, lb) {
            (Some(x), Some(y)) if x == y && matches!(x, Lang::Json | Lang::Xml) => {
                let (fa, ta) = format_to_temp(&sa, x, args.indent, a_name)?;
                let (fb, tb) = format_to_temp(&sb, x, args.indent, b_name)?;
                (fa, fb, vec![ta, tb])
            }
            _ => bail!("--mode format needs two JSON or two XML files"),
        }
    } else {
        (sa, sb, vec![])
    };

    let a = Lines::new(sa.bytes());
    let b = Lines::new(sb.bytes());
    let opts = Opts {
        ignore_case: args.ignore_case,
        ignore_ws: args.ignore_ws,
        mask: args.mask,
        timeout: if args.timeout_secs > 0 { Some(Duration::from_secs(args.timeout_secs)) } else { None },
    };
    let res = diff::diff_lines(&a, &b, &opts);
    if res.timed_out && !args.quiet {
        eprintln!("warning: diff timed out after {}s; result is approximate (more lines may be marked changed than necessary)", args.timeout_secs);
    }
    let ops = res.ops;
    let st = diff::stats(&ops);
    let mut identical = diff::is_identical(&ops);
    let eol_only = identical && !opts.normalising() && a.ends_with_newline() != b.ends_with_newline() && a.len() > 0 && b.len() > 0;
    if eol_only {
        identical = false;
    }

    if identical {
        if !args.quiet {
            if opts.normalising() {
                println!("Files are identical after normalisation (case/whitespace/mask options applied)");
            } else {
                println!("Files are identical");
            }
        }
        return Ok(0);
    }

    // exports
    if args.patch.is_some() && opts.normalising() {
        bail!("--patch cannot be combined with --ignore-case/--ignore-space/--mask (the patch would not apply exactly)");
    }
    if let Some(path) = &args.patch {
        let mut w = BufWriter::new(File::create(path)?);
        let plain = Painter::new(ColorMode::None, args.theme);
        render::write_unified(&mut w, &plain, a_name, b_name, &a, &b, &ops, args.context)?;
        w.flush()?;
        if !args.quiet {
            eprintln!("wrote patch: {}", path.display());
        }
    }
    if let Some(path) = &args.html {
        let mut w = BufWriter::new(File::create(path)?);
        // the HTML report always uses colour theme; fold equal runs unless context is huge
        render::write_html(&mut w, &p, a_name, b_name, &a, &b, &ops, Some(args.context), 300_000)?;
        w.flush()?;
        if !args.quiet {
            eprintln!("wrote HTML report: {}", path.display());
        }
    }

    let summary = format!(
        "{}+{} added{}  {}-{} removed{}  {}~{} changed{}  ({} hunk{}){}",
        p.fg(p.t.add_strong), st.added, p.reset(), p.fg(p.t.del_strong), st.removed, p.reset(), p.fg(p.t.chg_gutter), st.changed, p.reset(),
        st.hunks, if st.hunks == 1 { "" } else { "s" }, if eol_only { "  [differ only in final newline]" } else { "" }
    );

    let want_tui = args.tui || (!args.plain && std::io::stdout().is_terminal() && std::io::stdin().is_terminal());
    let export_only = (args.patch.is_some() || args.html.is_some()) && !args.tui;
    if args.stat {
        println!("{summary}");
    } else if export_only {
        // exports requested: just the summary
        println!("{summary}");
    } else if want_tui {
        diffview::run(&a, &b, &ops, a_name, b_name, p, args.context)?;
        println!("{summary}");
    } else {
        let out = std::io::stdout();
        let mut w = BufWriter::with_capacity(1 << 16, out.lock());
        if args.side_by_side {
            let width = args.width.unwrap_or_else(|| crossterm::terminal::size().map(|s| s.0 as usize).unwrap_or(160));
            writeln!(w, "{}{}{} ↔ {}{}", p.bold(), p.fg(p.t.fg), a_name, b_name, p.reset())?;
            render::write_side_by_side(&mut w, &p, &a, &b, &ops, Some(args.context), width)?;
        } else {
            render::write_unified(&mut w, &p, a_name, b_name, &a, &b, &ops, args.context)?;
        }
        if p.on() {
            writeln!(w, "{summary}")?;
        }
        w.flush()?;
    }
    Ok(1)
}
