//! Formatting via external, best-in-class tools for languages that can't be
//! streamed safely (HTML, CSS, JS/TS, YAML, Markdown, Python, shell, SQL, C-family).

use anyhow::{Result, bail};
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub struct ExtTool {
    pub name: &'static str,
    pub bin: &'static str,
    pub install: &'static str,
    pub langs: &'static str,
}

pub const PRETTIER: ExtTool = ExtTool { name: "prettier", bin: "prettier", install: "npm i -g prettier", langs: "HTML, CSS, SCSS, Less, JS/TS, JSX/TSX, Vue, YAML, Markdown, MDX, GraphQL, Handlebars, JSON5" };
pub const RUFF: ExtTool = ExtTool { name: "ruff", bin: "ruff", install: if cfg!(windows) { "pip install ruff   (or: winget install astral-sh.ruff)" } else if cfg!(target_os = "macos") { "brew install ruff" } else { "pipx install ruff   (or: pip install ruff)" }, langs: "Python" };
pub const SHFMT: ExtTool = ExtTool { name: "shfmt", bin: "shfmt", install: if cfg!(windows) { "winget install mvdan.shfmt   (or: scoop install shfmt)" } else if cfg!(target_os = "macos") { "brew install shfmt" } else { "sudo apt install shfmt" }, langs: "shell" };
pub const SQLFORMAT: ExtTool = ExtTool { name: "sqlformat", bin: "sqlformat", install: if cfg!(windows) || cfg!(target_os = "macos") { "pip install sqlparse" } else { "pipx install sqlparse" }, langs: "SQL" };
pub const CLANG: ExtTool = ExtTool { name: "clang-format", bin: "clang-format", install: if cfg!(windows) { "winget install LLVM.LLVM   (includes clang-format)" } else if cfg!(target_os = "macos") { "brew install clang-format" } else { "sudo apt install clang-format" }, langs: "C, C++, C#, Java, Objective-C, Protobuf, CUDA" };

pub const GOFMT: ExtTool = ExtTool { name: "gofmt", bin: "gofmt", install: if cfg!(windows) { "winget install GoLang.Go   (includes gofmt)" } else if cfg!(target_os = "macos") { "brew install go" } else { "sudo apt install golang-go" }, langs: "Go" };
pub const RUSTFMT: ExtTool = ExtTool { name: "rustfmt", bin: "rustfmt", install: "rustup component add rustfmt", langs: "Rust" };
pub const TAPLO: ExtTool = ExtTool { name: "taplo", bin: "taplo", install: "cargo install taplo-cli   (or: brew install taplo)", langs: "TOML" };
pub const STYLUA: ExtTool = ExtTool { name: "stylua", bin: "stylua", install: "cargo install stylua   (or: brew install stylua)", langs: "Lua" };
pub const TERRAFORM: ExtTool = ExtTool { name: "terraform", bin: "terraform", install: if cfg!(windows) { "winget install Hashicorp.Terraform" } else if cfg!(target_os = "macos") { "brew install terraform" } else { "https://developer.hashicorp.com/terraform/install" }, langs: "Terraform" };

pub const ALL: [&ExtTool; 10] = [&PRETTIER, &RUFF, &SHFMT, &SQLFORMAT, &CLANG, &GOFMT, &RUSTFMT, &TAPLO, &STYLUA, &TERRAFORM];

pub fn tool_for_ext(ext: &str) -> Option<&'static ExtTool> {
    match ext {
        "html" | "htm" | "css" | "scss" | "less" | "js" | "mjs" | "cjs" | "jsx" | "ts" | "tsx" | "vue" | "yaml" | "yml" | "md" | "markdown" | "graphql" | "gql" | "json5" | "mdx" | "hbs" | "handlebars" | "mts" | "cts" => Some(&PRETTIER),
        "py" | "pyi" => Some(&RUFF),
        "sh" | "bash" | "ksh" => Some(&SHFMT),
        "sql" => Some(&SQLFORMAT),
        "c" | "h" | "cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" | "java" | "cs" | "proto" | "m" | "mm" | "cu" | "cuh" | "ino" => Some(&CLANG),
        "go" => Some(&GOFMT),
        "rs" => Some(&RUSTFMT),
        "toml" => Some(&TAPLO),
        "lua" => Some(&STYLUA),
        "tf" | "tfvars" => Some(&TERRAFORM),
        _ => None,
    }
}

