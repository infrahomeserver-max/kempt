# Changelog

All notable changes are listed here. Format: [Keep a Changelog](https://keepachangelog.com/);
versioning: [SemVer](https://semver.org/).

## [0.2.7] - 2026-11-28

### Added
- **First-run introduction**: a seven-step tour of what Kempt can do, shown once to new users (not to anyone who already has recent files) and available later from Settings → About, Help and the welcome screen.
- **Same address after a restart**: Kempt reuses its last port and access token when that port is free, so a link from the browser history or a bookmark opens again after Kempt was restarted. Settings → General → *Keep the same address between runs* (on by default) turns it off. The token is kept in a private file (`address.json`, owner-only) outside the settings backup. `--port` is unchanged.

## [0.2.6] - 2026-11-27

### Fixed
- **Paste & compare**: Beautify, Beautify both, Clear, Swap and Paste can now be undone with Ctrl+Z, and each side has an Undo button.

## [0.2.5] - 2026-11-27

### Added
- **Lite mode** (Settings → Features): switch off the parts you do not use (files, Compare, Paste & compare, Checklist & notes, Calendar, World clock). They disappear from the menus, welcome screen and shortcuts and are not started; with one thing left Kempt opens straight into it. Presets: Everything, Files only, Just the clock, Clock + calendar, Checklist only.
- **Settings redesign**: a larger window with a search box, switches instead of tick boxes, cards for each group and new *Calendar & tools* and *Features* pages.
- **Welcome screen** counters: files opened this week, largest file, JSON files fixed and comparisons made (kept only on this computer), and a "Restore them" link when the last session had tabs.
- **Windows one-line installer**: `irm https://github.com/infrahomeserver-max/kempt/releases/latest/download/install.ps1 | iex` (checks the download against `SHA256SUMS`, adds "Open with Kempt").

### Changed
- **Big files open much faster on Windows.** Files of 64 MB and over are opened directly instead of being copied first (a 766 MB log no longer waits for a 766 MB copy). Saving over such a file needs Save As and says so.
- The line index is built in parallel on all cores (about 25 % faster to open a 766 MB log with two cores; more with more cores).
- Calendar, clock and reminder settings moved to *Calendar & tools*.

### Fixed
- **Tabs and unsaved edits are no longer lost** when the browser page is discarded or reloaded (for example after the laptop sleeps): the page takes the open files back from the running program.
- Flaky welcome-screen and find-mode tests on slow CI machines.

## [0.2.4] - 2026-11-27

### Added
- New landing page (dark and light designs) with recent files, pinned files and "restore last session".
- Opening a file from the command line while Kempt is running now adds it to the open window (drag and drop supported too).
- Compare any two open tabs (Ctrl+click, or right-click "Compare with...").
- Same-word highlighting when text is selected.
- Optional desktop notification and tab flash when a new file arrives.

### Fixed
- Selection colour no longer hides text in the editor.
- Windows installer now shows the correct version.

## [Unreleased]

## [0.2.9] - 2026-11-28

### Fixed
- Open dialog no longer shows a folder you did not ask for when its first (slow) listing arrives after you typed a path.

### Fixed
- Opening a pipe, device or socket as a file is refused instead of waiting on it forever. Settings, notes and the launch log are written readable by you only. Closing a big file gives its memory back at once.
- New automated checks: junk requests against every route, damaged and hostile files, script-injection payloads, file permissions and 16-thread load (tests/test_fuzz_api.py, test_fuzz_files.py, test_xss.py, test_perms.py).

### Fixed
- Several double-clicks while the browser is starting now open a single tab: exactly one launch opens the browser, the others wait and hand their file to it, and a big file is read only once.

### Fixed
- Double-clicking a file that is already open switches to its tab instead of adding a second copy, and the page starts listening for double-clicked files immediately (faster when the browser was just started).

### Fixed
- Double-clicking several times while the browser is still starting adds the files to the tab that is loading, instead of opening one tab per click.
- Rows that fail to load while a page is still starting are tried again, and a tab that loaded in the background draws itself when you switch to it (it could stay blank).
- `scripts/release.sh` no longer stops silently when a local tag differs from GitHub's; it fetches tags with force and says what failed.

## [0.2.8] - 2026-11-28

### Fixed
- Each `kempt gui <file>` launch writes one line to `launch.log` in the config folder (handed to the open window, or why not), to help track down double-clicks that seem to do nothing.

### Fixed
- Double-clicking a file while the Kempt tab is hidden behind other browser tabs now opens a new browser tab that comes to the front, instead of opening the file in the hidden tab where nothing seemed to happen.

### Fixed
- A file cut shorter or regenerated under the same name while open no longer crashes Kempt (blank window); it asks you to reopen the file instead.
- A file deleted or renamed while Kempt runs is no longer brought back as an empty tab when the window reloads or reconnects.

### Added
- Log tools in the Find box (⏱): level chips, go to a time (binary search), extract a time range into a new tab, and named saved searches. All on demand.

## [0.2.3] - 2026-11-27

### Added
- **Compare open tabs**: Ctrl+click (or Shift+click) two or more file tabs to select them, then press Compare in the bar that appears (with more than two, the first is compared with each of the others). Right-click a tab for "Compare with…", listing the other open files. Unsaved edits are included.
- **Same word highlighted everywhere**: select a word (double-click) in the viewer, editor, table, compare or notes and every other place it appears on screen is highlighted.
- **Open more files in the same window.** Double-clicking (or running `kempt gui`) on a second file now adds it as a new tab in the Kempt window that is already open, instead of a new browser tab, so two files can be compared. If no window is open, a browser tab opens as before.
- **Drag & drop**: drag files into the Kempt window to open each in a new tab. When the browser does not reveal the file's location, a copy is opened; Save then asks where to put it.

## [0.2.2] - 2026-11-25

### Changed
- **New logo**: a K with circuit lines. It is used for the browser tab icon, the app header and welcome screen (a light-tile version is shown in the light theme), the tray icon, the Linux, Windows (`.ico`) and macOS (`.icns`) icons and the README header.
- **New** (Alt+N) opens an empty `untitled.txt` at once, with no dialog. The folder, file name and extension chips are asked when you first save it (Ctrl+S); an existing name asks before it is replaced.
- Tabs of files show the unsaved dot once (it was shown twice).
- Closing one dialog no longer takes the keyboard focus away from the next one that opens (Esc and Enter work in it).

### Added
- Typing JSON or XML into an unsaved `untitled.txt` names it `.json` / `.xml` by itself, so it is coloured and Format and Fix work. Renaming a tab or choosing another extension in Save now also changes the editor's colours (they used to stay plain).
- **Rename a tab**: double-click its name or press F2. An unsaved file just changes its name (the extension changes its colours and tools); a file on disk is renamed in its folder and never over another file.

## [0.2.1] - 2026-11-25

### Added
- `install.sh` release asset: one-line install or clean reinstall of the newest `.deb` (amd64 / arm64) with a live download progress bar (percent, size, speed, ETA), verified against `SHA256SUMS`, `--purge` to start from a first-run state, `--version vX.Y.Z` to pin. The release job now checks the checksums and unpacks every `.deb` before publishing.
- **Paste & compare** (Tools menu, Alt+P): paste text into two panels, **Beautify** JSON or XML on either side (broken JSON is repaired), and **Compare** them in the usual diff tab, with no file saved anywhere. Ctrl+Enter compares; Swap, Copy, Clear and Paste buttons on each side.

## [0.2.0] - 2026-11-24

### Added
- More settings: accent colour, code font, 12/24-hour clock, working hours for meetings, checklist sort and ticked items, wrapped-line pane, hidden files in Open, and **Reset preferences**. About shows the version; Help → Formats shows which formatters are installed.
- One **Tools** menu in the top bar (Clock, Calendar, Checklist); the second Compare button is gone (use the toolbar's Compare with…).
- Pictures (PNG, JPG, GIF, WebP, BMP, SVG, ICO, AVIF) and **PDF** open in a viewer tab; Word (.docx), OpenDocument (.odt, .odp, .ods) and PowerPoint (.pptx) open as read-only text.
- Date format setting (default YYYY/MM/DD) with a new date and time picker used everywhere; a shared time-zone search that shows offsets and understands cst, cdt, cst6cdt, central, -6 and more.
- Checklist: sort by date, importance, newest or A–Z; Clear done says when there is nothing to clear.
- Folders and files have different, per-type colours in the file picker.
- Table and Fix are only enabled for files they apply to (Alt+T still forces the table).
- Wrap pane for long lines (Alt+Z), selected-lines count in the status bar, a clearer "Changes" button, and a calmer light theme.
- Help has a Formats tab listing every language Kempt can format.
- Notes and the checklist are stored encrypted (AES-256-GCM); the key is a separate owner-only file. `kempt notes-export` prints them as JSON.
- Notes need some text to be kept. The checklist no longer has its own calendar: it lists what is coming up, and Calendar opens from its button.
- Calendar: **drag an event** to another day (month view) or another day and time in 15-minute steps (week and day views); all-day events move by whole days. Dragging one occurrence of a repeating event moves only that one; Esc cancels a drag.
- Calendar: **Export** in the toolbar downloads every event as one `.ics` file (titles, places, notes, all-day, repeat rules, skipped days and reminders) that Google Calendar, Apple Calendar and Outlook import.
- Calendar: choose the first day of the week (Settings), meetings in their own colour, and a **Find a time** tab inside every event that marks free times for all your zones. The meeting finder is gone from the clock.
- Company holidays: add row above the list; choose the date format in Settings.
- Find: a pasted phrase is searched as one phrase; **Ctrl+E** adds the selected word to Find.
- Help is a compact three-tab view, and every main shortcut can be changed there.
- Panels (clock, calendar, checklist) have their own tab border and their top-bar button lights up while open.
- More formatter languages: Go, Rust, TOML, Lua, Terraform (tools you install), plus C#, Protobuf, MDX and more.
- World clock: live seconds, a big clock for your first zone, full-width layout, draggable sections (or Alt+Up/Down), a calendar in the meeting finder, and short zone names (IST, CST, EST, PST, CET and more; `cst6cdt`-style names are understood).
- Company holidays: add, import, scope to a country, hide built-in ones, and choose your weekend days.
- Checklist redesign: grouped by Overdue, Today, Tomorrow, This week, Later, No date and Done; quick Today/Tomorrow/Next week chips, important star, search, and a month calendar plus a "Coming up" list that includes calendar events.
- Notes: keep many notes, each with a title, a list and search. The old single scratchpad becomes the first note.
- Top bar buttons carry words on wide windows. Diff gutter buttons are smaller. No scrollbar in the tab bar.
- Built-in calendar (Alt+L): Day, Week, Month, Year and Schedule views, repeating events, per-event reminders, and checklist items shown on their day. Stored locally in `calendar.json`; the view code is a separate reusable file.
- New file can start in memory and is written only on the first Save.
- Fix unescapes backslash-escaped JSON and expands JSON stored inside string values.
- Excel binary workbooks (`.xlsb`) open like `.xlsx`, one tab per sheet.
- Open dialog: folders are amber and bold, files plain. No stray scrollbar in the tab bar.
- Meeting finder: neighbouring free times are joined into one block; the rare options are folded away.
- Help lists the terminal commands.

### Fixed
- Log viewer: dragging a selection past the top or bottom edge now scrolls and keeps selecting; Shift+click and Shift+drag extend the selection even after scrolling away from the starting point; Ctrl+C / Copy copies the whole range.
- Find: Next / Previous now paint the current match in a distinct orange so you can tell which hit is selected.
- A repeating calendar event that ends **on a date** now includes that last day (timed events used to stop one day early).

## [0.1.0] - 2026-11-09

First public release.

- Streaming JSON/XML formatter, memory-mapped log viewer, Fix for broken JSON/XML, side-by-side compare.
- Browser GUI (`kempt gui`) with tabs, find/replace, tables, tools, dark/light themes; fully offline.
- World clock: DST badges and comparison, clock-change alerts, meeting finder (any zone), working hours with weekend/holiday shading.
- Checklist and reminders: repeating reminders, Work/Personal categories, notes.
- Settings: appearance, editor, startup, browser auto-detect; backup export/import.
- Log viewer: double-click, triple-click and drag selection and Ctrl+F work reliably (selection survives the word highlight; a plain click no longer redraws lines mid-drag).
- Find: typing no longer makes the view jump per letter or the match counter flicker; the view moves only when typing pauses and never when a match is already on screen.
- Opt-in update check; packages: .deb (amd64/arm64), tar.gz, Windows zip; apt/Homebrew publishing scripts.
