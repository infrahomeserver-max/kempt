#!/usr/bin/env bash
# Kempt: clean install of the newest .deb release (Debian / Ubuntu, amd64 or arm64).
#
#   curl -fsSL https://github.com/infrahomeserver-max/kempt/releases/latest/download/install-deb.sh | bash -s -- --purge
#
#   --purge        also delete your Kempt data (settings, notes, calendar, checklist, notes key): a first-run clean install
#   --version TAG  install that release (for example v0.2.0) instead of the newest
#   --repo O/R     use another GitHub repository (default below)
#   -h, --help     this text
#
# It checks the download against the release's SHA256SUMS and test-unpacks the package before touching the
# installed copy. Prefer to read it first?  curl -fsSLO <URL>/install-deb.sh ; less install-deb.sh ; bash install-deb.sh
set -euo pipefail

REPO="${KEMPT_REPO:-infrahomeserver-max/kempt}"; TAG=""; PURGE=0
while [ $# -gt 0 ]; do
  case "$1" in
    --purge) PURGE=1 ;;
    --version) TAG="${2:?--version needs a tag such as v0.2.0}"; shift ;;
    --repo) REPO="${2:?--repo needs owner/repo}"; shift ;;
    -h|--help) sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown option: $1 (try --help)" >&2; exit 2 ;;
  esac
  shift
done
say() { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
die() { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

# 1. preflight
for c in curl dpkg dpkg-deb sha256sum tar; do command -v "$c" >/dev/null 2>&1 || die "'$c' is required (this installer is for Debian and Ubuntu)"; done
if [ "$(id -u)" -eq 0 ]; then SUDO=""; else command -v sudo >/dev/null 2>&1 || die "run as root or install sudo"; SUDO="sudo"; fi
# the person's own home, even when started with sudo (that is where Kempt keeps its data)
if [ -n "${SUDO_USER:-}" ] && [ "$SUDO_USER" != root ]; then UHOME="$(getent passwd "$SUDO_USER" | cut -d: -f6)"; else UHOME="$HOME"; fi

# 2. architecture
ARCH="$(dpkg --print-architecture)"
case "$ARCH" in amd64|arm64) ;; *) die "unsupported CPU architecture '$ARCH' (Kempt provides amd64 and arm64)" ;; esac

# 3. version: the redirect of /releases/latest names the newest published tag
BASE="${KEMPT_BASE_URL:-}"      # test hook: a directory that holds the release files
if [ -z "$BASE" ]; then
  if [ -z "$TAG" ]; then
    url="$(curl -fsSIL -o /dev/null -w '%{url_effective}' "https://github.com/$REPO/releases/latest")" || die "cannot reach github.com/$REPO"
    TAG="${url##*/}"
    case "$TAG" in v[0-9]*) ;; *) die "$REPO has no published release yet" ;; esac
  fi
  BASE="https://github.com/$REPO/releases/download/$TAG"
fi
VER="${TAG#v}"
[ -n "$VER" ] || die "no version given (set --version when using KEMPT_BASE_URL)"
DEB="kempt_${VER}-1_${ARCH}.deb"
say "Installing Kempt $VER ($ARCH)"

# 4. download into a temporary folder that is removed on exit
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
curl -fsSL "$BASE/SHA256SUMS" -o "$TMP/SHA256SUMS" || die "could not download SHA256SUMS from $BASE"
curl -fsSL "$BASE/$DEB" -o "$TMP/$DEB" || die "could not download $DEB (does this release have a .deb for $ARCH?)"

# 5. verify before anything is installed
say "Verifying"
( cd "$TMP" && grep -F " $DEB" SHA256SUMS | sha256sum -c - ) || die "checksum mismatch or $DEB missing from SHA256SUMS; nothing was installed"
dpkg-deb --fsys-tarfile "$TMP/$DEB" | tar -t >/dev/null || die "$DEB is corrupt; nothing was installed"
dpkg-deb -I "$TMP/$DEB" >/dev/null || die "$DEB has no valid control data; nothing was installed"

# 6. remove the old version (settings stay unless --purge)
pkill -x kempt 2>/dev/null || true
if dpkg -s kempt >/dev/null 2>&1; then say "Removing the installed version"; $SUDO apt-get remove -y kempt >/dev/null; fi

# 7. optional clean slate
if [ "$PURGE" -eq 1 ]; then
  say "Deleting your Kempt data"
  rm -rf "${XDG_CONFIG_HOME:-$UHOME/.config}/kempt" "$UHOME/.config/kempt" "$UHOME/.config/autostart/kempt.desktop"
fi

# 8. install (apt resolves the dependencies)
$SUDO apt-get install -y "$TMP/$DEB"
say "Installed: $(kempt --version)"
echo "Start it with:  kempt gui"
