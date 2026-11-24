# Changelog

All notable changes are listed here. Format: [Keep a Changelog](https://keepachangelog.com/);
versioning: [SemVer](https://semver.org/).

## [Unreleased]

### Added
- `install-deb.sh` release asset: one-line clean install of the newest `.deb` (amd64 / arm64), verified against `SHA256SUMS`, with `--purge` to start from a first-run state. The release job now checks the checksums and unpacks every `.deb` before publishing.
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
