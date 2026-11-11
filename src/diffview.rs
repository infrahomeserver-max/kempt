//! Interactive side-by-side diff viewer (TUI).

use crate::diff::{Lines, Op, stats};
use crate::render::{SideLayout, View, render_side_row};
use crate::theme::Painter;
use crate::tui::{self, Term, fit};
use anyhow::Result;
use crossterm::event::{Event, KeyCode, KeyModifiers, MouseButton, MouseEventKind};
use std::time::Duration;

struct State<'a> {
    a: &'a Lines<'a>,
    b: &'a Lines<'a>,
    ops: &'a [Op],
    ctx: usize,
    fold: bool,
    view: View,
    changes: Vec<usize>,
    cur: Option<usize>,
    top: usize,
    hoff: usize,
    help: bool,
}

impl<'a> State<'a> {
    fn rebuild(&mut self) {
        self.view = View::build(self.ops, if self.fold { Some(self.ctx) } else { None });
        self.changes = self.view.change_starts();
        self.cur = None;
        self.top = 0;
    }

    fn body_h(h: usize) -> usize {
        h.saturating_sub(2).max(1)
    }

    fn clamp(&mut self, bh: usize) {
        let max_top = self.view.total().saturating_sub(bh);
        self.top = self.top.min(max_top);
    }
}

