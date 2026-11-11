#!/usr/bin/env bash
# Build every release artifact into ./dist (run from the source folder).
#
#   packaging/build-release.sh            all platforms (cross-compiled with Zig)
#   packaging/build-release.sh linux      only some: linux | macos | windows | source
#
# Needs: rust (1.85+), python3 (for `pip install ziglang`), cargo-zigbuild, and for the non-Linux targets the Rust
# standard-library sources (rustup component rust-src), because the cross targets are built with -Zbuild-std.
# On GitHub the same artifacts are produced natively by .github/workflows/release.yml.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
VER="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
OUT="$PWD/dist"; NAME="$(basename "$PWD")"; mkdir -p "$OUT"
WANT="${*:-linux macos windows source}"
export PATH="$HOME/.cargo/bin:$PATH" RUSTC_BOOTSTRAP=1 MACOSX_DEPLOYMENT_TARGET=11.0
have() { case " $WANT " in *" $1 "*) return 0;; esac; return 1; }
zb() { cargo zigbuild --release --target "$1" -Zbuild-std=std,panic_abort; }
stage() {  # stage DIR  -> bundle skeleton with the common files
  rm -rf "$1"; mkdir -p "$1/packaging"; cp README.md LICENSE THIRD_PARTY_NOTICES.md CHANGELOG.md "$1/"
}
tarball() { (cd "$OUT/stage" && tar czf "$OUT/$1.tar.gz" "$1"); rm -rf "$OUT/stage/$1"; }

if have linux; then
  for t in "x86_64-unknown-linux-gnu.2.35:x86_64:amd64" "aarch64-unknown-linux-gnu.2.35:aarch64:arm64"; do
    tgt="${t%%:*}"; rest="${t#*:}"; arch="${rest%%:*}"; deb="${rest#*:}"
    zb "$tgt"; bin="target/${tgt%.2.35}/release/kempt"
    d="$OUT/stage/kempt-$VER-linux-$arch"; stage "$d"
    cp "$bin" install.sh uninstall.sh "$d/"; cp packaging/kempt.desktop packaging/kempt.svg "$d/packaging/"
    tarball "kempt-$VER-linux-$arch"
    KEMPT_VERSION="$VER" packaging/build-deb.sh "$bin" "$OUT" "$deb" >/dev/null
  done
fi
if have macos; then
  for t in "aarch64-apple-darwin:arm64" "x86_64-apple-darwin:x86_64"; do
    tgt="${t%%:*}"; arch="${t#*:}"; zb "$tgt"
    d="$OUT/stage/kempt-$VER-macos-$arch"; stage "$d"
    cp "target/$tgt/release/kempt" install.sh uninstall.sh "$d/"; cp packaging/kempt.icns "$d/packaging/"
    tarball "kempt-$VER-macos-$arch"
  done
fi
if have windows; then
  zb x86_64-pc-windows-gnu
  d="$OUT/stage/kempt-$VER-windows-x86_64"; stage "$d"
  cp target/x86_64-pc-windows-gnu/release/kempt.exe install.ps1 uninstall.ps1 install.cmd uninstall.cmd packaging/kempt.ico "$d/"
  (cd "$OUT/stage" && python3 -c "
import sys, zipfile, os
d = sys.argv[1]; z = zipfile.ZipFile(sys.argv[2], 'w', zipfile.ZIP_DEFLATED)
for root, _, files in os.walk(d):
    for f in sorted(files): p = os.path.join(root, f); z.write(p, p)
z.close()" "kempt-$VER-windows-x86_64" "$OUT/kempt-$VER-windows-x86_64.zip")
  rm -rf "$d"
fi
if have source; then
  (cd .. && tar czf "$OUT/kempt-$VER-src.tar.gz" --exclude="$NAME/target" --exclude="$NAME/dist" --exclude="$NAME/.git" --exclude='*/__pycache__' "$NAME")
fi
rm -rf "$OUT/stage"
(cd "$OUT" && sha256sum * > SHA256SUMS)
ls -la "$OUT"
