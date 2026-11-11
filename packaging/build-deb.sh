#!/usr/bin/env bash
# Build kempt_<version>_amd64.deb from a built binary.   usage: packaging/build-deb.sh [path/to/kempt] [outdir] [amd64|arm64]
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="${1:-$HERE/target/release/kempt}"; OUT="${2:-$HERE}"
[ -x "$BIN" ] || { echo "build first: cargo build --release" >&2; exit 1; }
VER="${KEMPT_VERSION:-$("$BIN" --version | awk '{print $2}')}"
ARCH="${3:-$(dpkg --print-architecture)}"
ROOT="$(mktemp -d)"; trap 'rm -rf "$ROOT"' EXIT
chmod 0755 "$ROOT"
install -Dm0755 "$BIN" "$ROOT/usr/bin/kempt"
install -Dm0644 "$HERE/packaging/kempt.svg" "$ROOT/usr/share/icons/hicolor/scalable/apps/kempt.svg"
install -Dm0644 "$HERE/packaging/kempt.desktop" "$ROOT/usr/share/applications/kempt.desktop"
install -Dm0644 "$HERE/README.md" "$ROOT/usr/share/doc/kempt/README.md"
gzip -9n "$ROOT/usr/share/doc/kempt/README.md"
install -Dm0644 "$HERE/THIRD_PARTY_NOTICES.md" "$ROOT/usr/share/doc/kempt/THIRD_PARTY_NOTICES.md"
install -Dm0644 "$HERE/CHANGELOG.md" "$ROOT/usr/share/doc/kempt/changelog"; gzip -9n "$ROOT/usr/share/doc/kempt/changelog"
{ cat <<'C'
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: kempt

Files: *
Copyright: 2026 Infra Server
License: MIT

Third-party Rust crates and the embedded fonts (Inter, JetBrains Mono: SIL OFL 1.1)
keep their own licences; see /usr/share/doc/kempt/THIRD_PARTY_NOTICES.md.

C
echo "License: MIT"; sed 's/^/ /;s/^ $/ ./' "$HERE/LICENSE"; } > "$ROOT/usr/share/doc/kempt/copyright"
mkdir -p "$ROOT/DEBIAN"
SIZE=$(du -sk "$ROOT/usr" | cut -f1)
cat > "$ROOT/DEBIAN/control" <<C
Package: kempt
Version: $VER-1
Section: utils
Priority: optional
Architecture: $ARCH
Depends: libc6 (>= 2.34)
Suggests: shfmt, clang-format, nodejs, npm, pipx
Installed-Size: $SIZE
Maintainer: Kempt contributors <noreply@example.com>
Description: format, view, search, fix and compare huge JSON, XML and log files
 Streaming JSON/XML formatter (constant memory), memory-mapped viewer for
 multi-hundred-megabyte logs, a browser-based GUI with multi-keyword
 AND/OR find, replace, a Fix button for broken JSON/XML, side-by-side
 compare with per-line copy buttons, and dark/light themes.
 JSON, XML and logs need no extra tools; other languages use prettier,
 ruff, shfmt, sqlformat or clang-format when installed.
C
cat > "$ROOT/DEBIAN/postinst" <<'C'
#!/bin/sh
set -e
if [ "$1" = configure ]; then
  command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database -q /usr/share/applications || true
  command -v gtk-update-icon-cache  >/dev/null 2>&1 && gtk-update-icon-cache -q -t -f /usr/share/icons/hicolor || true
fi
exit 0
C
cat > "$ROOT/DEBIAN/postrm" <<'C'
#!/bin/sh
set -e
if [ "$1" = remove ] || [ "$1" = purge ]; then
  command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database -q /usr/share/applications || true
  command -v gtk-update-icon-cache  >/dev/null 2>&1 && gtk-update-icon-cache -q -t -f /usr/share/icons/hicolor || true
fi
exit 0
C
chmod 0755 "$ROOT/DEBIAN/postinst" "$ROOT/DEBIAN/postrm"
DEB="$OUT/kempt_${VER}-1_${ARCH}.deb"
dpkg-deb --root-owner-group -Zxz --build "$ROOT" "$DEB" >/dev/null
echo "$DEB"
