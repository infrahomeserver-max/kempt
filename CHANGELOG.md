# Changelog

All notable changes are listed here. Format: [Keep a Changelog](https://keepachangelog.com/);
versioning: [SemVer](https://semver.org/).

## [Unreleased]

### Added
- Built-in calendar (Alt+L): Day, Week, Month, Year and Schedule views, repeating events, per-event reminders, and checklist items shown on their day. Stored locally in `calendar.json`; the view code is a separate reusable file.

- New file can start in memory and is written only on the first Save.
- Fix unescapes backslash-escaped JSON and expands JSON stored inside string values.
- Excel binary workbooks (`.xlsb`) open like `.xlsx`, one tab per sheet.
- Open dialog: folders are amber and bold, files plain. No stray scrollbar in the tab bar.
- Meeting finder: neighbouring free times are joined into one block; the rare options are folded away.
- Help lists the terminal commands.

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
