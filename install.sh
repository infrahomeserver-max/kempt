#!/usr/bin/env bash
# Kempt installer for Linux and macOS. (Windows: use install.ps1.)
#
# It detects your OS, version, CPU and package manager and adapts. Everything it writes is listed in a
# manifest, so `kempt-uninstall` removes exactly that.
#
#   ./install.sh                  install for the current user into ~/.local (no sudo)       [default]
#   sudo ./install.sh --system    install for everyone into /usr/local
#   ./install.sh --prefix DIR     install under DIR (DIR/bin, DIR/share/...)
#   ./install.sh --with-tools     also install formatters for HTML/CSS/JS/YAML/MD (prettier), Python (ruff),
#                                 SQL (sqlformat), shell (shfmt) and C/Java (clang-format). Missing OS packages
#                                 (node, python venv, shfmt, clang-format) come from your package manager.
#   ./install.sh --no-system-packages   never call apt/dnf/pacman/zypper/brew; only private tools
#   ./install.sh --dry-run        show what would happen (including detected OS and packages), change nothing
#   ./install.sh --no-desktop     skip the menu entry / icon / app bundle
#   ./install.sh --build          build from source even if a prebuilt binary exists
#   ./install.sh --install-rust   allow installing Rust (rustup) if a source build needs it
#   ./install.sh -y               don't ask questions
#
# Supported: Ubuntu 22.04+, Debian 12+, Fedora 36+, RHEL/Rocky/Alma 9+, openSUSE, Arch (x86_64, aarch64),
#            macOS 11+ (Intel and Apple Silicon). JSON, XML, logs, search, fix and compare need no extra tools.
# Remove again with:  kempt-uninstall      (or ./uninstall.sh [--user|--system|--prefix DIR])
set -euo pipefail

ORIG_ARGS=("$@")
MODE=""; PREFIX=""; WITH_TOOLS=0; DRY=0; DESKTOP=1; BUILD=0; INSTALL_RUST=0; SYSPKGS=1; YES=0
while [ $# -gt 0 ]; do
  case "$1" in
    --user) MODE=user ;;
    --system) MODE=system ;;
    --prefix) PREFIX="${2:?--prefix needs a directory}"; shift ;;
    --prefix=*) PREFIX="${1#*=}" ;;
    --with-tools) WITH_TOOLS=1 ;;
    --no-system-packages) SYSPKGS=0 ;;
    --dry-run) DRY=1 ;;
    --no-desktop) DESKTOP=0 ;;
    --build) BUILD=1 ;;
    --install-rust) INSTALL_RUST=1 ;;
    -y|--yes) YES=1 ;;
    -h|--help) sed -n '2,24p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown option: $1 (try --help)" >&2; exit 2 ;;
  esac
  shift
done

say()  { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33mwarning:\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }
plan() { printf '    \033[2m%s\033[0m\n' "$*"; }

# dotted-version compare: ver_ge A B  => true when A >= B
ver_ge() { awk -v a="$1" -v b="$2" 'BEGIN{n=split(a,x,".");m=split(b,y,".");k=(n>m?n:m);for(i=1;i<=k;i++){if((x[i]+0)>(y[i]+0))exit 0;if((x[i]+0)<(y[i]+0))exit 1}exit 0}'; }

: "$YES"   # accepted for compatibility: the installer never asks questions

