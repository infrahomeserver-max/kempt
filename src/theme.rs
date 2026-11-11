//! Colour theme + terminal colour capability handling.

use std::io::IsTerminal;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rgb(pub u8, pub u8, pub u8);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColorMode {
    None,
    Ansi16,
    Ansi256,
    True,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeKind {
    Dark,
    Light,
    Colorblind,
}

impl std::str::FromStr for ThemeKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "dark" | "auto" => Ok(ThemeKind::Dark),
            "light" => Ok(ThemeKind::Light),
            "colorblind" | "cb" => Ok(ThemeKind::Colorblind),
            _ => Err(format!("unknown theme '{s}' (dark, light, colorblind)")),
        }
    }
}

#[derive(Clone, Copy)]
pub struct Theme {
    pub add_bg: Rgb,
    pub add_strong: Rgb,
    pub del_bg: Rgb,
    pub del_strong: Rgb,
    pub chg_gutter: Rgb,
    pub moved_fg: Rgb,
    pub dim_fg: Rgb,
    pub fg: Rgb,
    pub match_bg: Rgb,
    pub sel_bg: Rgb,
    pub bar_bg: Rgb,
    pub bar_fg: Rgb,
    pub lvl_error: Rgb,
    pub lvl_warn: Rgb,
    pub lvl_info: Rgb,
    pub lvl_debug: Rgb,
    pub ts_fg: Rgb,
    pub fold_fg: Rgb,
}

impl Theme {
    pub fn new(k: ThemeKind) -> Theme {
        match k {
            ThemeKind::Dark => Theme {
                add_bg: Rgb(18, 58, 30),
                add_strong: Rgb(34, 134, 58),
                del_bg: Rgb(72, 24, 30),
                del_strong: Rgb(168, 42, 54),
                chg_gutter: Rgb(230, 180, 40),
                moved_fg: Rgb(80, 200, 220),
                dim_fg: Rgb(128, 128, 128),
                fg: Rgb(220, 220, 220),
                match_bg: Rgb(190, 60, 190),
                sel_bg: Rgb(48, 52, 70),
                bar_bg: Rgb(40, 44, 60),
                bar_fg: Rgb(210, 210, 220),
                lvl_error: Rgb(255, 95, 95),
                lvl_warn: Rgb(240, 200, 70),
                lvl_info: Rgb(120, 200, 120),
                lvl_debug: Rgb(130, 130, 150),
                ts_fg: Rgb(110, 170, 220),
                fold_fg: Rgb(110, 110, 140),
            },
            ThemeKind::Light => Theme {
                add_bg: Rgb(214, 245, 220),
                add_strong: Rgb(120, 220, 140),
                del_bg: Rgb(255, 222, 224),
                del_strong: Rgb(250, 150, 156),
                chg_gutter: Rgb(190, 130, 0),
                moved_fg: Rgb(0, 130, 160),
                dim_fg: Rgb(130, 130, 130),
                fg: Rgb(30, 30, 30),
                match_bg: Rgb(255, 150, 255),
                sel_bg: Rgb(220, 226, 245),
                bar_bg: Rgb(210, 214, 230),
                bar_fg: Rgb(30, 30, 50),
                lvl_error: Rgb(190, 0, 0),
                lvl_warn: Rgb(160, 110, 0),
                lvl_info: Rgb(0, 120, 0),
                lvl_debug: Rgb(110, 110, 130),
                ts_fg: Rgb(0, 80, 160),
                fold_fg: Rgb(120, 120, 160),
            },
            // blue = added, orange = removed (distinguishable in red-green colour blindness)
            ThemeKind::Colorblind => Theme {
                add_bg: Rgb(16, 48, 88),
                add_strong: Rgb(40, 112, 204),
                del_bg: Rgb(84, 50, 10),
                del_strong: Rgb(214, 122, 20),
                chg_gutter: Rgb(240, 228, 66),
                moved_fg: Rgb(204, 121, 167),
                dim_fg: Rgb(128, 128, 128),
                fg: Rgb(220, 220, 220),
                match_bg: Rgb(200, 70, 200),
                sel_bg: Rgb(48, 52, 70),
                bar_bg: Rgb(40, 44, 60),
                bar_fg: Rgb(210, 210, 220),
                lvl_error: Rgb(230, 159, 0),
                lvl_warn: Rgb(240, 228, 66),
                lvl_info: Rgb(86, 180, 233),
                lvl_debug: Rgb(130, 130, 150),
                ts_fg: Rgb(86, 180, 233),
                fold_fg: Rgb(110, 110, 140),
            },
        }
    }
}

