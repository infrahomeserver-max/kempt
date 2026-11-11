#!/usr/bin/env bash
# Kempt uninstaller (Linux and macOS): removes exactly what install.sh recorded, then any directories it
# created (only if empty). Your own files and other programs' files are never touched.
#
#   kempt-uninstall                 uninstall (finds the install by itself)
#   ./uninstall.sh --user|--system|--prefix DIR     pick a specific install
#   --purge-settings  also delete Kempt's saved settings (theme, last folder, recent files: one small file)
#   --purge-tools   also remove OS packages / Rust that the installer added (apt, dnf, pacman, zypper, brew, rustup)
#   --dry-run       show what would be removed
#   -y              do not ask
set -euo pipefail

PREFIX=""; DRY=0; YES=0; PURGE=0; PSET=0
while [ $# -gt 0 ]; do
  case "$1" in
    --user) PREFIX="$HOME/.local" ;;
    --system) PREFIX=/usr/local ;;
    --prefix) PREFIX="${2:?--prefix needs a directory}"; shift ;;
    --prefix=*) PREFIX="${1#*=}" ;;
    --purge-tools) PURGE=1 ;;
    --purge-settings) PSET=1 ;;
    --dry-run) DRY=1 ;;
    -y|--yes) YES=1 ;;
    -h|--help) sed -n '2,10p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
  shift