# ---------------------------------------------------------------- detect the environment
# (the KEMPT_FAKE_* / KEMPT_OS_RELEASE variables exist so the detection can be tested for other systems)
OS="${KEMPT_FAKE_OS:-$(uname -s 2>/dev/null || echo unknown)}"
ARCH="${KEMPT_FAKE_ARCH:-$(uname -m 2>/dev/null || echo unknown)}"
case "$ARCH" in x86_64|amd64) ARCH=x86_64 ;; aarch64|arm64) ARCH=arm64 ;; esac
PLAT=""; DISTRO_ID=""; DISTRO_LIKE=""; DISTRO_VER=""; DISTRO_NAME=""; PM=""; LIBC=""; MACVER=""
case "$OS" in
  Linux)
    PLAT=linux
    OSR="${KEMPT_OS_RELEASE:-/etc/os-release}"
    if [ -r "$OSR" ]; then
      info="$( . "$OSR" 2>/dev/null; printf '%s\n%s\n%s\n%s\n' "${ID:-}" "${ID_LIKE:-}" "${VERSION_ID:-}" "${PRETTY_NAME:-}" )"
      DISTRO_ID="$(printf '%s\n' "$info" | sed -n 1p)"; DISTRO_LIKE="$(printf '%s\n' "$info" | sed -n 2p)"
      DISTRO_VER="$(printf '%s\n' "$info" | sed -n 3p)"; DISTRO_NAME="$(printf '%s\n' "$info" | sed -n 4p)"
    fi
    case " $DISTRO_ID $DISTRO_LIKE " in
      *" debian "*|*" ubuntu "*) PM=apt ;;
      *" fedora "*|*" rhel "*|*" centos "*) PM=dnf ;;
      *" arch "*) PM=pacman ;;
      *" suse "*|*" opensuse "*) PM=zypper ;;
      *" alpine "*) PM=apk ;;
    esac
    if [ -z "${KEMPT_FAKE_OS:-}" ] && [ -n "$PM" ] && ! command -v "$( [ "$PM" = apt ] && echo apt-get || echo "$PM" )" >/dev/null 2>&1; then PM=""; fi
    LIBC="${KEMPT_FAKE_GLIBC:-}"
    if [ -z "$LIBC" ]; then
      LIBC="$(getconf GNU_LIBC_VERSION 2>/dev/null | awk '{print $2}')" || LIBC=""
      if [ -z "$LIBC" ]; then
        if ldd --version 2>&1 | grep -qi musl; then LIBC=musl; else LIBC="$(ldd --version 2>&1 | head -1 | awk '{print $NF}')"; fi
      fi
    fi
    ;;
  Darwin)
    PLAT=macos
    MACVER="${KEMPT_FAKE_MACVER:-$(sw_vers -productVersion 2>/dev/null || echo 0)}"
    DISTRO_NAME="macOS $MACVER"
    if [ -n "${KEMPT_FAKE_OS:-}" ] || command -v brew >/dev/null 2>&1; then PM=brew; fi
    ;;
  *) die "unsupported system '$OS'. On Windows use install.ps1 (PowerShell)." ;;
esac
case "$ARCH" in x86_64|arm64) ;; *) die "unsupported CPU architecture '$ARCH' (supported: x86_64, arm64/aarch64)" ;; esac

MIN_GLIBC=2.34; MIN_MAC=11
if [ "$PLAT" = linux ]; then
  say "Detected: ${DISTRO_NAME:-Linux} · $ARCH · libc ${LIBC:-unknown} · package manager: ${PM:-none found}"
else
  say "Detected: $DISTRO_NAME · $ARCH · package manager: ${PM:-none (Homebrew not installed)}"
fi
if [ "$PLAT" = macos ] && ! ver_ge "$MACVER" "$MIN_MAC"; then die "macOS $MIN_MAC or newer is required (you have $MACVER)"; fi
if [ "$PLAT" = linux ] && [ "$LIBC" = musl ]; then warn "musl-based system (Alpine): the prebuilt binary needs glibc; it will be built from source instead"; fi

# ---------------------------------------------------------------- where to install
if [ -z "$PREFIX" ]; then
  if [ -z "$MODE" ]; then [ "$(id -u)" -eq 0 ] && MODE=system || MODE=user; fi
  if [ "$MODE" = system ]; then PREFIX=/usr/local; else PREFIX="${HOME:?HOME is not set}/.local"; fi