pub fn detect_mode(no_color_flag: bool, force: bool, stream_is_tty: bool) -> ColorMode {
    if no_color_flag || std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()) {
        return ColorMode::None;
    }
    let term = std::env::var("TERM").unwrap_or_default();
    if term == "dumb" {
        return ColorMode::None;
    }
    if !stream_is_tty && !force {
        return ColorMode::None;
    }
    let ct = std::env::var("COLORTERM").unwrap_or_default();
    if ct == "truecolor" || ct == "24bit" || crate::plat::truecolor_hint() {
        return ColorMode::True;
    }
    if term.contains("256") || term.contains("truecolor") || term.contains("direct") {
        return ColorMode::Ansi256;
    }
    if term.is_empty() && !stream_is_tty {
        return ColorMode::Ansi256;
    }
    ColorMode::Ansi16
}

pub fn stdout_mode(no_color: bool, force: bool) -> ColorMode {
    detect_mode(no_color, force, std::io::stdout().is_terminal())
}

fn to_256(c: Rgb) -> u8 {
    let Rgb(r, g, b) = c;
    // grayscale ramp
    if r.abs_diff(g) < 10 && g.abs_diff(b) < 10 {
        let v = r as u32;
        if v < 8 {
            return 16;
        }
        if v > 248 {
            return 231;
        }
        return 232 + ((v - 8) * 24 / 247) as u8;
    }
    let q = |v: u8| -> u8 {
        if v < 48 {
            0
        } else if v < 115 {
            1
        } else {
            ((v as u32 - 35) / 40) as u8
        }
    };
    16 + 36 * q(r) + 6 * q(g) + q(b)
}

fn lum(c: Rgb) -> u32 {
    (c.0 as u32 * 299 + c.1 as u32 * 587 + c.2 as u32 * 114) / 1000
}

/// Nearest of the basic ANSI hues, ignoring brightness (so dark tints keep their hue).
fn to_16(c: Rgb) -> u8 {
    let (r, g, b) = (c.0 as f64, c.1 as f64, c.2 as f64);
    let mx = r.max(g).max(b);
    let mn = r.min(g).min(b);
    if mx - mn < 24.0 {
        return if lum(c) < 60 { 0 } else { 7 }; // grey: black / white
    }
    let d = mx - mn;
    let mut h = if mx == r {
        60.0 * (((g - b) / d) % 6.0)
    } else if mx == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    if h < 0.0 {
        h += 360.0;
    }
    // red, yellow, green, cyan, blue, magenta
    const CODES: [u8; 6] = [1, 3, 2, 6, 4, 5];
    CODES[((h / 60.0).round() as usize) % 6]
}

#[derive(Clone, Copy)]
pub struct Painter {
    pub mode: ColorMode,
    pub t: Theme,
}

pub const RESET: &str = "\x1b[0m";

impl Painter {
    pub fn new(mode: ColorMode, kind: ThemeKind) -> Self {
        Painter { mode, t: Theme::new(kind) }
    }

    pub fn on(&self) -> bool {
        self.mode != ColorMode::None
    }

