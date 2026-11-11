//! Diff rendering: row model shared by side-by-side, HTML and the TUI, plus
//! unified (patch-compatible) output.

use crate::diff::{Kind, Lines, Op, Seg, inline_diff};
use crate::theme::{Painter, Rgb};
use std::io::{self, Write};

// ───────────────────────────── row model ─────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RowKind {
    Equal,
    Del,
    Ins,
    Chg,
    Fold,
}

#[derive(Clone, Copy, Debug)]
pub struct Row {
    pub kind: RowKind,
    pub left: Option<usize>,
    pub right: Option<usize>,
    pub fold: usize,
    /// index of the diff op this row belongs to
    pub op: usize,
    /// row offset inside that op
    pub off: usize,
}

#[derive(Clone, Copy, Debug)]
enum Seg_ {
    Rows { kind: Kind, old: usize, new: usize, old_len: usize, new_len: usize, op: usize },
    Fold(usize),
}

impl Seg_ {
    fn rows(&self) -> usize {
        match *self {
            Seg_::Rows { old_len, new_len, .. } => old_len.max(new_len),
            Seg_::Fold(_) => 1,
        }
    }
}

pub struct View {
    segs: Vec<Seg_>,
    starts: Vec<usize>,
    total: usize,
}

impl View {
    /// `ctx = None` shows everything; `Some(n)` folds equal runs keeping n lines of context.
    pub fn build(ops: &[Op], ctx: Option<usize>) -> View {
        let mut segs: Vec<Seg_> = Vec::new();
        let n_ops = ops.len();
        for (idx, op) in ops.iter().enumerate() {
            let full = Seg_::Rows { kind: op.kind, old: op.old, new: op.new, old_len: op.old_len, new_len: op.new_len, op: idx };
            match (op.kind, ctx) {
                (Kind::Equal, Some(c)) => {
                    let head = if idx == 0 { 0 } else { c };
                    let tail = if idx == n_ops - 1 { 0 } else { c };
                    let len = op.old_len;
                    if len <= head + tail + 1 {
                        segs.push(full);
                    } else {
                        if head > 0 {
                            segs.push(Seg_::Rows { kind: Kind::Equal, old: op.old, new: op.new, old_len: head, new_len: head, op: idx });
                        }
                        segs.push(Seg_::Fold(len - head - tail));
                        if tail > 0 {
                            segs.push(Seg_::Rows {
                                kind: Kind::Equal,
                                old: op.old + len - tail,
                                new: op.new + len - tail,
                                old_len: tail,
                                new_len: tail,
                                op: idx,
                            });
                        }
                    }
                }
                _ => segs.push(full),
            }
        }
        let mut starts = Vec::with_capacity(segs.len());
        let mut total = 0;
        for s in &segs {
            starts.push(total);
            total += s.rows();
        }
        View { segs, starts, total }
    }

    pub fn total(&self) -> usize {
        self.total
    }

    pub fn row(&self, r: usize) -> Row {
        let si = self.starts.partition_point(|&s| s <= r) - 1;
        let off = r - self.starts[si];
        match self.segs[si] {
            Seg_::Fold(n) => Row { kind: RowKind::Fold, left: None, right: None, fold: n, op: 0, off: 0 },
            Seg_::Rows { kind, old, new, old_len, new_len, op } => {
                let left = if off < old_len { Some(old + off) } else { None };
                let right = if off < new_len { Some(new + off) } else { None };
                let rk = match (kind, left, right) {
                    (Kind::Equal, _, _) => RowKind::Equal,
                    (_, Some(_), Some(_)) => RowKind::Chg,
                    (_, Some(_), None) => RowKind::Del,
                    _ => RowKind::Ins,
                };
                Row { kind: rk, left, right, fold: 0, op, off }
            }
        }
    }

    /// first row of each change segment (for next/prev navigation)
    pub fn change_starts(&self) -> Vec<usize> {
        let mut v = Vec::new();
        for (i, s) in self.segs.iter().enumerate() {
            if let Seg_::Rows { kind, .. } = s {
                if *kind != Kind::Equal {
                    v.push(self.starts[i]);
                }
            }
        }
        v
    }