fi
PREFIX="${PREFIX%/}"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [ ! -f "$HERE/Cargo.toml" ] && [ ! -x "$HERE/kempt" ] && [ -f "$HERE/../Cargo.toml" ]; then HERE="$(cd "$HERE/.." && pwd)"; fi
PK="$HERE/packaging"; [ -d "$PK" ] || PK="$HERE"

# Need root only if the target is not writable by us: re-run the whole script via sudo.
probe="$PREFIX"; while [ ! -e "$probe" ]; do probe="$(dirname "$probe")"; done
if [ "$DRY" -eq 0 ] && [ ! -w "$probe" ] && [ "$(id -u)" -ne 0 ]; then
  command -v sudo >/dev/null 2>&1 || die "$PREFIX is not writable and sudo is not available"
  say "$PREFIX needs administrator rights; re-running with sudo"
  exec sudo env "HOME=$HOME" "$0" ${ORIG_ARGS[@]+"${ORIG_ARGS[@]}"}
fi

SHARE="$PREFIX/share/kempt"; MANIFEST="$SHARE/manifest.txt"
if [ -e "$MANIFEST" ] && [ "$DRY" -eq 0 ]; then say "Kempt is already installed in $PREFIX; upgrading in place"; fi

# ---------------------------------------------------------------- helpers that record what we create
LOG=()                      # manifest lines
rec() { LOG+=("$1"); }
PLANNED=":"                 # dry run: directories we pretend to have created
do_mkdir() {                # mkdir -p, recording every directory we create
  local d="$1" stack=() i
  while [ ! -e "$d" ] && [ "$DRY$PLANNED" = "${DRY}${PLANNED}" ] && case "$PLANNED" in *":$d:"*) false;; *) true;; esac; do stack+=("$d"); d="$(dirname "$d")"; done
  [ "${#stack[@]}" -eq 0 ] && return 0
  for ((i=${#stack[@]}-1; i>=0; i--)); do
    if [ "$DRY" -eq 1 ]; then plan "mkdir ${stack[$i]}"; PLANNED="$PLANNED${stack[$i]}:"; else mkdir "${stack[$i]}"; fi
    rec "dir:${stack[$i]}"
  done
}
put() {                     # put MODE SRC DEST
  local mode="$1" src="$2" dest="$3"
  do_mkdir "$(dirname "$dest")"
  if [ "$DRY" -eq 1 ]; then plan "install $dest"; else install -m "$mode" "$src" "$dest"; fi
  rec "file:$dest"
}

# package-manager helpers ------------------------------------------------
pkgname() {   # logical name -> package name for this package manager ("" = nothing to install)
  case "$PM:$1" in
    apt:node) echo nodejs;;     apt:npm) echo npm;;     apt:python) echo python3;;  apt:venv) echo python3-venv;;
    apt:shfmt) echo shfmt;;     apt:clangfmt) echo clang-format;;                   apt:cc) echo build-essential;;
    dnf:node) echo nodejs;;     dnf:npm) echo npm;;     dnf:python) echo python3;;  dnf:venv) echo "";;
    dnf:shfmt) echo shfmt;;     dnf:clangfmt) echo clang-tools-extra;;              dnf:cc) echo gcc;;
    pacman:node) echo nodejs;;  pacman:npm) echo npm;;  pacman:python) echo python;; pacman:venv) echo "";;
    pacman:shfmt) echo shfmt;;  pacman:clangfmt) echo clang;;                       pacman:cc) echo base-devel;;
    zypper:node) echo nodejs;;  zypper:npm) echo npm;;  zypper:python) echo python3;; zypper:venv) echo "";;
    zypper:shfmt) echo shfmt;;  zypper:clangfmt) echo clang;;                       zypper:cc) echo gcc;;
    brew:node) echo node;;      brew:npm) echo "";;     brew:python) echo python;;  brew:venv) echo "";;
    brew:shfmt) echo shfmt;;    brew:clangfmt) echo clang-format;;                  brew:cc) echo "";;
    *) echo "";;
  esac
}
have() { if [ -n "${KEMPT_FAKE_OS:-}" ]; then return 1; fi; command -v "$1" >/dev/null 2>&1; }   # simulated systems have nothing installed
pkg_installed() {
  [ -z "${KEMPT_FAKE_OS:-}" ] || return 1
  case "$PM" in
    apt) dpkg -s "$1" >/dev/null 2>&1 ;;
    dnf|zypper) rpm -q "$1" >/dev/null 2>&1 ;;
    pacman) pacman -Qi "$1" >/dev/null 2>&1 ;;
    brew) brew list --formula "$1" >/dev/null 2>&1 ;;
    *) return 1 ;;
  esac
}
as_root() { if [ "$(id -u)" -eq 0 ]; then "$@"; elif command -v sudo >/dev/null 2>&1; then sudo "$@"; else return 1; fi; }
pkg_run_install() {   # pkg_run_install name...   (returns non-zero on failure)
  case "$PM" in
    apt) as_root env DEBIAN_FRONTEND=noninteractive apt-get install -y "$@" ;;
    dnf) as_root dnf install -y "$@" ;;
    pacman) as_root pacman -S --noconfirm --needed "$@" ;;
    zypper) as_root zypper --non-interactive install "$@" ;;
    brew) brew install "$@" ;;
    *) return 1 ;;
  esac
}
install_pkgs() {      # install_pkgs why name...  — installs the missing ones and records them
  local why="$1"; shift; local want=() p
  for p in "$@"; do [ -n "$p" ] || continue; pkg_installed "$p" || want+=("$p"); done
  [ "${#want[@]}" -gt 0 ] || return 0
  if [ "$SYSPKGS" -eq 0 ] || [ -z "$PM" ]; then
    warn "$why needs: ${want[*]} — not installing OS packages (${PM:+--no-system-packages}${PM:-no package manager found}). Install them yourself, then re-run."
    return 1
  fi
  if [ "$DRY" -eq 1 ]; then plan "$PM install ${want[*]}    ($why)"; return 0; fi
  say "Installing OS packages for $why: ${want[*]}"
  if pkg_run_install "${want[@]}" >/dev/null 2>&1 || pkg_run_install "${want[@]}"; then
    for p in "${want[@]}"; do rec "pkg:$PM:$p"; done
  else
    warn "could not install ${want[*]} with $PM (no sudo or no network?). Run it yourself: $PM install ${want[*]}"
    return 1
  fi
}

