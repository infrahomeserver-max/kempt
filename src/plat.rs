//! Small platform layer: everything that differs between Linux, macOS and Windows lives here.

use std::path::{Path, PathBuf};

/// The user's home directory (`$HOME`, or `%USERPROFILE%` on Windows).
pub fn home_dir() -> PathBuf {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var)
        .or_else(|| std::env::var_os("HOME"))
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(if cfg!(windows) { "C:\\" } else { "/" }))
}

/// `std::fs::canonicalize`, but without the `\\?\` prefix Windows adds (it makes paths ugly and breaks comparisons).
pub fn canonical(p: &Path) -> std::io::Result<PathBuf> {
    std::fs::canonicalize(p).map(strip_verbatim)
}

pub fn strip_verbatim(p: PathBuf) -> PathBuf {
    if cfg!(windows) {
        let s = p.to_string_lossy();
        if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{rest}"));
        }
        if let Some(rest) = s.strip_prefix(r"\\?\") {
            // only plain drive paths (C:\...) are safe to simplify
            if rest.len() >= 2 && rest.as_bytes()[1] == b':' {
                return PathBuf::from(rest);
            }
        }
    }
    p
}

/// Cryptographically secure random bytes from the operating system.
pub fn random_bytes(buf: &mut [u8]) {
    if getrandom::fill(buf).is_err() {
        // extremely unlikely; never fall back to a predictable token
        eprintln!("kempt: the operating system could not provide random numbers; refusing to start the server");
        std::process::exit(2);
    }
}

/// Identity of a file for log-rotation detection (inode on Unix; not available elsewhere).
pub fn file_id(path: &Path) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(path).map(|m| m.ino()).unwrap_or(0)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        0
    }
}

/// Open `url` in the default browser. Best effort.
pub fn open_browser(url: &str) {
    use std::process::{Command, Stdio};
    let quiet = |mut c: Command| c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().is_ok();
    if cfg!(windows) {
        let mut c = Command::new("rundll32");
        c.args(["url.dll,FileProtocolHandler", url]);
        quiet(c);
    } else if cfg!(target_os = "macos") {
        let mut c = Command::new("open");
        c.arg(url);
        quiet(c);
    } else {
        for cmd in ["xdg-open", "sensible-browser", "gio", "x-www-browser"] {
            if crate::external::find_bin(cmd).is_some() {
                let mut c = Command::new(cmd);
                if cmd == "gio" {
                    c.arg("open");
                }
                c.arg(url);
                if quiet(c) {
                    return;
                }
            }
        }
    }
}

/// Make ANSI colour escapes work in the Windows console (no-op elsewhere).
pub fn enable_ansi() {
    #[cfg(windows)]
    {
        let _ = crossterm::ansi_support::supports_ansi();
    }
}

/// Is this a terminal that understands 24-bit colour without further hints?
pub fn truecolor_hint() -> bool {
    cfg!(windows) && (std::env::var_os("WT_SESSION").is_some() || std::env::var_os("TERM_PROGRAM").is_some())
        || std::env::var("TERM_PROGRAM").is_ok_and(|v| v == "iTerm.app" || v == "WezTerm" || v == "vscode")
}

/// Where Kempt keeps its small settings file. `KEMPT_CONFIG_DIR` overrides it (used by the tests).
pub fn config_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("KEMPT_CONFIG_DIR").filter(|v| !v.is_empty()) {
        return PathBuf::from(d);
    }
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(|| home_dir().join("AppData").join("Roaming"))
    } else if cfg!(target_os = "macos") {
        home_dir().join("Library").join("Application Support")
    } else {
        std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(|| home_dir().join(".config"))
    };
    base.join("kempt")
}

/// Give freed heap memory back to the OS after a big temporary computation (sort, duplicate scan).
/// glibc otherwise keeps it in per-thread arenas, so repeated sorts of a huge table would keep growing the process.
pub fn trim_memory() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        unsafe extern "C" {
            fn malloc_trim(pad: usize) -> i32;
        }
        // SAFETY: plain libc call with no pointers; it only releases free memory.
        unsafe {
            malloc_trim(0);
        }
    }
}