    /// overview ruler: for `h` cells, 0=none 1=added 2=removed 3=changed
    pub fn ruler(&self, h: usize) -> Vec<u8> {
        let mut cells = vec![0u8; h];
        if self.total == 0 || h == 0 {
            return cells;
        }
        for (i, s) in self.segs.iter().enumerate() {
            let Seg_::Rows { kind, old_len, new_len, .. } = *s else { continue };
            let code = match kind {
                Kind::Equal => continue,
                Kind::Insert => 1,
                Kind::Delete => 2,
                Kind::Replace => {
                    let _ = (old_len, new_len);
                    3
                }
            };
            let a = self.starts[i];
            let b = a + s.rows();
            let ca = a * h / self.total;
            let cb = ((b - 1) * h / self.total).min(h - 1);
            for c in ca..=cb {
                cells[c] = if cells[c] == 0 || cells[c] == code { code } else { 3 };
            }
        }
        cells
    }
}

// ───────────────────────────── cell rendering ─────────────────────────────

pub fn sanitize(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\t' => o.push_str("    "),
            c if (c as u32) < 32 || c as u32 == 127 => o.push('·'),
            c => o.push(c),
        }
    }
    o
}

fn segs_plain(line: &[u8]) -> Vec<Seg> {
    vec![(String::from_utf8_lossy(line).into_owned(), false)]
}

/// Render segments clipped to [hoff, hoff+w) and padded to w columns.
pub fn cell(p: &Painter, segs: &[Seg], base: Option<Rgb>, strong: Option<Rgb>, hoff: usize, w: usize) -> String {
    let mut out = String::new();
    let mut idx = 0usize;
    let mut emitted = 0usize;
    let set_state = |out: &mut String, is_strong: bool| {
        out.push_str(p.reset());
        let bg = if is_strong { strong.or(base) } else { base };
        if let Some(c) = bg {
            out.push_str(&p.bg(c));
            out.push_str(&p.tint_fg(c));
        }
        if is_strong {
            out.push_str(p.bold());
        }
    };
    let mut cur_strong = false;
    set_state(&mut out, false);
    'outer: for (text, is_strong) in segs {
        let text = sanitize(text);
        if *is_strong != cur_strong {
            cur_strong = *is_strong;
            set_state(&mut out, cur_strong);
        }
        for ch in text.chars() {
            if idx >= hoff {
                if emitted >= w {
                    break 'outer;
                }
                out.push(ch);
                emitted += 1;
            }
            idx += 1;
        }
    }
    if cur_strong {
        set_state(&mut out, false);
    }
    for _ in emitted..w {
        out.push(' ');
    }
    out.push_str(p.reset());
    out
}

pub struct SideLayout {
    pub num_w: usize,
    pub col_w: usize,
}

impl SideLayout {
    pub fn new(total_width: usize, max_line: usize) -> Self {
        let num_w = max_line.to_string().len().max(3);
        // [num] [m] col │ [num] [m] col
        let fixed = (num_w + 2) * 2 + 1;
        let col_w = total_width.saturating_sub(fixed) / 2;
        SideLayout { num_w, col_w: col_w.max(8) }
    }
    pub fn width(&self) -> usize {
        (self.num_w + 2) * 2 + 1 + self.col_w * 2
    }
}