done
say() { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33mwarning:\033[0m %s\n' "$*" >&2; }

# portable "readlink -f" (macOS' readlink has no -f on older versions)
resolve() {
  local p="$1" d
  while [ -L "$p" ]; do
    d="$(readlink "$p")"
    case "$d" in /*) p="$d" ;; *) p="$(dirname "$p")/$d" ;; esac
  done
  printf '%s/%s\n' "$(cd "$(dirname "$p")" && pwd -P)" "$(basename "$p")"
}
SELF="$(resolve "${BASH_SOURCE[0]}")"

# locate the manifest: explicit prefix > next to this script (installed copy) > the usual places
M=""
if [ -n "$PREFIX" ]; then M="$PREFIX/share/kempt/manifest.txt"
else
  for c in "$(dirname "$SELF")/manifest.txt" "$HOME/.local/share/kempt/manifest.txt" /usr/local/share/kempt/manifest.txt; do
    [ -f "$c" ] && { M="$c"; break; }
  done
fi
FALLBACK=""
if [ -z "$M" ] || [ ! -f "$M" ]; then
  if [ -n "$PREFIX" ]; then
    # no manifest: fall back to the well-known paths of that prefix
    say "No manifest in $PREFIX; removing the standard Kempt files"
    FALLBACK="$(mktemp -d)"; M="$FALLBACK/manifest.txt"
    { echo "prefix:$PREFIX"
      for f in bin/kempt bin/kempt-uninstall share/applications/kempt.desktop share/icons/hicolor/scalable/apps/kempt.svg share/kempt/uninstall.sh; do echo "file:$PREFIX/$f"; done
      echo "dir:$PREFIX/share/kempt"; } > "$M"
  else
    echo "Kempt does not seem to be installed here (no manifest found). If it came from the .deb package use:  sudo apt remove kempt" >&2
    exit 1
  fi
fi
PREFIX="$(sed -n 's/^prefix://p' "$M" | head -1)"; [ -n "$PREFIX" ] || PREFIX="$(dirname "$(dirname "$(dirname "$M")")")"
SHARE="$PREFIX/share/kempt"

# need root?
if [ "$DRY" -eq 0 ] && [ ! -w "$PREFIX/bin" ] && [ "$(id -u)" -ne 0 ]; then
  XA=(); [ "$PSET" -eq 1 ] && XA+=(--purge-settings); [ "$PURGE" -eq 1 ] && XA+=(--purge-tools)
  exec sudo env "HOME=$HOME" "$SELF" --prefix "$PREFIX" -y ${XA[@]+"${XA[@]}"}
fi

FILES=(); while IFS= read -r l; do FILES+=("$l"); done < <(sed -n 's/^file://p' "$M")
DIRS=();  while IFS= read -r l; do DIRS+=("$l");  done < <(sed -n 's/^dir://p' "$M" | awk '{print length($0) "\t" $0}' | sort -rn | cut -f2-)
PKGS=();  while IFS= read -r l; do PKGS+=("$l");  done < <( { sed -n 's/^pkg://p' "$M"; sed -n 's/^apt:\(.*\)/apt:\1/p' "$M"; } )
RUSTUP="$(sed -n 's/^rustup://p' "$M" | head -1)"
nf=${#FILES[@]}; np=${#PKGS[@]}

# where Kempt keeps its one settings file (see "Saved settings" in the README)
if [ "$(uname -s)" = Darwin ]; then CFG="$HOME/Library/Application Support/kempt"; else CFG="${XDG_CONFIG_HOME:-$HOME/.config}/kempt"; fi
[ -n "${KEMPT_CONFIG_DIR:-}" ] && CFG="$KEMPT_CONFIG_DIR"

say "Kempt in $PREFIX will be removed:"
if [ "$nf" -gt 0 ]; then for f in "${FILES[@]}"; do if [ -e "$f" ] || [ -L "$f" ]; then echo "  $f"; fi; done; fi
if [ -d "$SHARE/tools" ]; then echo "  $SHARE/tools/   (helper formatters installed with --with-tools)"; fi
pkg_list=""; if [ "$np" -gt 0 ]; then for p in "${PKGS[@]}"; do pkg_list="$pkg_list ${p#pkg:}"; done; fi
if [ "$PURGE" -eq 1 ]; then
  [ -n "$pkg_list" ] && echo "  OS packages:$pkg_list"
  [ -n "$RUSTUP" ] && echo "  rustup (~/.cargo, ~/.rustup)"
elif [ -n "$pkg_list" ] || [ -n "$RUSTUP" ]; then
  echo "  (kept, installed earlier by the installer:$pkg_list${RUSTUP:+ rustup} — add --purge-tools to remove)"
fi
if [ "$PSET" -eq 1 ] && [ -d "$CFG" ]; then echo "  $CFG/   (saved settings)"; fi
if [ "$DRY" -eq 1 ]; then echo "(dry run: nothing was removed)"; [ -n "$FALLBACK" ] && rm -rf "$FALLBACK"; exit 0; fi
if [ "$YES" -eq 0 ] && [ -t 0 ]; then read -r -p "Continue? [y/N] " a; case "$a" in [Yy]*) ;; *) echo aborted; exit 1;; esac; fi

# files first (the manifest itself and this script last), then the private tools dir, then empty dirs
if [ "$nf" -gt 0 ]; then
  for f in "${FILES[@]}"; do
    case "$f" in "$M"|"$SHARE/uninstall.sh") continue;; esac
    rm -f -- "$f"
  done
fi
rm -rf -- "$SHARE/tools"
if [ "$PSET" -eq 1 ] && [ -d "$CFG" ]; then rm -f -- "$CFG/state.json" "$CFG/state.json.tmp"; rmdir -- "$CFG" 2>/dev/null || true; fi
if [ "$PURGE" -eq 1 ]; then
  if [ "$np" -gt 0 ]; then
    for p in "${PKGS[@]}"; do
      e="${p#pkg:}"; pm="${e%%:*}"; name="${e#*:}"
      case "$pm" in
        apt)    apt-get remove -y "$name" >/dev/null 2>&1 || sudo apt-get remove -y "$name" >/dev/null 2>&1 || warn "could not remove $pm package $name" ;;
        dnf)    dnf remove -y "$name" >/dev/null 2>&1 || sudo dnf remove -y "$name" >/dev/null 2>&1 || warn "could not remove $pm package $name" ;;
        pacman) pacman -Rs --noconfirm "$name" >/dev/null 2>&1 || sudo pacman -Rs --noconfirm "$name" >/dev/null 2>&1 || warn "could not remove $pm package $name" ;;
        zypper) zypper --non-interactive remove "$name" >/dev/null 2>&1 || sudo zypper --non-interactive remove "$name" >/dev/null 2>&1 || warn "could not remove $pm package $name" ;;
        brew)   brew uninstall "$name" >/dev/null 2>&1 || warn "could not remove $pm package $name" ;;
      esac
    done
  fi
  if [ -n "$RUSTUP" ] && command -v rustup >/dev/null 2>&1; then rustup self uninstall -y >/dev/null 2>&1 || warn "rustup self uninstall failed"; fi
fi
rm -f -- "$M" "$SHARE/uninstall.sh"
# cache files written by update-desktop-database / gtk-update-icon-cache would keep a directory we created alive
only_cache() { [ -d "$1" ] && [ "$(ls -A "$1")" = "$2" ]; }
if [ "${#DIRS[@]}" -gt 0 ]; then
  for d in "${DIRS[@]}"; do
    case "$d" in
      */share/applications) only_cache "$d" mimeinfo.cache && rm -f -- "$d/mimeinfo.cache" ;;
      */icons/hicolor)      only_cache "$d" icon-theme.cache && rm -f -- "$d/icon-theme.cache" ;;
    esac
    rmdir -- "$d" 2>/dev/null || true
  done
fi
rmdir -- "$SHARE" 2>/dev/null || true
if [ "$(uname -s)" = Linux ]; then
  # refresh desktop caches only where the directory still exists (running them on a missing dir would re-create it)
  [ -d "$PREFIX/share/applications" ] && command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$PREFIX/share/applications" 2>/dev/null || true
  [ -d "$PREFIX/share/icons/hicolor" ] && command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q -t -f "$PREFIX/share/icons/hicolor" 2>/dev/null || true
fi
[ -n "$FALLBACK" ] && rm -rf "$FALLBACK"
say "Kempt has been removed."
if [ "$PSET" -eq 0 ] && [ -f "$CFG/state.json" ]; then
  echo "Your saved settings (theme, last folder, recent files) were kept in $CFG — add --purge-settings to delete them."
fi
