# Kempt

**Open, search, fix, format and compare big JSON, XML and log files.**

Kempt is a single small program (about 4 MB) for **Ubuntu and other Linux, macOS and Windows**. It has a graphical
interface that runs in your browser (`kempt gui`), a terminal viewer, and a command line for scripts and CI. It is built for files that
make ordinary editors struggle: 250 MB logs, gigabyte-sized JSON, one-line minified dumps.

- **Open huge files instantly.** A 325 MB log shows its first rows in about 0.2 s and uses a few MB of memory.
- **Find and replace.** Search several keywords at once with AND / OR, `"exact phrases"`, `-exclusions`, case, whole-word and regex options. Every match is highlighted, each keyword in its own colour.
- **Fix broken JSON and XML in one click.** Trailing commas, single quotes, comments, unclosed brackets and tags, and more.
- **Format** JSON, JSONL and XML at any file size in constant memory (about 14 MB). With 2 spaces, 4 spaces or tabs. Other languages are formatted through the usual tools (prettier, ruff, shfmt, sqlformat, clang-format).
- **Compare two files side by side**, like VS Code, with colour-highlighted differences, a copy button on every changed line, undo and save.
- **Dark, light and colour-blind-safe themes**, all inside a Settings dialog.
- **Private.** Everything runs on your computer. Nothing is uploaded. Kempt never goes online unless you switch on *Settings → General → Check for updates* (off by default), which makes one request a day to GitHub's release page.

> **Version 0.1.0** · MIT licence · Linux, macOS, Windows · no account, no cloud, no telemetry

## Contents

