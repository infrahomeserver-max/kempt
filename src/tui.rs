//! Minimal terminal plumbing: raw mode / alt screen guard, frame output, prompt.

use anyhow::Result;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::{cursor, event, execute, terminal};
use std::io::{Write, stdout};
use std::time::Duration;

pub struct Term;

impl Term {
    pub fn enter() -> Result<Term> {
        terminal::enable_raw_mode()?;
        execute!(stdout(), terminal::EnterAlternateScreen, cursor::Hide, event::EnableMouseCapture)?;
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            restore();
            prev(info);
        }));
        Ok(Term)
    }
}

fn restore() {
    let _ = execute!(stdout(), event::DisableMouseCapture, cursor::Show, terminal::LeaveAlternateScreen);
    let _ = terminal::disable_raw_mode();
}

impl Drop for Term {
    fn drop(&mut self) {
        restore();
    }
}

pub fn size() -> (usize, usize) {
    terminal::size().map(|(w, h)| (w as usize, h as usize)).unwrap_or((120, 40))
}

/// Draw a frame: `lines` are already styled; each is padded/cleared to the full width by the terminal.
pub fn draw(lines: &[String]) -> Result<()> {
    let mut o = String::with_capacity(lines.iter().map(|l| l.len() + 8).sum::<usize>() + 16);
    o.push_str("\x1b[H");
    for (i, l) in lines.iter().enumerate() {
        o.push_str(l);
        o.push_str("\x1b[0m\x1b[K");
        if i + 1 < lines.len() {
            o.push_str("\r\n");
        }
    }
    let mut out = stdout().lock();
    out.write_all(o.as_bytes())?;
    out.flush()?;
    Ok(())
}

pub fn poll(timeout: Duration) -> Result<Option<Event>> {
    if event::poll(timeout)? { Ok(Some(event::read()?)) } else { Ok(None) }
}

/// One-line text prompt.
pub struct Prompt {
    pub label: String,
    pub text: String,
}

pub enum PromptResult {
    Pending,
    Submit(String),
    Cancel,
}

impl Prompt {
    pub fn new(label: &str, initial: &str) -> Prompt {
        Prompt { label: label.to_string(), text: initial.to_string() }
    }
    pub fn key(&mut self, k: &KeyEvent) -> PromptResult {
        match k.code {
            KeyCode::Enter => PromptResult::Submit(self.text.clone()),
            KeyCode::Esc => PromptResult::Cancel,
            KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => PromptResult::Cancel,
            KeyCode::Char('u') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                self.text.clear();
                PromptResult::Pending
            }
            KeyCode::Backspace => {
                self.text.pop();
                PromptResult::Pending
            }
            KeyCode::Char(c) => {
                self.text.push(c);
                PromptResult::Pending
            }
            _ => PromptResult::Pending,
        }
    }
}

/// Pad (by display chars) or clip a plain string to exactly `w` chars.
pub fn fit(s: &str, w: usize) -> String {
    let mut o: String = s.chars().take(w).collect();
    let n = o.chars().count();
    for _ in n..w {
        o.push(' ');
    }
    o
}