pub fn run(a: &Lines, b: &Lines, ops: &[Op], a_name: &str, b_name: &str, p: Painter, ctx: usize) -> Result<()> {
    let mut st = State { a, b, ops, ctx, fold: true, view: View::build(ops, Some(ctx)), changes: vec![], cur: None, top: 0, hoff: 0, help: false };
    st.changes = st.view.change_starts();
    let sum = stats(ops);
    let _term = Term::enter()?;
    // jump to first change initially
    if let Some(&c) = st.changes.first() {
        st.top = c.saturating_sub(3);
        st.cur = Some(0);
    }
    let mut dirty = true;
    loop {
        let (w, h) = tui::size();
        let bh = State::body_h(h);
        st.clamp(bh);
        if dirty {
            draw(&st, &p, a_name, b_name, &sum, w, h, bh)?;
            dirty = false;
        }
        let Some(ev) = tui::poll(Duration::from_millis(250))? else { continue };
        dirty = true;
        match ev {
            Event::Resize(..) => {}
            Event::Key(k) if k.kind != crossterm::event::KeyEventKind::Release => {
                if st.help {
                    st.help = false;
                    continue;
                }
                let page = bh.saturating_sub(2).max(1);
                match k.code {
                    KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => break,
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Char('j') | KeyCode::Down => {
                        st.top += 1;
                        st.cur = None;
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        st.top = st.top.saturating_sub(1);
                        st.cur = None;
                    }
                    KeyCode::Char(' ') | KeyCode::PageDown | KeyCode::Char('f') => {
                        st.top += page;
                        st.cur = None;
                    }
                    KeyCode::Char('b') | KeyCode::PageUp => {
                        st.top = st.top.saturating_sub(page);
                        st.cur = None;
                    }
                    KeyCode::Char('g') | KeyCode::Home => {
                        st.top = 0;
                        st.cur = None;
                    }
                    KeyCode::Char('G') | KeyCode::End => {
                        st.top = st.view.total();
                        st.cur = None;
                    }
                    KeyCode::Right | KeyCode::Char('l') => st.hoff += 8,
                    KeyCode::Left | KeyCode::Char('h') => st.hoff = st.hoff.saturating_sub(8),
                    KeyCode::Char('0') => st.hoff = 0,
                    KeyCode::Char('c') => {
                        st.fold = !st.fold;
                        st.rebuild();
                        if let Some(&c) = st.changes.first() {
                            st.top = c.saturating_sub(3);
                            st.cur = Some(0);
                        }
                    }
                    KeyCode::Char('n') => {
                        let next = match st.cur {
                            Some(i) => i + 1,
                            None => st.changes.partition_point(|&s| s < st.top),
                        };
                        if next < st.changes.len() {
                            st.cur = Some(next);
                            st.top = st.changes[next].saturating_sub(3);
                        }
                    }
                    KeyCode::Char('p') | KeyCode::Char('N') => {
                        let prev = match st.cur {
                            Some(i) => i.checked_sub(1),
                            None => st.changes.partition_point(|&s| s < st.top).checked_sub(1),
                        };
                        if let Some(pv) = prev {
                            st.cur = Some(pv);
                            st.top = st.changes[pv].saturating_sub(3);
                        }
                    }
                    KeyCode::Char('?') => st.help = true,
                    _ => {}
                }
            }
            Event::Mouse(m) => match m.kind {
                MouseEventKind::ScrollDown => {
                    st.top += 3;
                    st.cur = None;
                }
                MouseEventKind::ScrollUp => {
                    st.top = st.top.saturating_sub(3);
                    st.cur = None;
                }
                MouseEventKind::Down(MouseButton::Left) if m.column as usize == w - 1 && m.row >= 1 && (m.row as usize) <= bh => {
                    let frac = (m.row as usize - 1) as f64 / bh as f64;
                    st.top = ((st.view.total() as f64 * frac) as usize).saturating_sub(bh / 2);
                    st.cur = None;
                }
                _ => {}
            },
            _ => {}
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn draw(st: &State, p: &Painter, a_name: &str, b_name: &str, sum: &crate::diff::Stats, w: usize, h: usize, bh: usize) -> Result<()> {
    let t = &p.t;
    let body_w = w.saturating_sub(1);
    let lay = SideLayout::new(body_w, st.a.len().max(st.b.len()));
    let mut lines: Vec<String> = Vec::with_capacity(h);

    // header
    let half = lay.width() / 2;
    let hdr = format!(" {} ", fit(a_name, half.saturating_sub(2)));
    let hdr2 = format!(" {} ", fit(b_name, lay.width().saturating_sub(half + 2)));
    lines.push(format!("{}{}{}{}{}{}{}", p.bg(t.bar_bg), p.fg(t.del_strong), p.bold(), hdr, p.fg(t.add_strong), hdr2, p.reset()));

    if st.help {
        let help = [
            "  kempt diff — keys",
            "",
            "  n / p        next / previous difference",
            "  j k ↑ ↓      scroll line      space / b   page down / up",
            "  g G          top / bottom     ← → h l     scroll sideways (0 resets)",
            "  c            toggle: changes-only (with context) ⇄ full file",
            "  mouse        wheel scrolls; click the right-edge ruler to jump",
            "  q / Esc      quit             any key     close this help",
            "",
            "  colours:  green = only in right    red = only in left",
            "            ~ yellow gutter = changed line; bold = exact changed characters",
        ];
        for i in 0..bh {
            let s = help.get(i).copied().unwrap_or("");
            lines.push(format!("{}{}", p.fg(t.fg), fit(s, w)));
        }
    } else {
        let ruler = st.view.ruler(bh);
        let total = st.view.total().max(1);
        let th0 = st.top * bh / total;
        let th1 = ((st.top + bh).min(total) * bh / total).max(th0 + 1).min(bh);
        for i in 0..bh {
            let r = st.top + i;
            let mut s = if r < st.view.total() {
                let row = st.view.row(r);
                render_side_row(p, &row, st.a, st.b, &lay, st.hoff)
            } else {
                format!("{}~{}", p.fg(t.dim_fg), p.reset())
            };
            // pad row to body width then ruler
            let visible = if r < st.view.total() { lay.width() } else { 1 };
            for _ in visible..body_w {
                s.push(' ');
            }
            let in_thumb = i >= th0 && i < th1;
            let (ch, col) = match ruler[i] {
                1 => ('█', Some(t.add_strong)),
                2 => ('█', Some(t.del_strong)),
                3 => ('█', Some(t.chg_gutter)),
                _ => (if in_thumb { '│' } else { ' ' }, Some(t.dim_fg)),
            };
            s.push_str(p.reset());
            if in_thumb {
                s.push_str(&p.bg(t.bar_bg));
            }
            if let Some(c) = col {
                s.push_str(&p.fg(c));
            }
            s.push(ch);
            lines.push(s);
        }
    }

    // status bar
    let pos = format!("row {}/{}", (st.top + 1).min(st.view.total()), st.view.total());
    let dpos = match st.cur {
        Some(i) => format!("diff {}/{}", i + 1, st.changes.len()),
        None => format!("{} diffs", st.changes.len()),
    };
    let left = format!(
        " {}+{}{} {}-{}{} {}~{}{}{}  │ {} │ {} │ {} ",
        p.fg(t.add_strong), sum.added, p.fg(t.bar_fg), p.fg(t.del_strong), sum.removed, p.fg(t.bar_fg), p.fg(t.chg_gutter), sum.changed, p.fg(t.bar_fg), "",
        pos, dpos, if st.fold { "changes only" } else { "full file" }
    );
    let hint = " n/p diff · c fold · ←→ scroll · ? help · q quit ";
    let plain_len = strip_len(&left);
    let pad = w.saturating_sub(plain_len + hint.chars().count());
    lines.push(format!("{}{}{}{}{}{}{}", p.bg(t.bar_bg), p.fg(t.bar_fg), left, " ".repeat(pad), p.fg(t.dim_fg), hint, p.reset()));
    tui::draw(&lines)
}

/// visible length of a string containing SGR escapes
fn strip_len(s: &str) -> usize {
    let mut n = 0;
    let mut in_esc = false;
    for c in s.chars() {
        if in_esc {
            if c == 'm' {
                in_esc = false;
            }
        } else if c == '\x1b' {
            in_esc = true;
        } else {
            n += 1;
        }
    }
    n
}
