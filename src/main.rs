mod diff;
mod diffcmd;
mod diffview;
mod docs;
mod external;
mod fmtcmd;
mod index;
mod jsonfix;
mod plat;
mod jsonfmt;
mod render;
mod search;
mod service;
mod server;
mod sdiff;
mod source;
mod state;
mod table;
mod xlsx;
mod tsort;
mod theme;
mod tui;
mod viewer;
mod xmlfmt;

use clap::{Parser, Subcommand};
use std::io::IsTerminal;
use std::path::PathBuf;

/// `--indent` accepts 1-8 (spaces) or `tab`
fn parse_indent(s: &str) -> Result<usize, String> {
    match s {
        "tab" | "tabs" => Ok(jsonfmt::INDENT_TAB),
        n => n.parse::<usize>().ok().filter(|v| (1..=8).contains(v)).ok_or_else(|| "indent must be 1-8 or 'tab'".to_string()),
    }
}

#[derive(Parser)]
#[command(name = "kempt", version, about = "Format, view and compare JSON, XML, logs and source files — built for big files")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Format files in place (JSON/JSONL/XML streamed in constant memory; others via prettier/ruff/shfmt/...)
    #[command(alias = "fmt")]
    Format {
        /// files or directories ('-' reads stdin; needs --lang)
        #[arg(required = true)]
        paths: Vec<String>,
        /// don't write; exit 1 if any file would change
        #[arg(short, long)]
        check: bool,
        /// compact output (JSON / XML only)
        #[arg(long)]
        minify: bool,
        /// JSON only: turn JSON stored inside string values into real, indented JSON
        #[arg(long)]
        expand_embedded: bool,
        /// indent: 1-8 spaces, or 'tab'
        #[arg(long, default_value = "2", value_parser = parse_indent)]
        indent: usize,
        /// write result here instead of in place ('-' = stdout); single input only
        #[arg(short, long)]
        output: Option<String>,
        /// largest file (MB) handed to non-streaming external formatters
        #[arg(long, default_value_t = 20)]
        max_size: u64,
        /// ignore --max-size
        #[arg(long)]
        force: bool,
        /// language for stdin (json, xml, python, ...)
        #[arg(long)]
        lang: Option<String>,
        /// only print errors
        #[arg(short, long)]
        quiet: bool,
    },
    /// Open a file in the viewer (formats JSON/XML on open; handles huge logs). Pipes get plain text.
    #[command(alias = "open")]
    View {
        path: PathBuf,
        /// show the file as-is, don't pretty-print JSON/XML
        #[arg(long)]
        raw: bool,
        /// follow the file as it grows (implies --raw)
        #[arg(short = 'f', long)]
        follow: bool,
        /// start at this line
        #[arg(short = 'n', long)]
        line: Option<usize>,
        /// start with this regex filter (or filter piped output)
        #[arg(short = 'F', long)]
        filter: Option<String>,
        #[arg(long, default_value = "2", value_parser = parse_indent)]
        indent: usize,
        /// view even if the file looks binary
        #[arg(long)]
        force: bool,
        #[arg(long, default_value = "dark")]
        theme: theme::ThemeKind,
        #[arg(long)]
        no_color: bool,
        /// never start the interactive UI; print to stdout
        #[arg(long)]
        plain: bool,
    },
    /// Compare two files with colour-highlighted differences (exit 0 = same, 1 = differ)
    Diff {
        a: PathBuf,
        b: PathBuf,
        /// auto | text | json (structural) | format (format both, then diff lines)
        #[arg(short, long, default_value = "auto")]
        mode: diffcmd::Mode,
        /// ignore all whitespace
        #[arg(short = 'w', long)]
        ignore_space: bool,
        /// ignore case
        #[arg(short = 'i', long)]
        ignore_case: bool,
        /// mask timestamps, UUIDs, IPs, hex ids and long numbers before comparing (for logs)
        #[arg(long)]
        mask: bool,
        /// lines of context
        #[arg(short = 'U', long, default_value_t = 3)]
        context: usize,
        /// side-by-side output
        #[arg(short = 'y', long)]
        side_by_side: bool,
        /// output width for side-by-side
        #[arg(long)]
        width: Option<usize>,
        /// only print the summary
        #[arg(long)]
        stat: bool,
        /// write a unified patch to this file
        #[arg(long)]
        patch: Option<PathBuf>,
        /// write a colour HTML report to this file
        #[arg(long)]
        html: Option<PathBuf>,
        #[arg(long, default_value = "dark")]
        theme: theme::ThemeKind,
        #[arg(long)]
        no_color: bool,
        /// emit colours even when stdout is not a terminal
        #[arg(long)]
        color: bool,
        /// never start the interactive UI
        #[arg(long)]
        plain: bool,
        /// force the interactive UI
        #[arg(long)]
        tui: bool,
        /// structural JSON diff limit (MB per file; needs ~30x that in RAM); bigger files use streamed format-then-diff
        #[arg(long, default_value_t = 40)]
        struct_limit: u64,
        /// give up refining the diff after N seconds (0 = never)
        #[arg(long, default_value_t = 60)]
        timeout: u64,
        #[arg(long, default_value = "2", value_parser = parse_indent)]
        indent: usize,
        #[arg(short, long)]
        quiet: bool,
    },
    /// Open the graphical interface in your browser (find/replace, Fix, side-by-side compare with copy buttons, themes).
    /// One file opens it; two files open a comparison.
    Gui {
        files: Vec<PathBuf>,
        /// port to listen on (default: any free port); the server only listens on 127.0.0.1
        #[arg(long, default_value_t = 0)]
        port: u16,
        /// print the URL but don't launch a browser
        #[arg(long)]
        no_open: bool,
        /// with two files, open them as two tabs instead of a comparison
        #[arg(long)]
        tabs: bool,
        /// run quietly with no window (reminders, notifications and the tray icon keep working); this is what start-at-login uses
        #[arg(long)]
        background: bool,
        /// with --background: return to the terminal at once and keep running on its own
        #[arg(long)]
        detach: bool,
        /// open straight to a tool: `notes` (checklist & reminders) or `clock`
        #[arg(long, value_parser = ["notes", "clock"])]
        view: Option<String>,
        /// open in its own app window (needs a Chromium-based browser) instead of a browser tab
        #[arg(long)]
        app: bool,
    },
    /// Start Kempt automatically when you log in (background mode: reminders and tray icon). `on`, `off` or `status`.
    Autostart {
        #[arg(value_parser = ["on", "off", "status"])]
        action: String,
    },
    /// Stop the Kempt that is running in the background
    Stop,
    /// Check which formatter tools are installed
    Doctor,
}