pub fn render_side_row(p: &Painter, row: &Row, a: &Lines, b: &Lines, lay: &SideLayout, hoff: usize) -> String {
    let t = &p.t;
    let blank = |n: usize| " ".repeat(n);
    match row.kind {
        RowKind::Fold => {
            let msg = format!("⋯ {} unchanged line{} ⋯", row.fold, if row.fold == 1 { "" } else { "s" });
            let w = lay.width();
            let pad = w.saturating_sub(msg.chars().count()) / 2;
            format!("{}{}{}{}{}", p.fg(t.fold_fg), blank(pad), msg, blank(w.saturating_sub(pad + msg.chars().count())), p.reset())
        }
        _ => {
            let (lsegs, rsegs): (Option<Vec<Seg>>, Option<Vec<Seg>>) = match row.kind {
                RowKind::Chg => {
                    let (x, y) = inline_diff(a.get(row.left.unwrap()), b.get(row.right.unwrap()));
                    (Some(x), Some(y))
                }
                _ => (row.left.map(|i| segs_plain(a.get(i))), row.right.map(|i| segs_plain(b.get(i)))),
            };
            let (lbase, rbase, lstrong, rstrong, lm, rm, mcol) = match row.kind {
                RowKind::Equal => (None, None, None, None, ' ', ' ', t.dim_fg),
                RowKind::Del => (Some(t.del_bg), None, None, None, '-', ' ', t.del_strong),
                RowKind::Ins => (None, Some(t.add_bg), None, None, ' ', '+', t.add_strong),
                RowKind::Chg => (Some(t.del_bg), Some(t.add_bg), Some(t.del_strong), Some(t.add_strong), '~', '~', t.chg_gutter),
                RowKind::Fold => unreachable!(),
            };
            let num = |n: Option<usize>| match n {
                Some(i) => format!("{:>w$}", i + 1, w = lay.num_w),
                None => blank(lay.num_w),
            };
            let mut s = String::new();
            // left
            s.push_str(&p.fg(t.dim_fg));
            s.push_str(&num(row.left));
            s.push_str(p.reset());
            s.push(' ');
            s.push_str(&p.fg(mcol));
            s.push_str(p.bold());
            s.push(lm);
            s.push_str(p.reset());
            match lsegs {
                Some(sg) => s.push_str(&cell(p, &sg, lbase, lstrong, hoff, lay.col_w)),
                None => s.push_str(&blank(lay.col_w)),
            }
            s.push_str(&p.fg(t.dim_fg));
            s.push('│');
            s.push_str(p.reset());
            // right
            s.push_str(&p.fg(t.dim_fg));
            s.push_str(&num(row.right));
            s.push_str(p.reset());
            s.push(' ');
            s.push_str(&p.fg(mcol));
            s.push_str(p.bold());
            s.push(rm);
            s.push_str(p.reset());
            match rsegs {
                Some(sg) => s.push_str(&cell(p, &sg, rbase, rstrong, hoff, lay.col_w)),
                None => s.push_str(&blank(lay.col_w)),
            }
            s
        }
    }
}

// ───────────────────────────── unified output ─────────────────────────────

struct Hunk {
    ops: Vec<Op>,
}

fn build_hunks(ops: &[Op], ctx: usize) -> Vec<Hunk> {
    let mut hunks = Vec::new();
    let mut cur: Vec<Op> = Vec::new();
    let n = ops.len();
    let eq = |op: &Op, from: usize, len: usize| Op { kind: Kind::Equal, old: op.old + from, new: op.new + from, old_len: len, new_len: len };
    for (idx, op) in ops.iter().enumerate() {
        if op.kind == Kind::Equal {
            let last = idx == n - 1;
            let len = op.old_len;
            if cur.is_empty() {
                if !last && ctx > 0 {
                    let take = ctx.min(len);
                    cur.push(eq(op, len - take, take));
                }
            } else if last {
                let take = ctx.min(len);
                if take > 0 {
                    cur.push(eq(op, 0, take));
                }
                hunks.push(Hunk { ops: std::mem::take(&mut cur) });
            } else if len > 2 * ctx {
                if ctx > 0 {
                    cur.push(eq(op, 0, ctx));
                }
                hunks.push(Hunk { ops: std::mem::take(&mut cur) });
                if ctx > 0 {
                    cur.push(eq(op, len - ctx, ctx));
                }
            } else {
                cur.push(*op);
            }
        } else {
            cur.push(*op);
        }
    }
    if cur.iter().any(|o| o.kind != Kind::Equal) {
        hunks.push(Hunk { ops: cur });
    }
    hunks
}