    pub fn fg(&self, c: Rgb) -> String {
        match self.mode {
            ColorMode::None => String::new(),
            ColorMode::True => format!("\x1b[38;2;{};{};{}m", c.0, c.1, c.2),
            ColorMode::Ansi256 => format!("\x1b[38;5;{}m", to_256(c)),
            ColorMode::Ansi16 => format!("\x1b[{}m", 30 + to_16(c)),
        }
    }

    pub fn bg(&self, c: Rgb) -> String {
        match self.mode {
            ColorMode::None => String::new(),
            ColorMode::True => format!("\x1b[48;2;{};{};{}m", c.0, c.1, c.2),
            ColorMode::Ansi256 => format!("\x1b[48;5;{}m", to_256(c)),
            // 16 colours can't express subtle tints: line tints get no background (see tint_fg)
            ColorMode::Ansi16 if self.is_tint(c) => String::new(),
            ColorMode::Ansi16 => format!("\x1b[{}m", 40 + to_16(c)),
        }
    }

    fn is_tint(&self, c: Rgb) -> bool {
        c == self.t.add_bg || c == self.t.del_bg
    }

    /// Foreground that stands in for a line-tint background on 16-colour terminals.
    pub fn tint_fg(&self, tint: Rgb) -> String {
        if self.mode == ColorMode::Ansi16 && self.is_tint(tint) {
            format!("\x1b[{}m", 30 + to_16(tint))
        } else {
            String::new()
        }
    }

    pub fn bold(&self) -> &'static str {
        if self.on() { "\x1b[1m" } else { "" }
    }

    pub fn reset(&self) -> &'static str {
        if self.on() { RESET } else { "" }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn none_mode_emits_nothing() {
        let p = Painter::new(ColorMode::None, ThemeKind::Dark);
        assert_eq!(p.bg(Rgb(1, 2, 3)), "");
        assert_eq!(p.fg(Rgb(1, 2, 3)), "");
        assert_eq!(p.reset(), "");
        assert_eq!(p.bold(), "");
    }

    #[test]
    fn truecolor_sequences() {
        let p = Painter::new(ColorMode::True, ThemeKind::Dark);
        assert_eq!(p.bg(Rgb(1, 2, 3)), "\x1b[48;2;1;2;3m");
        assert_eq!(p.fg(Rgb(1, 2, 3)), "\x1b[38;2;1;2;3m");
    }

    #[test]
    fn fallbacks_in_range() {
        for c in [Rgb(168, 42, 54), Rgb(34, 134, 58), Rgb(128, 128, 128), Rgb(0, 0, 0), Rgb(255, 255, 255)] {
            let p = Painter::new(ColorMode::Ansi256, ThemeKind::Dark);
            assert!(p.bg(c).starts_with("\x1b[48;5;"));
            let p = Painter::new(ColorMode::Ansi16, ThemeKind::Dark);
            let s = p.bg(c);
            assert!(s.is_empty() || (s.starts_with("\x1b[4") && s.len() == 5), "{s}");
        }
        // red-ish maps to red(1), green-ish to green(2)
        assert_eq!(to_16(Rgb(168, 42, 54)), 1);
        assert_eq!(to_16(Rgb(34, 134, 58)), 2);
        // dark tints keep their hue and are rendered as coloured text instead of a background
        assert_eq!(to_16(Rgb(72, 24, 30)), 1);
        assert_eq!(to_16(Rgb(18, 58, 30)), 2);
        let p = Painter::new(ColorMode::Ansi16, ThemeKind::Dark);
        assert_eq!(p.bg(Rgb(72, 24, 30)), "");
        assert_eq!(p.tint_fg(Rgb(72, 24, 30)), "\x1b[31m");
        assert_eq!(p.tint_fg(Rgb(34, 134, 58)), "");
    }

    #[test]
    fn theme_parse() {
        assert_eq!("colorblind".parse::<ThemeKind>().unwrap(), ThemeKind::Colorblind);
        assert!("nope".parse::<ThemeKind>().is_err());
    }
}