fn main() {
    plat::enable_ansi();
    let code = match real_main() {
        Ok(c) => c,
        Err(e) => {
            // a closed pipe (e.g. `| head`) is not an error
            // on Windows a closed pipe can also surface as ERROR_NO_DATA (232) or ERROR_PIPE_NOT_CONNECTED (233)
            let closed = |io: &std::io::Error| io.kind() == std::io::ErrorKind::BrokenPipe || (cfg!(windows) && matches!(io.raw_os_error(), Some(109 | 232 | 233)));
            if e.chain().any(|c| c.downcast_ref::<std::io::Error>().is_some_and(closed)) {
                std::process::exit(0);
            }
            eprintln!("kempt: {e:#}");
            2
        }
    };
    std::process::exit(code);
}

fn real_main() -> anyhow::Result<i32> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Format { paths, check, minify, expand_embedded, indent, output, max_size, force, lang, quiet } => {
            jsonfmt::set_expand(expand_embedded);
            fmtcmd::run(paths, fmtcmd::FmtOpts { check, minify, indent, output, max_size_mb: max_size, force, lang, quiet })
        }
        Cmd::View { path, raw, follow, line, filter, indent, force, theme, no_color, plain } => {
            let mode = theme::detect_mode(no_color, false, std::io::stdout().is_terminal());
            // the TUI always paints; when colours are off, fall back to ANSI16-less plain styling
            let painter = theme::Painter::new(if mode == theme::ColorMode::None && std::io::stdout().is_terminal() && !no_color { theme::ColorMode::Ansi16 } else { mode }, theme);
            viewer::run(viewer::ViewArgs { path, raw, follow, line, filter, indent, force, painter, plain })?;
            Ok(0)
        }
        Cmd::Diff { a, b, mode, ignore_space, ignore_case, mask, context, side_by_side, width, stat, patch, html, theme, no_color, color, plain, tui, struct_limit, timeout, indent, quiet } => {
            diffcmd::run(diffcmd::DiffArgs {
                a,
                b,
                mode,
                ignore_ws: ignore_space,
                ignore_case,
                mask,
                context,
                side_by_side,
                width,
                stat,
                patch,
                html,
                theme,
                no_color,
                force_color: color,
                plain,
                tui,
                struct_limit_mb: struct_limit,
                timeout_secs: timeout,
                indent,
                quiet,
            })
        }
        Cmd::Gui { files, port, no_open, tabs, background, detach, view, app } => {
            if detach {
                if service::running_instance().is_none() {
                    service::spawn_detached()?;
                }
                return Ok(0);
            }
            server::run(server::GuiOpts { files, port, no_open, as_tabs: tabs, background, app_window: app, view })?;
            Ok(0)
        }
        Cmd::Autostart { action } => {
            match action.as_str() {
                "on" => {
                    service::set_autostart(true)?;
                    println!("Kempt will start in the background when you log in (reminders and the tray icon keep working). Undo with: kempt autostart off");
                }
                "off" => {
                    service::set_autostart(false)?;
                    println!("Kempt will no longer start at login.");
                }
                _ => println!("start at login: {}", if service::autostart_on() { "on" } else { "off" }),
            }
            Ok(0)
        }
        Cmd::Stop => {
            if service::stop_running()? {
                println!("Kempt stopped.");
            } else {
                println!("Kempt is not running in the background.");
            }
            Ok(0)
        }
        Cmd::Doctor => Ok(external::doctor()),
    }
}