**Getting started**
1. [Download](#1-download-and-verify)
2. [Install](#2-install) — [Ubuntu / Debian](#21-ubuntu-debian-linux-mint-pop_os-deb-package) · [other Linux](#22-other-linux-fedora-rhel-opensuse-arch) · [macOS](#23-macos) · [Windows](#24-windows) · [from source](#25-build-from-source) · [apt / Homebrew](#26-apt-and-homebrew-when-published)
3. [First run](#3-first-run)
4. [Update](#4-update) · 5. [Uninstall](#5-uninstall)
6. [Supported platforms](#6-supported-platforms) · 7. [Optional formatters](#7-optional-formatters-for-other-languages)

**User manual**
[Graphical interface](#using-the-graphical-interface) · [World clock](#world-clock) · [Background, reminders, tray](#running-in-the-background-reminders-tray-icon-own-window) · [Checklist and notes](#checklist-notes-and-reminders) · [Command line](#command-line) · [Big data](#how-it-handles-big-data) · [Privacy and security](#privacy-and-security) · [Troubleshooting](#troubleshooting) · [Known limits](#known-limits)

**Project**
[Development](#development) · [Contributing](#contributing-licence-and-security) · [Publishing](#publishing-updates-apt-and-homebrew)

---

# Getting started

## 1. Download and verify

Get the files from the project's **Releases** page on GitHub (the right-hand side of the repository page, "Releases", then the newest version, then *Assets*).

| You use | Download this file |
|---|---|
| Ubuntu, Debian, Mint, Pop!_OS on a normal PC | `kempt_0.1.0-1_amd64.deb` |
| Ubuntu / Debian on ARM (Raspberry Pi 4/5 64-bit, ARM servers) | `kempt_0.1.0-1_arm64.deb` |
| Fedora, RHEL, openSUSE, Arch, other Linux (PC) | `kempt-0.1.0-linux-x86_64.tar.gz` |
| Other Linux on ARM | `kempt-0.1.0-linux-aarch64.tar.gz` |
| Mac with Apple chip (M1, M2, M3, M4) | `kempt-0.1.0-macos-arm64.tar.gz` |
| Mac with Intel chip | `kempt-0.1.0-macos-x86_64.tar.gz` |
| Windows 10 / 11 | `kempt-0.1.0-windows-x86_64.zip` |
| Build it yourself | `kempt-0.1.0-src.tar.gz` |

Not sure about your CPU? Linux/macOS: run `uname -m` (`x86_64` = PC/Intel, `aarch64` or `arm64` = ARM). Windows: Settings → System → About.

**Check the download (recommended).** Every release has a `SHA256SUMS` file. Put it in the same folder as the download, then:

```bash
# Linux
sha256sum --ignore-missing -c SHA256SUMS
# macOS
shasum -a 256 -c SHA256SUMS 2>/dev/null | grep kempt-0.1.0
```
```powershell
# Windows PowerShell: compare this value with the line for your file in SHA256SUMS
Get-FileHash .\kempt-0.1.0-windows-x86_64.zip -Algorithm SHA256
```
You want `OK` (Linux/macOS) or identical text (Windows). If it differs, download again and do not install it.

## 2. Install

Kempt installs a few files and stores nothing else, so it can be removed completely. You never need to be an administrator except for the optional all-users installs.

### 2.1 Ubuntu, Debian, Linux Mint, Pop!_OS (.deb package)

Needs Ubuntu 22.04 or newer, Debian 12 or newer (any release with glibc 2.34+).

```bash
cd ~/Downloads
sudo apt install ./kempt_0.1.0-1_amd64.deb      # arm64 machines: ./kempt_0.1.0-1_arm64.deb
kempt --version                                 # should print: kempt 0.1.0
kempt gui                                       # starts Kempt in your browser
```
- Keep the `./` in front of the file name; without it `apt` looks for a package in the online lists instead of your file.
- It installs `/usr/bin/kempt`, an application-menu entry ("Kempt"), an icon and the documentation. You can also double-click the `.deb` in the file manager and press *Install*.
- Remove it: `sudo apt remove kempt`.

### 2.2 Other Linux (Fedora, RHEL, openSUSE, Arch)

```bash
cd ~/Downloads
tar xzf kempt-0.1.0-linux-x86_64.tar.gz         # or ...-linux-aarch64.tar.gz
cd kempt-0.1.0-linux-x86_64
./install.sh --dry-run                          # optional: show what it found and what it would do
./install.sh                                    # installs for you only, into ~/.local (no sudo)
```
Then open a new terminal and run `kempt --version`. If it says *command not found*, add `~/.local/bin` to your PATH:
```bash
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.bashrc && source ~/.bashrc
```
For everyone on the machine use `sudo ./install.sh --system` (installs into `/usr/local`).

### 2.3 macOS

Needs macOS 11 (Big Sur) or newer.

```bash
cd ~/Downloads
tar xzf kempt-0.1.0-macos-arm64.tar.gz          # Intel Mac: kempt-0.1.0-macos-x86_64.tar.gz
cd kempt-0.1.0-macos-arm64
./install.sh
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zshrc && source ~/.zshrc
kempt --version
```
The installer also creates `~/Applications/Kempt.app`, removes the download quarantine flag and ad-hoc signs the binary. The program is **not notarised by Apple**; if macOS still says it cannot verify the developer, right-click `Kempt.app` → *Open* → *Open*, or run `xattr -dr com.apple.quarantine ~/.local/bin/kempt`.

### 2.4 Windows

Needs Windows 10 or 11 (64-bit).

1. Right-click the downloaded zip → **Extract All…** (do not run it from inside the zip).
2. Open the extracted folder and **double-click `install.cmd`**. (Or in PowerShell: `.\install.ps1`.) No administrator rights are needed; it installs to `%LOCALAPPDATA%\Programs\Kempt`.
3. Open a **new** terminal window (PATH changes only reach new windows) and run `kempt --version`, or use the **Kempt** entry in the Start menu.

Windows SmartScreen may warn "Windows protected your PC" because the program is not code-signed: choose *More info* → *Run anyway*. Optional: `.\install.ps1 -ContextMenu` adds "Open with Kempt" to the right-click menu; `-System` installs for all users (run PowerShell as administrator); `-DryRun` previews. Remove it under **Settings → Apps → Installed apps → Kempt**.

### 2.5 Build from source

Needs Rust 1.85+ and a C compiler (`build-essential` on Ubuntu, Xcode command-line tools on macOS).

```bash
git clone https://github.com/infrahomeserver-max/kempt.git && cd kempt      # or unpack kempt-0.1.0-src.tar.gz
cargo build --release                                          # produces target/release/kempt
./target/release/kempt --version
./install.sh                                                   # optional: install it (Linux / macOS)
```
`./install.sh` on its own also builds from source when no prebuilt binary suits your system (for example Alpine or very old distributions).

### 2.6 apt and Homebrew (when published)

If the maintainer has published the package repositories (see [Publishing](#publishing-updates-apt-and-homebrew)), you install and update with your normal tools:
```bash
# Ubuntu / Debian: the repository address and key come from the project's Releases page
sudo apt update && sudo apt install kempt && sudo apt upgrade
# macOS / Linux with Homebrew
brew tap infrahomeserver-max/kempt && brew install kempt && brew upgrade kempt
```

### Installer options (Linux / macOS)

| Option | Meaning |
|---|---|
| `--user` / `--system` / `--prefix DIR` | where to install (default `~/.local`, or `/usr/local` when run as root) |
| `--with-tools` | also install helper formatters (see below) |
| `--no-system-packages` | never call the package manager; only private tools |
| `--dry-run` | print the plan, change nothing |
| `--no-desktop` | skip the menu entry, icon and `Kempt.app` |
| `--build` / `--install-rust` | build from source even if a binary exists / allow installing Rust if the build needs it |

Uninstaller: `kempt-uninstall [--dry-run] [-y] [--purge-tools] [--user|--system|--prefix DIR]`. Every file and folder the installer creates is listed in `<prefix>/share/kempt/manifest.txt`; the uninstaller removes exactly those and touches nothing of yours. Running `install.sh` again upgrades in place.

## 3. First run

```bash
kempt gui                      # opens the app in your browser (a fresh tab)
kempt gui app.log              # open a file straight away
kempt gui old.json new.json    # two files open as a side-by-side comparison
kempt gui --no-open            # only print the address (use this over SSH)
```
1. Your browser opens at an address like `http://127.0.0.1:41873/?t=…`. The long `t=` value is a private key for this session; only your browser tab knows it.
2. Press **Open** (or drag a file in) and pick a file. Huge files open in a fraction of a second.
3. Try **Find** (Ctrl+F), **Fix** for broken JSON/XML, **Format**, **Compare**, and the clock and checklist buttons at the top. Press the **?** (Help) button for all shortcuts.
4. Nothing is written to your file until you press **Save**.
5. Open *Settings* (gear) to choose theme, browser, and (optionally) update checks.

On Ubuntu you can also start it from the application menu ("Kempt"). Check that everything is in order with `kempt doctor`.

## 4. Update

- **.deb:** download the new `.deb` and run `sudo apt install ./kempt_<new version>_amd64.deb` (it upgrades in place), or use `sudo apt update && sudo apt upgrade` if the apt repository is published.
- **tarball / zip:** unpack the new version and run its installer again; it replaces the old files.
- **Notice of new versions:** *Settings → General → Check for updates* is off by default. When you turn it on, Kempt asks GitHub once a day whether a newer release exists and shows a notice. It never downloads or installs anything by itself.

Your settings, notes and reminders are kept across updates. Back them up any time with *Settings → General → Backup → Export…*.

## 5. Uninstall

| System | Command |
|---|---|
| Ubuntu / Debian (.deb) | `sudo apt remove kempt` |
| Linux / macOS (install.sh) | `kempt-uninstall` (add `--dry-run` to preview) |
| Windows | Settings → Apps → Installed apps → Kempt → Uninstall, or `uninstall.cmd` in the install folder |

Your personal data (settings, notes, reminders) is a few small files in the config folder (`~/.config/kempt` on Linux, `~/Library/Application Support/kempt` on macOS, `%APPDATA%\kempt` on Windows). Delete that folder too if you want no trace left. If you enabled "start in background at login", turn it off first with `kempt autostart off`.

## 6. Supported platforms

| OS | Versions | CPU | Package | Install |
|---|---|---|---|---|
| **Ubuntu** | 22.04 LTS, 24.04 LTS and newer | x86_64, arm64 | `.deb`, tarball | `sudo apt install ./kempt_0.1.0-1_amd64.deb` |
| **Debian / Linux Mint / Pop!_OS** | Debian 12+, Mint 21+ | x86_64, arm64 | `.deb`, tarball | same as Ubuntu |
| **Fedora / RHEL / Rocky / Alma / openSUSE / Arch** | Fedora 36+, RHEL-family 9+, current openSUSE and Arch | x86_64, arm64 | tarball | `./install.sh` |
| **macOS** | 11 Big Sur and newer | Apple Silicon, Intel | tarball | `./install.sh` (creates `Kempt.app`) |
| **Windows** | 10 and 11 (x64; Windows 11 on ARM through emulation) | x64 | zip | `install.cmd` / `install.ps1` |
| Older Linux (Ubuntu 20.04, Debian 11, CentOS 7), Alpine | glibc < 2.34 or musl | any | source | `./install.sh` builds it (needs Rust 1.85+) |

**How well each is tested.** Linux (Ubuntu 24.04, x86_64) runs the complete automated test suite for every release, in Chromium and Firefox. The Linux binary is built against glibc 2.35, so it also runs on Ubuntu 22.04. macOS and Windows builds are cross-compiled; the Windows build's core commands and GUI server were run under Wine, and the installers' file logic is tested. They have **not yet been run on real macOS or Windows hardware** and the macOS and Windows packages are **unsigned**. The repository's CI (`.github/workflows/ci.yml`) builds and tests on real Ubuntu, macOS and Windows runners; treat a platform as verified once that is green. Real Ubuntu desktop integration (tray icon, dock, notifications) is likewise unverified.

**Browsers.** Chrome and Chromium, Firefox: tested. Edge, Brave, Vivaldi: Chromium-based, expected to behave the same, not run directly. Safari and DuckDuckGo use the system web view and were not tested.

## 7. Optional formatters for other languages

JSON, JSONL, XML, logs, viewing, find, replace, Fix and compare need nothing extra. For the languages below, run the installer
with `--with-tools` (`-WithTools` on Windows):

| Language | Tool | How it is installed |
|---|---|---|
| HTML, CSS, JS/TS, YAML, Markdown | prettier | privately, with npm. Old Node.js is handled: Node 18+ gets the latest prettier, 14-17 prettier 3.0.3, 10-13 prettier 2.8.8 (Ubuntu 22.04 ships Node 12) |
| Python | ruff | privately, in a Python virtual environment (the installer adds `python3-venv` on Debian/Ubuntu if missing) |
| SQL | sqlformat | privately, in the same virtual environment |
| Shell | shfmt | OS package (apt / dnf / pacman / zypper / Homebrew); on Windows `winget install mvdan.shfmt` |
| C, C++, Java | clang-format | OS package (`clang-format`, `clang-tools-extra` or `clang`); on Windows `winget install LLVM.LLVM` |

Private tools live in `share/kempt/tools` (`tools\` on Windows), are found by Kempt automatically, and are never added to PATH, so they
disappear with Kempt. OS packages are installed only if missing, recorded, and kept on uninstall unless you pass `--purge-tools`.
Run `kempt doctor` to see what is available.

---

# User manual

## Using the graphical interface

```bash
kempt gui                      # opens the app in your browser
kempt gui app.log              # open a file
kempt gui old.json new.json    # two files open as a comparison (add --tabs for two tabs instead)
kempt gui --no-open            # just print the address (for example over SSH)
kempt gui --port 8765          # fixed port instead of a free one
```
You can also open Kempt from the Ubuntu application menu, or right-click a file and choose Open With.
Over SSH, forward the port: `ssh -L PORT:127.0.0.1:PORT host`, then open the printed address locally.

**Nothing is written to your file until you press Save.** Every edit (replace, format, fix, copy a line)
creates a separate working copy and can be undone with Ctrl+Z (the last 6 versions are kept).

### Settings (gear icon, top right)
- **Appearance:** theme (System, Dark, Light, Colour-blind safe), code font size and the World clock colour season.
- **Editor:** indentation for Format and Fix (2 spaces, 4 spaces or Tab; changing it re-indents the JSON or XML file you have open), opening files read-only, opening delimited .txt/.csv as plain text, and expanding JSON stored inside strings.
- **General:** reminder sound (with a Test button), start Kempt in the background at login, which browser Kempt opens in, and its own-window mode.
- **General** also holds **Updates** (check for a newer release, off by default; enter your GitHub `owner/repo` once, or build with `KEMPT_REPO=owner/repo`) and **Backup** (Export / Import one JSON file with settings, zones, alerts, checklist and notes; recent files and folders stay on this computer).
- **About:** a short description, the keyboard shortcuts and Quit Kempt.

### Saved settings and the last folder
Your theme, code font size, indentation, recent files and the **last folder you opened a file from** are remembered
between runs. They live in one small JSON file (no database):

| System | File |
|---|---|
| Linux | `~/.config/kempt/state.json` (or `$XDG_CONFIG_HOME/kempt/state.json`) |
| macOS | `~/Library/Application Support/kempt/state.json` |
| Windows | `%APPDATA%\kempt\state.json` |

Open (Ctrl+O) starts in the last folder; if it no longer exists it falls back to your home folder. Delete the file to
reset everything. Only those few settings are stored, never file contents. The uninstaller keeps the file unless you
pass `--purge-settings` (Windows: `-PurgeSettings`).

### Several files at once (tabs)
Every file opens in its own tab, and so does every comparison. Each tab keeps its own scroll position, search,
undo history, and table state (sort, duplicates). Opening a file that is already open just switches to its tab.
Closing a tab with unsaved changes asks first.

### Table view (CSV, TSV and other delimited files)
`.csv`, `.tsv`, `.psv` and `.tab` files open as a table automatically. Any other text file (a `|`-separated log, say)
can be shown the same way with the **Table** button; click it again to return to the text.

- **Icons first.** The strip above the table has icons only (hover for the name): **separator**, **header row**,
  **funnel (filter)**, **sort**, **duplicates** and ✕ (reset the view). Whatever needs options opens in one slim
  *detail bar* under the strip, so there are no pop-up dialogs to hunt for. Click the icon again to close the bar.
- **Columns:** lettered A, B, C… with the first row used as the header (click the header icon to switch that off).
  Drag a column border to resize it, double-click the border to fit the width.
- **Separator:** comma, tab, semicolon, pipe `|` and space are built in and auto-detected. Pipe is fully supported, and
  so is **any other single character** (`^`, `~`, `:`, `#`…) via *Other character…* in the separator bar. A separator
  inside quotes (`"a|b"`) stays in the field. Not supported: separators longer than one character (`||`, `::`) and
  fixed-width files.
- **Filter (the funnel).** Click the funnel on a column header for an Excel-style list of that column's values with
  counts: search, tick the ones to keep, OK. The funnel turns blue on filtered columns. *Text filters…* (or the funnel
  in the strip) opens the filter bar with conditions: contains, does not contain, equals, does not equal, begins with,
  ends with, is blank, is not blank, `>`, `≥`, `<`, `≤`, between. Several conditions (on different or the same
  columns) must all match. Text comparisons ignore case (the **Aa** button switches to exact case); number
  comparisons read `$1,234`, `12%` and so on, and a word in a number box is reported instead of being ignored. The
  status bar shows `3 rows of 6 (filtered)`, the strip lists the active conditions with a *clear* link, and column
  statistics, sorting and duplicates all work on the filtered rows.
- **Sort:** click the ⇅ button in a column header (ascending, descending, off), or open the ▾ menu and pick a *kind* of
  sort with its ▲ / ▼ buttons. Blank cells always go last, and the bottom bar says which kind is in use.

  | Kind | What it does | Example |
  |---|---|---|
  | Automatic | numeric if the whole column is numbers, otherwise alphabetical | |
  | Alphabetical | A → Z, ignores upper/lower case | `apple`, `Banana`, `cherry` |
  | Lexicographic | exact character order: capitals before lower case, `10` before `2` | `Banana`, `apple` |
  | Numeric | by value; also reads `$1,234.50`, `12%`, `(5)` as −5 and `1 234` | `7`, `12%`, `$1,200` |
  | Natural | numbers inside text count by value, ignoring case | `file2`, `file10` |
  | Version | `1.2.9` before `1.2.10`; a leading `v` and the separators `. - _ +` are ignored | `1.2.9`, `v1.2.10`, `1.10.0` |
  | Date / time | year-first (`2026-11-03`, with optional time and `Z`/`+05:30`), day-first (`03/11/2026`), `3 Nov 2026`, `Nov 3, 2026`, `3:30 PM`, `14:30` | |
  | Date / time (US) | same, but `03/11/2026` means 11 March | |
  | Length | number of characters, shortest first | |
  | Frequency | how often the value repeats: ▲ least repeated first, ▼ most repeated first | |

  **Sorting by several columns ("by city, then by score").** *Shift+click* the ⇅ button of a second column to add it as
  another level (again to flip it, a third time to remove it); the headers show the level numbers (▲¹ ▼²). Or press
  the **sort** icon above the table, add levels, pick a column, a kind and a direction for each, and reorder them with ↑ ↓.
  Rows are ordered by level 1; rows that tie are ordered by level 2, and so on (up to 8 levels). The order of the levels
  matters. The kind of sort (for example lexicographic) only decides how the text *inside one column* is compared.

  The header button remembers the kind you chose for that column. Values that are not valid for the kind (text in a numeric
  sort, an unreadable date) go last with the blanks. Sorting also works inside a duplicates list. European decimals such as `1.234,56` are not understood.
- **Duplicates:** the duplicates icon opens a bar with a chip for every column. Tick **one** column to find repeated
  values in it, tick **several** to treat the combination as the key (`name` + `city` finds rows where both match another
  row), or leave *Entire row* ticked to compare the whole line; *Ignore case* is optional. Blank keys are never counted
  as duplicates. Results are grouped, colour-banded and marked `×N`; the strip says how many groups and extra copies
  there are, and *show all rows* brings everything back. Duplicates are searched inside the current filter, and
  sorting works inside the list.
- **Counts on the bottom bar:** the number of rows and columns, what you have selected (`12 rows selected`,
  `6 cells selected (3 × 2)`, `column D selected`) and, for a number selection, `Count`, `Sum` and `Avg`. Selecting a
  whole column adds `unique`, `blank` and `duplicate rows` counts plus `Sum / Avg / Min / Max`.
- **Selecting:** click or drag cells, click a row number or a column header, Shift-click or Shift-arrows to extend,
  Ctrl+A for everything. **Ctrl+C** copies the selection as tab-separated text, ready to paste into a spreadsheet.
- Large files are fine: a 1,000,000-row CSV opens instantly, sorts in under a second (natural and version sorts: about 1.5 s) and lists duplicates in about a second.
  The table only holds a list of row numbers (4 bytes per row) on top of the memory-mapped file.


### Viewers: Markdown, HTML and a JSON tree
The **eye** icon in the toolbar (or **Alt+V**) switches a file between its source and a rendered view.

- **Markdown** (`.md`, `.markdown`) opens rendered: headings, bold / italic / strikethrough, inline and fenced code,
  block quotes, nested bullet / numbered / task lists, tables with alignment, links (open in a new tab), horizontal
  rules, front matter (folded) and `:rocket:`-style emoji shortcodes. Raw HTML inside Markdown is shown as text, never
  run. Images are shown only when embedded as `data:` images; other images appear as a labelled placeholder because
  Kempt does not fetch files or web addresses for you.
- **HTML** (`.html`, `.htm`) opens as a preview in a locked-down sandbox: **no scripts, no forms, no outside resources**
  (images, fonts and styles from the web are blocked), and the page cannot touch Kempt. Pages that rely on JavaScript
  or web styles will look plainer than in a browser; use the source view to read or edit them.
- **JSON** opens as text first (large files stay instant); the eye icon turns it into a collapsible **tree**: counts
  next to every object and array (`{8}`, `[500]`), arrays shown 200 items at a time, *expand all* / *collapse all*, and a
  search box that steps through matching keys and values and opens the path to each. Click a key to copy its path
  (`$.nested.deep.k[2]`), double-click a value to copy it. Invalid JSON gets an explanation and a **Try Fix** button;
  JSON Lines files show as a list of records. The viewers load the whole file, so they are limited to 16 MB; larger
  files stay in the text view. Numbers beyond 15–16 significant digits lose precision in the tree (the text view is exact).
- Viewers update after Format, Fix or Replace, and Find switches back to the source automatically.

**Emoji and Unicode.** Emoji, accents, CJK and right-to-left text display and can be found: searching for `😀` highlights
exactly that character, JSON `\ud83d\ude00` escapes show as 😀, and long lines are cut on a character boundary, never in
the middle of an emoji. They are drawn with your system's emoji font (Noto Color Emoji on Ubuntu: `sudo apt install fonts-noto-color-emoji`
if you see empty boxes). In the monospaced source view an emoji is wider than one letter, so the horizontal-scroll
column counter is approximate on lines that contain them.

### New file, editing, Excel files and syntax colours
- **New** (top bar, **Alt+N**) creates a file with the name and extension you choose, in a folder you type or pick with **Browse…**, (quick buttons for `.txt .md .json .xml .csv .html .js .py .sh .log .yaml .sql`) in a folder of your choice, and opens it ready to type in. JSON, XML, HTML, shell and Python files get a small starter. An existing file is never overwritten.
- **Files are always editable.** Every file up to **2 MB** opens ready to type in; there is no Edit button to press. You get line numbers, colours as you type, **Tab / Shift+Tab** to indent a line or a block, **Enter** keeps the indentation (and adds one level after `{ [ ( :`), **Ctrl+/** comments or uncomments lines, **Ctrl+D** duplicates a line, and **Ctrl+Z / Ctrl+Y** undo and redo (including a Format or Fix). Your typing reaches the document a moment after you stop (and immediately before Save, Format, Fix, Compare or a view switch), so everything always sees it. **Find & Replace** switches to the search view and returns to the editor when you press Esc. Larger files open in the read-only viewer (Find & Replace, Format and Fix still work). Settings ▸ Formatting ▸ *Open files read-only* turns typing off if you prefer to browse safely. Line endings (LF / CRLF) and a byte-order mark are preserved. CSV / TSV and Excel sheets open as tables; the table icon switches them to the text editor.
- **Copy** (the copy icon next to Save, or **Ctrl+Shift+C**) puts the whole file on the clipboard, or just the selected text if you have selected some.
- **In a comparison** every line is editable: click a line on either side, retype it and press Enter (Esc cancels); the comparison updates at once and the **Save L / Save R** buttons write the files. The **copy L / copy R** buttons copy a whole side to the clipboard, and the arrow buttons still copy changes across.
- **JSON stored inside strings** (for example `"parameters": "{\"format\":\"%Y\"}"`): with *Settings ▸ Editor ▸ Expand JSON stored inside strings* on, **Format** and **Compare** show it as normal indented JSON (any depth). Only text that is valid JSON is expanded. This changes the data (a string becomes an object), so it is off by default and Ctrl+Z undoes it. On the command line: `kempt fmt --expand-embedded file.json`.
- **Excel (`.xlsx`, `.xlsm`)** opens as a table with **one tab per sheet** (up to 50 sheets; empty sheets are skipped): values and dates look as Excel shows them, formulas show their last calculated result, and line breaks inside a cell show as ⏎. Formatting, colours, charts and images are not shown. **Save writes CSV** (use Save as and give a `.csv` name); the original workbook is never changed. Old `.xls`, `.ods` and `.numbers` files cannot be read: save them as `.xlsx` or `.csv` first.
- **Delimited text**: `.txt` / `.dat` files whose lines are split by tabs, pipes, semicolons or commas (same number of columns on every line) open as a **table**. To see the raw text, use the **Open as** dropdown in the Open dialog (Automatic, Plain text, Table with a detected / comma / tab / semicolon / pipe separator, JSON, XML), switch the tab with the table icon, or turn on *Settings ▸ Formatting ▸ Opening files* to always open them as text. `.csv` and `.tsv` always open as tables.
- **Syntax colours** follow the VS Code palette of the selected theme: *Dark+* in the dark theme, *Light+* in the light theme, and a colour-blind-safe variant. Covered: JSON, XML / HTML, CSS, JavaScript / TypeScript, Python, shell, SQL, YAML, TOML / INI, Markdown, diff / patch, C-family languages (C, C++, C#, Java, Go, Rust, Kotlin, Swift, PHP…) and **logs** (timestamps, ERROR / WARN / INFO / DEBUG, stack traces, exception names, URLs). Markdown code blocks are coloured by their language. This is a lightweight highlighter (it tracks multi-line comments and strings while you edit, not every corner of every language), so unusual syntax may be left uncoloured; the viewer colours each line on its own, so a comment that starts on an earlier, off-screen line is not coloured there.

### Find and Replace (one button; Ctrl+F / Ctrl+H)
The **Find & Replace** button opens the find box with the Replace row visible. `Ctrl+F` opens Find only (click ▸ to
add Replace) and `Ctrl+H` opens it with Replace. **Replace** changes the current line; **Replace All** changes every
match, and both can be undone. In regex mode, `$1` groups work.

**How the box splits what you type into items**
- Spaces separate items: `error timeout` finds lines containing **any** of them. Choose **All keywords (AND)** to require all of them on the same line. The chips under the box show exactly how your text was split.
- **To keep spaces inside one item, use quotes:** `"connection timeout"`.
- A leading dash excludes: `-debug` drops lines containing `debug`. To search for text that really starts with a dash, quote it (`"-1"`) or escape it (`\-1`).
- To search for a literal double quote, escape it: `\"status\":`. Any other backslash is just a backslash, so Windows paths like `C:\temp` need no escaping.
- Symbols such as `( ) [ ] * + ? . $` are plain text unless the **.\*** (regex) toggle is on.
- **Everything as one item:** the **“ ”** toggle (Alt+P) searches for exactly what you typed, as a single phrase, with spaces, quotes and dashes untouched. Use it to paste in a whole sentence or a snippet of code without quoting anything.
- Toggles: **Aa** match case, **ab** whole word, **.\*** regular expression, **“ ”** one phrase.
- Every keyword has its own colour in the text, in the chips under the box, and as ticks on the right-edge ruler.
- `Enter`, `Shift+Enter` or `F3` step through matches ("3 of 120 lines"). Double-click any word to highlight all of its occurrences.

### Fix
Repairs broken **JSON**: trailing, missing or extra commas; single quotes; unquoted keys or values; `//` and
`/* */` comments; `True`, `False`, `None`, `NaN`, `undefined`; hex and `+5` / `.5` numbers; raw newlines in
strings; mismatched, missing or stray brackets; unterminated strings. For **XML** it repairs unclosed or
mismatched tags, stray `&`, unquoted or valueless attributes, and `<br>`-style void tags. It tells you exactly
what it fixed, and Undo reverts it.

### Format
Pretty-prints JSON and XML at any size, and other languages through their tools. Indentation comes from
Settings. From the command line: `kempt format --indent 4 f.json` or `--indent tab`.

### Compare
- Per row: **›** copies the line left to right, **‹** right to left, **✕** removes a line from its side. The first row of a multi-line change also has **»** / **«** to copy the whole block.
- `F7` / `Shift+F7` jump to the next or previous change. An overview ruler shows where the changes are.
- Toggles: hide unchanged, ignore whitespace, ignore case, mask ids (timestamps, UUIDs, IPs), swap sides, find inside the comparison.
- **Left ⇒ all** and **⇐ Right all** copy every change in one direction. **Save** each side when you are done.
- **Changes vs saved** shows exactly what your edits changed compared with the file on disk.
- Small JSON files are compared by structure (key order and layout ignored); larger files line by line.

### Keyboard shortcuts

On macOS use Cmd in place of Ctrl.

| Keys | Action |
|---|---|
| Ctrl+O | open a file |
| Alt+N | new file |
| Ctrl+Shift+C | copy the whole file or the selection |
| Tab / Shift+Tab, Ctrl+/, Ctrl+D | (while editing) indent / outdent, comment, duplicate line |
| Ctrl+S / Ctrl+Shift+S | save / save as |
| Ctrl+Z / Ctrl+Y (Ctrl+Shift+Z) | undo / redo |
| Ctrl+F / Ctrl+H | find / find and replace |
| Enter / Shift+Enter / F3 / Shift+F3 | next / previous match |
| Ctrl+G | go to line (or row in a table) |
| Ctrl+A, Ctrl+C | select everything, copy it (text view: the whole file, up to 16 MB; table: the selected cells as tab-separated text) |
| Ctrl+Shift+F | format |
| Alt+V | switch between source and the rendered view (Markdown, HTML, JSON tree) |
| Alt+T | switch between text and table |
| Alt+W | close the current tab |
| Ctrl+Shift+] / Ctrl+Shift+[ | next / previous tab |
| F7 / Shift+F7 | next / previous change (compare) |
| Arrows, PageUp / PageDown, Ctrl+Home / Ctrl+End | move around |
| F1 | help and shortcuts |
| Esc | close the find box, a menu or a dialog; clear a selection |

Ctrl+W, Ctrl+T, Ctrl+N and Ctrl+Tab belong to your web browser, which does not pass them to web pages, so Kempt uses the
Alt and Ctrl+Shift alternatives above. Kempt runs in your browser, so these are also the shortcuts the browser itself does not use.

---

## World clock

The clock button in the top bar (or **Alt+K**) opens a **World clock** tab: this computer's zone and UTC by default. Type a city, a zone id or an abbreviation such as `PST` to add one, drag the ⋮⋮ handle to reorder, × to remove. Each zone has a 24-hour strip (three tones only: night, day, working hours 9–17, tinted by season: spring greens, summer golds, autumn rust and plum, winter blues; the season is set in Settings → Appearance and defaults to *Auto*, which follows the shown date using northern-hemisphere seasons, and has light and dark versions); click or drag on any strip, or use the arrow keys, and one marker moves across every zone. A date-and-time box (type it or use the calendar) jumps every zone to that moment; the **in …** menu beside it says which zone you are typing in (the chosen zone's row gets a blue edge, and changing the menu re-reads the same moment in the new zone), **Now** returns to live time, and 12 h / 24 h is a toggle. Zones are remembered. It uses the browser's built-in time-zone data and your computer's clock, so it works offline and adds no code to the server; its one-minute timer only runs while the tab is visible.

**Holidays.** Under the clock, pick a country (India, United States, United Kingdom, Germany, France, Australia) to see its next ten public holidays; a ★ with the holiday's name appears on any zone row whose date falls on one. The built-in rules cover fixed dates, weekday rules (e.g. US Thanksgiving) and Easter-based days, including observed/substitute days for the US, UK and Australia. Holidays that follow the moon (Diwali, Eid, Holi, Chinese New Year…) change every year and cannot be worked out offline, so add them under "Your own dates" (tick *every year* to repeat). State, regional and company holidays are not included; check official calendars.

## Running in the background (reminders, tray icon, own window)

Reminders need something running. Kempt can now stay running quietly with no browser open:

- `kempt gui --background` starts it with no window; add `--detach` to get your terminal back. `kempt stop` stops it.
- `kempt autostart on` (or Settings → General → "Start Kempt in the background when I log in") starts it at every login: a `.desktop` entry on Linux, a Run key on Windows, a LaunchAgent on macOS. `kempt autostart off` undoes it.
- While it runs, **reminders are announced by the program itself**: a desktop notification (`notify-send` on Linux, a toast on Windows, Notification Centre on macOS) and the three-tone sound (`paplay`, `pw-play`, `aplay`, `play` or `ffplay` on Linux, `afplay` on macOS, PowerShell on Windows). No browser tab is needed. If a helper is missing, the open page does it instead.
- **Tray icon (Linux):** a Kempt icon appears in the system tray (the StatusNotifier protocol: KDE, XFCE, Cinnamon, and GNOME/Ubuntu with the AppIndicator extension, which Ubuntu enables by default). Click it to open Kempt; its menu has Open Kempt, Checklist & reminders, World clock and Quit. If your desktop has no tray host, there is simply no icon. Windows and macOS have no tray icon yet.
- Only one Kempt runs per user: `kempt gui file.json` while it is running just opens that file in the running copy.
- **Which browser:** Settings → General → "Browser" (System default plus whichever of Chrome, Firefox, Edge, Brave, Chromium, Vivaldi, Safari it finds installed; the list is detected each time Settings opens). It applies whenever Kempt opens itself (`kempt gui`, the tray icon, start at login); if the chosen one is not installed the system default is used.
- **Own window:** Settings → "Open Kempt in its own window…" (or `kempt gui --app`) opens it as an app window with its own taskbar entry, using Chrome, Edge, Brave or Chromium if installed (Firefox has no app-window mode).

**On Ubuntu (GNOME).** Three different places, three different mechanisms:
- *Top bar, right side:* the tray icon. Ubuntu ships the AppIndicator extension enabled; if you ever see no icon, run `gnome-extensions list | grep appindicator` and make sure it is enabled. It appears while Kempt is running (background mode or not).
- *Dock (the launcher on the left/bottom):* Kempt's own icon appears when a Kempt **window** is open. A normal browser tab shows the browser's icon, so turn on Settings → "Open Kempt in its own window…" (needs Chrome, Chromium, Brave or Edge). That window carries the window class `Kempt`, which `kempt.desktop` matches (`StartupWMClass`), so the dock groups it under Kempt's icon, and right-clicking the dock icon offers *Checklist & reminders* and *World clock*. Pin it to keep it there.
- *Notifications:* reminders use Ubuntu's own notification system (`notify-send`), so they appear as normal Ubuntu notifications at the top of the screen and in the notification list, with Kempt's icon.

Cost: the background copy is one sleeping thread that wakes only when a reminder is due (or every 30 s to re-check); no polling of the page.

**Working hours.** The 9-17 colour only applies on working days: on a Saturday or Sunday (in that zone) or on a holiday (the zone's country from the Holidays list, or your own dates) those hours are shown as plain day with a hatch, and the tooltip says why. Countries with a Friday/Saturday weekend are not modelled.

**Clock-change alerts.** The *Clock-change alerts* section lets you watch any zones (they need not be on the clock), pick how long before to be alerted (2 weeks, 1 week, 3 days, 1 day, 1 hour; several allowed) and shows the next 12 months of changes. Each alert is stored as an ordinary reminder (hidden from the checklist), so it uses the same desktop notification and sound, also with no browser open when Kempt runs in the background. The browser works out the dates, so Kempt schedules the next 12 months every time it is opened; if you never open it for a year, alerts stop until you do.

**Meeting finder.** The *Meeting finder* section lists the next times (up to 12) that fall inside working hours for everyone you tick: pick a length (30 min to 2 h), the working hours (default 9-17) and how far ahead to look (3, 7 or 14 days). It uses every zone on your clock and lets you add other people's zones without adding them to the clock. Saturdays, Sundays and holidays in each zone are skipped. Click a time to show it on the clock. **Check a time** lets you type a proposed date and time in *any* zone (not just yours) and shows what it is for everyone ticked, with a ✓ or the reason it does not fit (before or after hours, weekend, holiday). *Show suggestions in* lists the suggested times in another zone's clock.

**Daylight saving.** A zone that is currently on daylight saving time gets an amber **DST** badge next to its name. Under the strips, **Daylight saving through the year** draws one bar per zone for the calendar year being shown: shaded where the clocks are shifted, with short points per zone (DST now or standard, "▲ Mar 8 +1 h", "▼ Nov 1 −1 h", next change) and a marker for the date shown. Its rows follow the zone list, so dragging a zone reorders both. To compare a zone without adding it to the clock, type it in the box at the top of the DST section; it is listed as "comparison only" until you remove it (× next to its name). Click the **Daylight saving** or **Holidays** bar to fold it away (remembered; a folded section is not drawn at all). Southern-hemisphere zones (e.g. Sydney) show their two stretches the other way round, and zones that never change are marked "No DST". The rules come from the browser's own time-zone database, so they work offline and follow whatever your browser knows (keep the browser updated for law changes).

## Checklist, notes and reminders

The notes button (or **Alt+J**) opens a checklist, each item marked **Work** or **Personal** (filter All / Work / Personal, with counts; move an item with its menu; older items without a category show under All), with an optional reminder time per item, plus a free-text scratchpad. Everything is saved in one small file, `notes.json`, next to the settings (limit 512 KB): no database. A single timer is armed for the next reminder only, so nothing runs when there are none. Reminders pop up (with **Snooze 10 min** and **Done**, plus a desktop notification if you allow it) **while Kempt is open in a browser tab**; a background or minimised tab is fine. Kempt cannot wake itself when it is closed, so anything that came due then is shown the next time you open it, and the top-bar button carries a red count of due items. **Repeat:** a reminder can repeat daily, on weekdays, weekly or monthly (the 31st falls back to the month's last day). The item itself is always the next occurrence; the following ones are stored ahead of time (30 daily, 22 weekday, 13 weekly or 12 monthly) as hidden reminders, so the background service announces them with no page open. Ticking a repeating item moves it to the next time instead of finishing it; clearing its time or deleting it stops the series. Open Kempt at least once in that span to schedule more. A due reminder also plays a short three-tone sound (switch it off or test it in Settings → General; browsers only allow sound after you have clicked or typed in the page once) and flashes the tab title when the tab is in the background.

## Command line

```bash
# format
kempt format data.json config.xml script.py      # in place, atomic (temp file, then rename)
kempt format src/                                # a whole directory, in parallel
kempt format --check data.json                   # exit 1 if it would change (for CI)
kempt format --minify big.json                   # compact output
kempt format --indent tab data.json              # 1-8 spaces or tab
cat x.json | kempt format - --lang json          # stdin to stdout

# view in the terminal
kempt view app.log                               # interactive viewer
kempt view data.json                             # JSON/XML pretty-printed on open
kempt view -f /var/log/syslog                    # follow (tail -f), survives log rotation
kempt view app.log | head                        # in a pipe: plain text

# compare
kempt diff a.json b.json                         # structural: key order and layout ignored
kempt diff old.log new.log --mask                # ignore timestamps, UUIDs, IPs
kempt diff a.txt b.txt -y                        # side by side
kempt diff a b --patch fix.patch --html r.html   # export (the patch applies with patch(1))

kempt doctor                                     # which helper tools are installed
```
`kempt fmt` is accepted as an alias of `kempt format`. The command is `kempt`, not `fmt`, which is a coreutils program.

### Terminal viewer keys
`j k ↑ ↓ space b g G` move · `/` search (regex, smart-case), `n N` · `&` filter lines · `e` / `w` errors / warnings
and errors only · `:` go to line · `t` go to timestamp · `Enter` detail pane (pretty-prints a JSON log line, `J K`
scroll) · `F` follow · `← →` scroll sideways · mouse wheel · `?` help · `q` quit.

### Terminal compare keys
`n` / `p` next / previous difference · `c` changes only / full file · `← →` scroll · click the right-edge ruler to
jump · `?` help · `q` quit.

### Colours in the terminal
Green means added (only on the right), red means removed (only on the left), a yellow `~` marks a changed line,
bold bright text marks the exact characters that differ, and cyan marks type changes in structural JSON diffs.
Colour depth (truecolor, 256 or 16 colours) is detected automatically. `--theme colorblind` uses blue and orange.
`--no-color` or `NO_COLOR` gives plain output; the `+ - ~` markers always remain, so colour is never the only signal.

---

## How it handles big data

| Area | Approach |
|---|---|
| JSON, JSONL, XML formatting | byte-streaming state machine that never builds a tree: constant memory (about 14 MB) at any size |
| `format --check` | streamed comparison with early exit, no second copy |
| Writing files | temp file in the same folder, then atomic rename. Invalid input leaves the original untouched; symlinks and permissions are preserved |
| Viewer | memory-mapped file with a background line index; only the visible rows are rendered; search and filter run on background threads |
| Text diff | line hashes, then unique-line anchors, then Myers only inside the gaps; memory grows with the number of lines |
| JSON diff | structural (arrays aligned by element) up to `--struct-limit` (default 40 MB per file); above that, streamed format then diff |

Measured in a real browser on a 2-core sandbox, warm cache:

| Task | Size | Time |
|---|---|---|
| Open a log and show the first rows | 325 MB, 2.3 M lines | 0.18 s |
| Jump to the end / start of the file | same | 13–35 ms |
| OR / AND / regex search over every line, with highlights and ruler | same | 0.37–0.42 s |
| Replace All (143,750 lines) | same | 1.3 s |
| Undo | same | 0.06 s |
| Save atomically | 325 MB | 0.85 s |
| Format a one-line JSON into 31 M lines | 250 MB to 519 MB | 3.4 s |
| Fix (repair and format) | 250 MB | 5.4 s |
| Open a comparison of two logs | 2 × 325 MB | 0.19 s |
| Next change (F7) | same | 11–30 ms |
| Copy one line across and re-diff | same | 1.3 s |

From the command line, on the same machine:

| Task | Size | Time | Peak memory |
|---|---|---|---|
| Format minified JSON | 250 MB / 1 GB | 2.1 s / 8.0 s | 14 MB |
| Format single-line XML | 250 MB | 3.4 s | 14 MB |
| `format --check`, already formatted | 2.1 GB | 10 s | 14 MB |
| Open a log in the viewer, first screen | 325 MB | 24 ms | 4 MB |
| Viewer: search, filter, jump to a time | 325 MB | 0.2–0.35 s | +15 MB |
| Diff, 0.1 % of lines changed | 2 × 325 MB | 1.05 s | about 250 MB |
| Diff with `--mask` | 2 × 325 MB | 3.3 s | about 250 MB |

Formatting is lossless: minifying the formatted output gives back the original, byte for byte (checked on files from 250 MB to 1 GB).

---

## Privacy and security

- The GUI is a small web server that listens on `127.0.0.1` only. It requires a random per-run token and a loopback `Host` header, so other machines and other web pages cannot talk to it. It sends a strict Content-Security-Policy.
- Nothing is uploaded. The interface and its fonts are embedded in the program, so it works offline. The one exception is the opt-in update check: the program runs the system `curl` once a day to read `api.github.com/repos/<owner>/<repo>/releases/latest` and shows a notice; it never downloads or installs anything.
- Kempt writes only to files you explicitly save, and does so atomically. The only other thing it stores is the small settings file described above (theme, last folder, recent file names). Working copies are anonymous temporary files that disappear when the program exits.

## Troubleshooting

| Problem | Fix |
|---|---|
| `kempt: command not found` after a per-user install | add `~/.local/bin` to PATH: `export PATH="$HOME/.local/bin:$PATH"`. On Windows open a **new** terminal (PATH changes apply to new windows) |
| ``version `GLIBC_2.xx' not found`` | the system is older than Ubuntu 22.04 / Debian 12; build from source (`./install.sh` does this by itself if the source is present) |
| macOS says the app "cannot be opened because the developer cannot be verified" | the installer clears this; if it still appears, right-click the app and choose Open, or run `xattr -dr com.apple.quarantine ~/.local/bin/kempt` |
| Windows SmartScreen warns about `kempt.exe` | the build is unsigned; choose "More info" then "Run anyway". Code signing needs a paid certificate |
| PowerShell says scripts are disabled | run `install.cmd` instead, or `powershell -ExecutionPolicy Bypass -File install.ps1` |
| The browser doesn't open | run `kempt gui --no-open` and open the printed address yourself |
| Using it over SSH | `kempt gui --no-open --port 8765`, then `ssh -L 8765:127.0.0.1:8765 host` and open the address locally |
| "`prettier` is not installed" (or ruff, shfmt, ...) | install the helper tools with `--with-tools`, then check `kempt doctor` |
| `--with-tools` skipped something | the installer says what and why (for example "Node.js too old", "no python3-venv"); `--no-system-packages` means you install OS packages yourself |
| The page says it can't reach the server | the session ended; start `kempt gui` again (the address contains a one-time token) |
| Theme, font size or last folder were forgotten | they are stored in the settings file shown under *Saved settings*; check that the folder is writable, or delete the file to start fresh |
| The browser tab has no icon | restart `kempt gui` (the icon is served by the program); some browsers cache a missing icon until the tab is reopened |
| Windows: "Access denied" when saving | another program has the file open without sharing; close it and save again |

## Known limits

- Formatters that need the whole document (HTML, CSS, JS/TS, YAML, Markdown, Python, shell, SQL, C-family) run through external tools; files over `--max-size` (20 MB) are skipped unless you pass `--force`.
- XML mixed content that starts with a child element (`<p><b>x</b> text</p>`) is indented before the first child. Text-first mixed content is preserved exactly.
- Structural JSON diff holds both documents in memory (about 30 times the file size), hence the 40 MB default limit. Key order is not normalised in the streamed fallback.
- `--patch` can't be combined with `-i`, `-w` or `--mask`, because the patch would not apply exactly.
- The text diff and the viewer work by lines; very wide characters (CJK) count as one column in side-by-side layouts.
- The terminal viewer needs a real terminal. The mouse wheel is captured while it runs; hold Shift to select text.

**Platforms**
- Windows: log-rotation detection in `kempt view -f` is not available (no inode); the file picker starts in your user folder and has no drive list (type `D:\`); the Start-menu shortcut leaves a minimised console window while the server runs.
- macOS and Windows builds are unsigned (no Apple Developer ID / Authenticode certificate).
- Unix file permissions are preserved on Linux and macOS only.

**Graphical interface**
- Typing works up to 2 MB per file; the editor draws only the visible lines but keeps the whole text in the page, so very long single lines (over about 4,000 characters) are shown without colours. Spell-check and auto-close brackets are not provided. Typing is committed as one working copy per pause, so the 6-version history covers your last few pauses, not every keystroke (Ctrl+Z inside the editor undoes typing itself).
- Excel: values only (no formulas, formatting, charts, merged-cell layout or hidden-sheet handling); at most 50 sheets are opened, hidden sheets are shown like the others; saving produces CSV.
- Each edit creates a new working copy of the file, so very large files take about a second per edit, and history is limited to the last 6 versions. Memory grows with the number of lines (8 bytes per line per copy).
- Table view: every line is one record, so a quoted CSV field that contains a line break is split across two rows; at most 1,000 columns are shown; cells are clipped to 400 characters on screen (Ctrl+C copies the full text); duplicate detection compares 64-bit hashes (the chance of a false match is about one in 10 billion for a million rows). The table is a read-only view; edit in Text view (Find/Replace switch to it automatically). Separators are one character (no `||` or fixed-width files); the funnel's value list shows the 500 most frequent values of a column (use a text filter for the rest); filters and duplicate keys are applied to the first 300 columns in the pickers.
- AND / OR combine keywords within a line; there is no multi-line search.
- Lines longer than 8,000 bytes are shown truncated. Use Format to break up minified data.
- Find in the compare window highlights and steps through matches but does not count them.
- There is no timestamp jump in the browser interface (the terminal viewer has `t`); use Find with the timestamp text.

---

# Project

## Development

```bash
cargo build --release                 # needs Rust 1.85+, produces target/release/kempt (kempt.exe on Windows)
cargo test --release                  # unit tests (118)

python3 tests/test_api.py             # backend checks: auth, find, replace, fix, compare
python3 tests/test_security.py        # security checks: token, Host/DNS-rebinding, headers, injection, limits, secrets scan
python3 tests/test_filter.py          # column filters, duplicates, custom separators
python3 tests/test_table.py           # saved settings, favicon, table sort/duplicates, 1,000,000-row timings
python3 tests/test_files.py           # command line: format, check, diff, view, doctor
python3 tests/test_service.py         # background service, autostart, notifications, update check
python3 tests/test_ui.py              # browser end-to-end (pip install playwright; playwright install chromium)
python3 tests/test_ui_clock.py        # world clock, DST, meeting finder
python3 tests/test_ui_notes.py        # checklist, reminders, repeats
python3 tests/test_ui_tools.py        # update check, backup
python3 tests/test_ui_select.py       # selecting text (double/triple-click, drag), Ctrl+F
python3 tests/test_ui_edit.py         # editing, find/replace, compare
python3 tests/test_ui_layout.py       # layout and responsive checks
python3 tests/test_ui_viewers.py      # Markdown / HTML / JSON-tree viewers
python3 tests/test_ui_table.py        # table view
python3 tests/test_tui.py             # terminal UI in a pseudo-terminal (pip install pyte)
python3 tests/test_edge.py            # command-line robustness
python3 tests/test_packaging.py       # apt repository and Homebrew formula scripts (needs apt, gpg)
python3 tests/test_release_integrity.py dist   # version consistency, documents, checksums, package contents
tests/test_install.sh [--system] [--deb] [--tools] [--ps]   # installers: install/upgrade/removal, simulated distributions

# test another build, e.g. an installed or Windows binary under Wine:
KEMPT_BIN=/path/to/kempt python3 tests/test_files.py
```

**Releases.** `.github/workflows/ci.yml` builds and tests on Ubuntu 22.04 / 24.04 (x86_64 and arm64), macOS (Apple Silicon and Intel) and Windows. `.github/workflows/audit.yml` runs `cargo audit` and a licence check weekly. `.github/workflows/release.yml` builds the packages when you push a tag (`v*`) and attaches them to a GitHub release. Without GitHub, `packaging/build-release.sh` cross-compiles everything from one Linux machine with Zig (`cargo zigbuild`) and writes `dist/`: archives, `.deb` packages, a source archive and `SHA256SUMS`.

**Layout.** `src/` Rust (formatters, search, diff, viewer, GUI server, `plat.rs` for OS differences) · `ui/index.html` the whole browser interface · `packaging/` menu entry, icons, `.deb`/release builders, apt and Homebrew scripts · `install.*` / `uninstall.*` installers · `.github/` workflows and templates · `tests/`.

## Contributing, licence and security

Kempt is released under the [MIT licence](LICENSE). Contributions are welcome: read [CONTRIBUTING.md](CONTRIBUTING.md) (commits are signed off under the DCO) and the [Code of Conduct](CODE_OF_CONDUCT.md). Report vulnerabilities privately as described in [SECURITY.md](SECURITY.md). Changes are listed in [CHANGELOG.md](CHANGELOG.md); third-party licences in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

The interface uses **Inter** and code is shown in **JetBrains Mono**, both under the SIL Open Font License 1.1 and embedded in the program (Latin subsets).

## Publishing updates (apt and Homebrew)

`packaging/PUBLISHING.md` explains how to publish a signed apt repository and a Homebrew tap from a tag push, so installed copies update with `sudo apt update && sudo apt upgrade` or `brew upgrade`.