/// Look for a helper tool on PATH, then in kempt's private tools directory
/// (`<prefix>/share/kempt/tools` on Linux/macOS, `<install dir>\tools` on Windows; filled by the installer's
/// `--with-tools` option), so that removing kempt also removes everything it installed.
pub fn find_bin(bin: &str) -> Option<PathBuf> {
    let exts: &[&str] = if cfg!(windows) { &["", ".exe", ".cmd", ".bat", ".com"] } else { &[""] };
    let probe = |dir: &std::path::Path| -> Option<PathBuf> {
        for e in exts {
            let p = dir.join(format!("{bin}{e}"));
            if p.is_file() {
                return Some(p);
            }
        }
        None
    };
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            if let Some(p) = probe(&dir) {
                return Some(p);
            }
        }
    }
    let exe = crate::plat::canonical(&std::env::current_exe().ok()?).ok()?;
    let here = exe.parent()?;
    let tools = if cfg!(windows) { here.join("tools") } else { here.parent()?.join("share/kempt/tools") };
    let subs: &[&str] = if cfg!(windows) { &["bin", "node_modules/.bin", "venv/Scripts", "."] } else { &["bin", "node_modules/.bin"] };
    for sub in subs {
        if let Some(p) = probe(&tools.join(sub)) {
            return Some(p);
        }
    }
    None
}

/// Run `tool` on stdin, returning formatted bytes.
pub fn run(tool: &ExtTool, filename: &str, indent: usize, stdin: Stdio) -> Result<Vec<u8>> {
    let Some(bin) = find_bin(tool.bin) else {
        bail!("'{}' is not installed (needed for {}). Install: {}", tool.bin, tool.langs, tool.install);
    };
    let mut cmd = Command::new(bin);
    match tool.name {
        "prettier" => {
            cmd.args(["--stdin-filepath", filename]);
            if indent == crate::jsonfmt::INDENT_TAB { cmd.arg("--use-tabs"); } else { cmd.args(["--tab-width", &indent.to_string()]); }
        }
        "ruff" => {
            cmd.args(["format", "--stdin-filename", filename, "-"]);
        }
        "shfmt" => {
            cmd.args(["-i", &(if indent == crate::jsonfmt::INDENT_TAB { 0 } else { indent }).to_string()]); // shfmt: -i 0 = tabs
        }
        "sqlformat" => {
            cmd.args(["--reindent", "--keywords", "upper", "--indent_width", &(if indent == crate::jsonfmt::INDENT_TAB { 4 } else { indent }).to_string(), "-"]);
        }
        "clang-format" => {
            cmd.args(["--assume-filename", filename]);
        }
        "rustfmt" => {
            cmd.args(["--edition", "2021", "--emit", "stdout"]);
        }
        "taplo" => {
            cmd.args(["format", "-"]);
        }
        "stylua" => {
            cmd.args(["--indent-type", if indent == crate::jsonfmt::INDENT_TAB { "Tabs" } else { "Spaces" }, "--indent-width", &(if indent == crate::jsonfmt::INDENT_TAB { 4 } else { indent }).to_string(), "-"]);
        }
        "terraform" => {
            cmd.args(["fmt", "-"]);
        }
        _ => {}
    }
    let out = cmd.stdin(stdin).stdout(Stdio::piped()).stderr(Stdio::piped()).output()?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let first: String = err.lines().filter(|l| !l.trim().is_empty()).take(6).collect::<Vec<_>>().join("\n    ");
        bail!("{} failed on {}:\n    {}", tool.name, filename, first);
    }
    Ok(out.stdout)
}

pub fn doctor() -> i32 {
    println!("kempt formatters\n");
    println!("  {:<14} {:<10} {}", "tool", "status", "formats");
    let mut missing = 0;
    println!("  {:<14} {:<10} {}", "(built-in)", "ok", "JSON, JSONL, XML  — streaming, any file size");
    for t in ALL {
        match find_bin(t.bin) {
            Some(p) => println!("  {:<14} {:<10} {}   [{}]", t.name, "ok", t.langs, p.display()),
            None => {
                missing += 1;
                println!("  {:<14} {:<10} {}\n  {:<14} {:<10} install: {}", t.name, "MISSING", t.langs, "", "", t.install);
            }
        }
    }
    if missing > 0 { 1 } else { 0 }
}