const INLINE_PAIR_LIMIT: usize = 2000;

fn write_raw_line<W: Write>(w: &mut W, prefix: u8, raw: &[u8]) -> io::Result<()> {
    w.write_all(&[prefix])?;
    w.write_all(raw)?;
    if raw.last() != Some(&b'\n') {
        w.write_all(b"\n\\ No newline at end of file\n")?;
    }
    Ok(())
}

fn write_colored_line<W: Write>(w: &mut W, p: &Painter, prefix: char, base: Rgb, strong: Rgb, segs: Option<&[Seg]>, raw: &[u8], last_no_nl: bool) -> io::Result<()> {
    let mut s = String::new();
    s.push_str(&p.bg(base));
    s.push_str(&p.fg(strong));
    s.push_str(p.bold());
    s.push(prefix);
    s.push_str(p.reset());
    s.push_str(&p.bg(base));
    s.push_str(&p.tint_fg(base));
    match segs {
        Some(sg) => {
            let mut strong_on = false;
            for (t, st) in sg {
                if *st != strong_on {
                    strong_on = *st;
                    s.push_str(p.reset());
                    s.push_str(&p.bg(if strong_on { strong } else { base }));
                    s.push_str(&p.tint_fg(if strong_on { strong } else { base }));
                    if strong_on {
                        s.push_str(p.bold());
                    }
                }
                s.push_str(t);
            }
        }
        None => s.push_str(&String::from_utf8_lossy(raw)),
    }
    s.push_str(p.reset());
    s.push('\n');
    w.write_all(s.as_bytes())?;
    if last_no_nl {
        w.write_all(b"\\ No newline at end of file\n")?;
    }
    Ok(())
}