# ---------------------------------------------------------------- 1. get a binary
runs_ok() { [ -x "$1" ] && "$1" --version 2>/dev/null | grep -q '^kempt '; }
BIN=""; WHY_NOT=""
if [ "$BUILD" -eq 0 ]; then
  for cand in "$HERE/kempt" "$HERE/target/release/kempt"; do
    [ -x "$cand" ] || continue
    if [ "$PLAT" = linux ] && { [ "$LIBC" = musl ] || { [ -n "$LIBC" ] && ! ver_ge "$LIBC" "$MIN_GLIBC"; }; }; then
      WHY_NOT="needs glibc $MIN_GLIBC or newer (this system has ${LIBC:-an unknown libc})"; continue
    fi
    if [ -n "${KEMPT_FAKE_OS:-}" ]; then BIN="$cand"; break; fi
    if runs_ok "$cand"; then BIN="$cand"; break; else WHY_NOT="the prebuilt binary does not run here (wrong CPU or system?)"; fi
  done
fi
if [ -z "$BIN" ]; then
  [ -f "$HERE/Cargo.toml" ] || die "${WHY_NOT:-no kempt binary found next to install.sh}, and there is no source to build from.
  Download the build for your system from the project's releases page, or use the source package."
  [ -n "$WHY_NOT" ] && warn "$WHY_NOT — building from source instead"
  ok_rust=0
  if command -v cargo >/dev/null 2>&1 && command -v rustc >/dev/null 2>&1; then
    v="$(rustc --version | awk '{print $2}')"
    ver_ge "$v" 1.85 && ok_rust=1
  fi
  if [ "$ok_rust" -eq 0 ]; then
    [ "$INSTALL_RUST" -eq 1 ] || die "building needs Rust >= 1.85 (found: $(command -v rustc >/dev/null 2>&1 && rustc --version || echo none)).
  Install it yourself (https://rustup.rs) or re-run with --install-rust to let this script do it
  (that adds ~/.cargo and ~/.rustup, which kempt-uninstall --purge-tools will remove again)."
    say "Installing Rust via rustup"
    if [ "$DRY" -eq 1 ]; then plan "rustup (minimal profile) -> ~/.cargo ~/.rustup"; else
      curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
      # shellcheck disable=SC1091
      . "$HOME/.cargo/env"
    fi
    rec "rustup:$HOME"
  fi
  if ! command -v cc >/dev/null 2>&1; then
    if [ "$PLAT" = macos ]; then die "a C toolchain is needed to build: run  xcode-select --install  and try again"; fi
    install_pkgs "building Kempt" "$(pkgname cc)" || die "a C compiler/linker is required to build"
  fi
  say "Building Kempt (release)"
  if [ "$DRY" -eq 1 ]; then plan "cargo build --release"; else (cd "$HERE" && cargo build --release); fi
  BIN="$HERE/target/release/kempt"
fi

say "Installing into $PREFIX"
put 0755 "$BIN" "$PREFIX/bin/kempt"
put 0755 "$HERE/uninstall.sh" "$SHARE/uninstall.sh"
if [ "$DRY" -eq 1 ]; then plan "link $PREFIX/bin/kempt-uninstall"; else ln -sfn "$SHARE/uninstall.sh" "$PREFIX/bin/kempt-uninstall"; fi
rec "file:$PREFIX/bin/kempt-uninstall"

if [ "$PLAT" = macos ] && [ "$DRY" -eq 0 ]; then
  # downloaded binaries carry a quarantine flag and unsigned arm64 code won't start: clear / ad-hoc sign (best effort)
  xattr -d com.apple.quarantine "$PREFIX/bin/kempt" 2>/dev/null || true
  command -v codesign >/dev/null 2>&1 && codesign --force --sign - "$PREFIX/bin/kempt" 2>/dev/null || true
fi

if [ "$DESKTOP" -eq 1 ] && [ "$PLAT" = linux ] && [ -f "$PK/kempt.desktop" ]; then
  APPS="$PREFIX/share/applications"
  do_mkdir "$APPS"
  if [ "$DRY" -eq 1 ]; then plan "install $APPS/kempt.desktop"; else
    sed "s|^Exec=kempt |Exec=$PREFIX/bin/kempt |" "$PK/kempt.desktop" > "$APPS/kempt.desktop"; chmod 0644 "$APPS/kempt.desktop"; fi
  rec "file:$APPS/kempt.desktop"
  [ -f "$PK/kempt.svg" ] && put 0644 "$PK/kempt.svg" "$PREFIX/share/icons/hicolor/scalable/apps/kempt.svg"
fi

if [ "$DESKTOP" -eq 1 ] && [ "$PLAT" = macos ]; then
  if [ "$PREFIX" = /usr/local ]; then APPDIR=/Applications; else APPDIR="$HOME/Applications"; fi
  APP="$APPDIR/Kempt.app"
  if [ -e "$APP" ] && ! grep -q "^dir:$APP\$" "$MANIFEST" 2>/dev/null; then
    warn "$APP already exists and was not created by this installer; skipping the app bundle"
  else
    do_mkdir "$APP/Contents/MacOS"; do_mkdir "$APP/Contents/Resources"
    if [ "$DRY" -eq 1 ]; then plan "create $APP"; else
      cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>Kempt</string>
  <key>CFBundleDisplayName</key><string>Kempt</string>
  <key>CFBundleIdentifier</key><string>io.kempt.app</string>
  <key>CFBundleExecutable</key><string>Kempt</string>
  <key>CFBundleIconFile</key><string>kempt</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.3.2</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
      printf '#!/bin/bash\nexec "%s/bin/kempt" gui "$@"\n' "$PREFIX" > "$APP/Contents/MacOS/Kempt"; chmod 0755 "$APP/Contents/MacOS/Kempt"
      [ -f "$PK/kempt.icns" ] && cp "$PK/kempt.icns" "$APP/Contents/Resources/kempt.icns"
    fi
    rec "file:$APP/Contents/Info.plist"; rec "file:$APP/Contents/MacOS/Kempt"; [ -f "$PK/kempt.icns" ] && rec "file:$APP/Contents/Resources/kempt.icns"
  fi
fi

# ---------------------------------------------------------------- 2. optional formatter tools
# Everything except OS packages goes into $SHARE/tools, so removing Kempt removes it too.
if [ "$WITH_TOOLS" -eq 1 ]; then
  say "Formatter tools (private to Kempt, in $SHARE/tools)"
  TOOLS="$SHARE/tools"
  [ "$DRY" -eq 1 ] || do_mkdir "$TOOLS"

  # ---- Node + prettier
  if ! have node || ! have npm; then
    install_pkgs "prettier (needs Node.js)" "$(pkgname node)" "$(pkgname npm)" || true
  fi
  NODEV="${KEMPT_FAKE_NODE:-}"
  if [ -z "$NODEV" ] && have node; then NODEV="$(node --version 2>/dev/null | sed 's/^v//')"; fi
  if [ -z "$NODEV" ] && [ "$DRY" -eq 1 ] && [ -n "$(pkgname node)" ]; then
    case "${DISTRO_ID}:${DISTRO_VER}" in ubuntu:20.04) NODEV=10.19.0 ;; ubuntu:22.04) NODEV=12.22.9 ;; debian:11) NODEV=12.22.12 ;; *) NODEV=20.0.0 ;; esac
    plan "(Node.js would come from the distribution: assuming v$NODEV)"
  fi
  if [ -z "$NODEV" ]; then warn "no Node.js: skipping prettier (HTML/CSS/JS/YAML/Markdown)"; else
    major="${NODEV%%.*}"; PRETTIER=""
    if   [ "$major" -ge 18 ]; then PRETTIER="prettier"
    elif [ "$major" -ge 14 ]; then PRETTIER="prettier@3.0.3"
    elif [ "$major" -ge 10 ]; then PRETTIER="prettier@2.8.8"; fi
    if [ -z "$PRETTIER" ]; then warn "Node.js $NODEV is too old for prettier (need 10+): skipping"; else
      [ "$PRETTIER" = prettier ] || say "Node.js $NODEV is older than 18: using $PRETTIER, the newest prettier that supports it"
      if [ "$DRY" -eq 1 ]; then plan "npm install $PRETTIER -> $TOOLS"; else
        NC="$(mktemp -d)"   # throw-away npm cache: leave no ~/.npm behind
        npm install --prefix "$TOOLS" --cache "$NC" --no-audit --no-fund --loglevel=error "$PRETTIER" || warn "prettier install failed"
        rm -rf "$NC"
      fi
    fi
  fi

  # ---- Python venv + ruff + sqlformat
  PYV="${KEMPT_FAKE_PYTHON:-}"
  if [ -z "$PYV" ] && have python3; then PYV="$(python3 -c 'import sys;print("%d.%d"%sys.version_info[:2])' 2>/dev/null || true)"; fi
  need_py=()
  if [ -z "$PYV" ]; then need_py+=("$(pkgname python)"); fi
  if [ "$PM" = apt ] && { [ -n "${KEMPT_FAKE_OS:-}" ] || ! python3 -c 'import venv, ensurepip' >/dev/null 2>&1; }; then need_py+=("$(pkgname venv)"); fi
  if [ "${#need_py[@]}" -gt 0 ]; then install_pkgs "ruff / sqlformat (need Python with venv)" "${need_py[@]}" || true; fi
  if [ -z "$PYV" ] && [ "$DRY" -eq 1 ]; then PYV=3.10; fi
  if [ -z "$PYV" ] && have python3; then PYV="$(python3 -c 'import sys;print("%d.%d"%sys.version_info[:2])' 2>/dev/null || true)"; fi
  if [ -z "$PYV" ]; then warn "no Python 3: skipping ruff and sqlformat"
  elif ! ver_ge "$PYV" 3.7; then warn "Python $PYV is too old for ruff (need 3.7+): skipping ruff and sqlformat"
  elif [ "$DRY" -eq 1 ]; then plan "python venv + pip install ruff sqlparse -> $TOOLS/venv"
  elif python3 -c 'import venv, ensurepip' >/dev/null 2>&1; then
    python3 -m venv "$TOOLS/venv" && "$TOOLS/venv/bin/pip" install --quiet --no-cache-dir --disable-pip-version-check ruff sqlparse \
      && mkdir -p "$TOOLS/bin" && ln -sf ../venv/bin/ruff "$TOOLS/bin/ruff" && ln -sf ../venv/bin/sqlformat "$TOOLS/bin/sqlformat" \
      || warn "ruff/sqlformat install failed"
  else warn "python3-venv is not available: skipping ruff and sqlformat"; fi

  # ---- shfmt, clang-format (distribution packages)
  missing_tools=()
  have shfmt || missing_tools+=("$(pkgname shfmt)")
  have clang-format || missing_tools+=("$(pkgname clangfmt)")
  if [ "${#missing_tools[@]}" -gt 0 ]; then install_pkgs "shell and C/Java formatting" "${missing_tools[@]}" || true; fi