/// Unified diff. With `Painter` mode None this is byte-exact and `patch(1)`-compatible.
pub fn write_unified<W: Write>(w: &mut W, p: &Painter, a_name: &str, b_name: &str, a: &Lines, b: &Lines, ops: &[Op], ctx: usize) -> io::Result<()> {
    let hunks = build_hunks(ops, ctx);
    if hunks.is_empty() {
        return Ok(());
    }
    let t = &p.t;
    if p.on() {
        writeln!(w, "{}{}--- {}{}", p.bold(), p.fg(t.del_strong), a_name, p.reset())?;
        writeln!(w, "{}{}+++ {}{}", p.bold(), p.fg(t.add_strong), b_name, p.reset())?;
    } else {
        writeln!(w, "--- {a_name}")?;
        writeln!(w, "+++ {b_name}")?;
    }
    let a_nonl = !a.ends_with_newline();
    let b_nonl = !b.ends_with_newline();
    for h in &hunks {
        let first = h.ops.first().unwrap();
        let old_count: usize = h.ops.iter().map(|o| o.old_len).sum();
        let new_count: usize = h.ops.iter().map(|o| o.new_len).sum();
        let hs = |start: usize, count: usize| if count == 0 { start } else { start + 1 };
        let header = format!("@@ -{},{} +{},{} @@", hs(first.old, old_count), old_count, hs(first.new, new_count), new_count);
        if p.on() {
            writeln!(w, "{}{}{}", p.fg(t.chg_gutter), header, p.reset())?;
        } else {
            writeln!(w, "{header}")?;
        }
        for op in &h.ops {
            match op.kind {
                Kind::Equal => {
                    for k in 0..op.old_len {
                        let i = op.old + k;
                        let raw = a.raw(i);
                        if p.on() {
                            let mut line = String::new();
                            line.push(' ');
                            line.push_str(&String::from_utf8_lossy(a.get(i)));
                            writeln!(w, "{}{}{}", p.fg(t.dim_fg), line, p.reset())?;
                            if raw.last() != Some(&b'\n') {
                                w.write_all(b"\\ No newline at end of file\n")?;
                            }
                        } else {
                            write_raw_line(w, b' ', raw)?;
                        }
                    }
                }
                Kind::Delete | Kind::Insert | Kind::Replace => {
                    let pairs = op.old_len.min(op.new_len).min(INLINE_PAIR_LIMIT);
                    let mut new_segs: Vec<Vec<Seg>> = Vec::with_capacity(pairs);
                    for k in 0..op.old_len {
                        let i = op.old + k;
                        if p.on() {
                            let segs = if k < pairs {
                                let (x, y) = inline_diff(a.get(i), b.get(op.new + k));
                                new_segs.push(y);
                                Some(x)
                            } else {
                                None
                            };
                            let nonl = a_nonl && i == a.len() - 1;
                            write_colored_line(w, p, '-', t.del_bg, t.del_strong, segs.as_deref(), a.get(i), nonl)?;
                        } else {
                            write_raw_line(w, b'-', a.raw(i))?;
                        }
                    }
                    for k in 0..op.new_len {
                        let i = op.new + k;
                        if p.on() {
                            let segs = if k < pairs { Some(new_segs[k].as_slice()) } else { None };
                            let nonl = b_nonl && i == b.len() - 1;
                            write_colored_line(w, p, '+', t.add_bg, t.add_strong, segs, b.get(i), nonl)?;
                        } else {
                            write_raw_line(w, b'+', b.raw(i))?;
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

// ───────────────────────────── side-by-side (plain) ─────────────────────────────

pub fn write_side_by_side<W: Write>(w: &mut W, p: &Painter, a: &Lines, b: &Lines, ops: &[Op], ctx: Option<usize>, width: usize) -> io::Result<()> {
    let view = View::build(ops, ctx);
    let lay = SideLayout::new(width, a.len().max(b.len()));
    for r in 0..view.total() {
        let row = view.row(r);
        let s = render_side_row(p, &row, a, b, &lay, 0);
        w.write_all(s.as_bytes())?;
        w.write_all(b"\n")?;
    }
    Ok(())
}

// ───────────────────────────── HTML export ─────────────────────────────

fn esc(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
}

fn css_rgb(c: Rgb) -> String {
    format!("rgb({},{},{})", c.0, c.1, c.2)
}

pub fn write_html<W: Write>(w: &mut W, p: &Painter, a_name: &str, b_name: &str, a: &Lines, b: &Lines, ops: &[Op], ctx: Option<usize>, max_rows: usize) -> io::Result<()> {
    let t = &p.t;
    let view = View::build(ops, ctx);
    let st = crate::diff::stats(ops);
    let mut head = String::new();
    head.push_str("<!doctype html><meta charset=utf-8><title>kempt diff</title><style>");
    head.push_str(&format!(
        "body{{background:#16181d;color:{fg};font:13px/1.45 ui-monospace,Menlo,Consolas,monospace;margin:16px}}\
         table{{border-collapse:collapse;width:100%;table-layout:fixed}}td{{vertical-align:top;white-space:pre-wrap;word-break:break-all;padding:0 6px}}\
         td.n{{width:4em;text-align:right;color:{dim};user-select:none}}td.m{{width:1.5em;text-align:center;font-weight:bold}}\
         .del{{background:{del}}}.add{{background:{add}}}.eq{{}}.fold{{text-align:center;color:{fold}}}\
         mark.d{{background:{dels};color:inherit;font-weight:bold}}mark.a{{background:{adds};color:inherit;font-weight:bold}}\
         .chg{{color:{chg}}}h1{{font-size:15px}}.sum span{{margin-right:1.2em}}",
        fg = css_rgb(t.fg), dim = css_rgb(t.dim_fg), del = css_rgb(t.del_bg), add = css_rgb(t.add_bg),
        fold = css_rgb(t.fold_fg), dels = css_rgb(t.del_strong), adds = css_rgb(t.add_strong), chg = css_rgb(t.chg_gutter)
    ));
    head.push_str("</style>");
    head.push_str("<h1>");
    esc(a_name, &mut head);
    head.push_str(" ↔ ");
    esc(b_name, &mut head);
    head.push_str("</h1>");
    head.push_str(&format!(
        "<p class=sum><span style='color:{}'>+{} added</span><span style='color:{}'>−{} removed</span><span class=chg>~{} changed</span></p><table>",
        css_rgb(t.add_strong), st.added, css_rgb(t.del_strong), st.removed, st.changed
    ));
    w.write_all(head.as_bytes())?;
    let total = view.total();
    for r in 0..total.min(max_rows) {
        let row = view.row(r);
        let mut s = String::new();
        if row.kind == RowKind::Fold {
            s.push_str(&format!("<tr><td colspan=6 class=fold>⋯ {} unchanged lines ⋯</td></tr>\n", row.fold));
            w.write_all(s.as_bytes())?;
            continue;
        }
        let (lh, rh) = match row.kind {
            RowKind::Chg => {
                let (x, y) = inline_diff(a.get(row.left.unwrap()), b.get(row.right.unwrap()));
                (Some(seg_html(&x, "d")), Some(seg_html(&y, "a")))
            }
            _ => (
                row.left.map(|i| seg_html(&[(String::from_utf8_lossy(a.get(i)).into_owned(), false)], "d")),
                row.right.map(|i| seg_html(&[(String::from_utf8_lossy(b.get(i)).into_owned(), false)], "a")),
            ),
        };
        let (lc, rc, lm, rm) = match row.kind {
            RowKind::Equal => ("eq", "eq", "", ""),
            RowKind::Del => ("del", "eq", "−", ""),
            RowKind::Ins => ("eq", "add", "", "+"),
            _ => ("del", "add", "~", "~"),
        };
        s.push_str("<tr>");
        s.push_str(&format!("<td class=n>{}</td><td class='m {}'>{}</td><td class={}>{}</td>", row.left.map(|i| (i + 1).to_string()).unwrap_or_default(), lc, lm, lc, lh.unwrap_or_default()));
        s.push_str(&format!("<td class=n>{}</td><td class='m {}'>{}</td><td class={}>{}</td>", row.right.map(|i| (i + 1).to_string()).unwrap_or_default(), rc, rm, rc, rh.unwrap_or_default()));
        s.push_str("</tr>\n");
        w.write_all(s.as_bytes())?;
    }
    if total > max_rows {
        writeln!(w, "<tr><td colspan=6 class=fold>… output truncated after {max_rows} rows ({} more) …</td></tr>", total - max_rows)?;
    }
    w.write_all(b"</table>")?;
    Ok(())
}

fn seg_html(segs: &[Seg], cls: &str) -> String {
    let mut s = String::new();
    for (t, strong) in segs {
        if *strong {
            s.push_str(&format!("<mark class={cls}>"));
            esc(t, &mut s);
            s.push_str("</mark>");
        } else {
            esc(t, &mut s);
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::{Opts, diff_lines};
    use crate::theme::{ColorMode, ThemeKind};

    fn ops_for(a: &Lines, b: &Lines) -> Vec<Op> {
        diff_lines(a, b, &Opts::default()).ops
    }

    #[test]
    fn view_rows_cover_all_lines() {
        let a = Lines::new(b"a\nb\nc\nd\ne\nf\n");
        let b = Lines::new(b"a\nB\nc\nd\ne\nf\ng\n");
        let ops = ops_for(&a, &b);
        let v = View::build(&ops, None);
        assert_eq!(v.total(), 7);
        let kinds: Vec<_> = (0..v.total()).map(|r| v.row(r).kind).collect();
        assert_eq!(kinds[0], RowKind::Equal);
        assert_eq!(kinds[1], RowKind::Chg);
        assert_eq!(kinds[6], RowKind::Ins);
    }

    #[test]
    fn view_folding() {
        let mut sa = String::new();
        let mut sb = String::new();
        for i in 0..100 {
            sa.push_str(&format!("line{i}\n"));
            sb.push_str(&if i == 50 { "CHANGED\n".to_string() } else { format!("line{i}\n") });
        }
        let a = Lines::new(sa.as_bytes());
        let b = Lines::new(sb.as_bytes());
        let ops = ops_for(&a, &b);
        let v = View::build(&ops, Some(3));
        // fold + 3 ctx + change + 3 ctx + fold
        assert_eq!(v.total(), 1 + 3 + 1 + 3 + 1);
        assert_eq!(v.row(0).kind, RowKind::Fold);
        assert_eq!(v.row(0).fold, 47);
        assert_eq!(v.change_starts(), vec![4]);
        assert_eq!(v.row(8).kind, RowKind::Fold);
        assert_eq!(v.row(8).fold, 46);
    }

    #[test]
    fn ruler_marks_changes() {
        let a = Lines::new(b"a\nb\nc\nd\n");
        let b = Lines::new(b"a\nb\nc\nd\ne\nf\n");
        let ops = ops_for(&a, &b);
        let v = View::build(&ops, None);
        let r = v.ruler(6);
        assert_eq!(r[0], 0);
        assert_eq!(r[5], 1);
    }

    #[test]
    fn unified_plain_matches_expected() {
        let a = Lines::new(b"a\nb\nc\n");
        let b = Lines::new(b"a\nX\nc\n");
        let ops = ops_for(&a, &b);
        let mut out = Vec::new();
        let p = Painter::new(ColorMode::None, ThemeKind::Dark);
        write_unified(&mut out, &p, "a.txt", "b.txt", &a, &b, &ops, 3).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "--- a.txt\n+++ b.txt\n@@ -1,3 +1,3 @@\n a\n-b\n+X\n c\n");
    }

    #[test]
    fn unified_no_newline_marker() {
        let a = Lines::new(b"a\nb");
        let b = Lines::new(b"a\nc");
        let ops = ops_for(&a, &b);
        let mut out = Vec::new();
        let p = Painter::new(ColorMode::None, ThemeKind::Dark);
        write_unified(&mut out, &p, "a", "b", &a, &b, &ops, 3).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("-b\n\\ No newline at end of file\n+c\n\\ No newline at end of file\n"), "{s}");
    }

    #[test]
    fn colored_unified_has_strong_inline_spans() {
        let a = Lines::new(b"status: ok\n");
        let b = Lines::new(b"status: fail\n");
        let ops = ops_for(&a, &b);
        let mut out = Vec::new();
        let p = Painter::new(ColorMode::True, ThemeKind::Dark);
        write_unified(&mut out, &p, "a", "b", &a, &b, &ops, 3).unwrap();
        let s = String::from_utf8(out).unwrap();
        // strong del colour (168,42,54) and strong add colour (34,134,58) backgrounds present
        assert!(s.contains("48;2;168;42;54"), "{s:?}");
        assert!(s.contains("48;2;34;134;58"), "{s:?}");
    }

    #[test]
    fn cell_clips_and_pads() {
        let p = Painter::new(ColorMode::None, ThemeKind::Dark);
        let c = cell(&p, &[("hello world".into(), false)], None, None, 0, 5);
        assert_eq!(c, "hello");
        let c = cell(&p, &[("hi".into(), false)], None, None, 0, 5);
        assert_eq!(c, "hi   ");
        let c = cell(&p, &[("hello world".into(), false)], None, None, 6, 5);
        assert_eq!(c, "world");
    }

    #[test]
    fn hunks_split_on_long_equal_runs() {
        let mut sa = String::new();
        let mut sb = String::new();
        for i in 0..40 {
            sa.push_str(&format!("l{i}\n"));
            sb.push_str(&if i == 2 || i == 35 { format!("X{i}\n") } else { format!("l{i}\n") });
        }
        let a = Lines::new(sa.as_bytes());
        let b = Lines::new(sb.as_bytes());
        let ops = ops_for(&a, &b);
        assert_eq!(build_hunks(&ops, 3).len(), 2);
        assert_eq!(build_hunks(&ops, 40).len(), 1);
    }
}