fi

# ---------------------------------------------------------------- 3. write the manifest
if [ "$DRY" -eq 0 ]; then
  do_mkdir "$SHARE"
  { echo "# kempt install manifest — used by kempt-uninstall. Do not edit."
    echo "version:$("$PREFIX/bin/kempt" --version 2>/dev/null | awk '{print $2}')"
    echo "prefix:$PREFIX"
    echo "system:${DISTRO_NAME:-$OS} $ARCH"
    # keep entries from a previous install (upgrade) so nothing is orphaned
    [ -f "$MANIFEST" ] && grep -E '^(dir|file|pkg|apt|rustup):' "$MANIFEST" || true
    printf '%s\n' "${LOG[@]}"
    echo "file:$MANIFEST"
  } | awk '!seen[$0]++' > "$MANIFEST.new" && mv "$MANIFEST.new" "$MANIFEST"
  if [ "$PLAT" = linux ]; then
    command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$PREFIX/share/applications" 2>/dev/null || true
    command -v gtk-update-icon-cache  >/dev/null 2>&1 && gtk-update-icon-cache -q -t -f "$PREFIX/share/icons/hicolor" 2>/dev/null || true
  fi
fi

say "Done."
case ":$PATH:" in *":$PREFIX/bin:"*) ;; *)
  if [ "$PLAT" = macos ]; then warn "$PREFIX/bin is not on your PATH. Add it:  echo 'export PATH=\"$PREFIX/bin:\$PATH\"' >> ~/.zshrc   (then open a new terminal)"
  else warn "$PREFIX/bin is not on your PATH. Add:  export PATH=\"$PREFIX/bin:\$PATH\"   (Ubuntu adds ~/.local/bin automatically at next login if it exists)"; fi ;;
esac
[ "$DRY" -eq 1 ] && { echo "(dry run: nothing was changed)"; exit 0; }
"$PREFIX/bin/kempt" doctor || true
cat <<EOT

Try:    kempt gui app.log          graphical viewer: find/replace, Fix button, themes
        kempt gui a.json b.json    side-by-side compare with per-line copy buttons
Remove: kempt-uninstall            (add --purge-tools to also remove OS packages / Rust this installer added)
EOT
