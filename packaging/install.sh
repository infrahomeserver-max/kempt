#!/usr/bin/env bash
# Kempt: one-line install or clean reinstall of the newest .deb release (Debian / Ubuntu, amd64 or arm64).
#
#   curl -fsSL https://github.com/infrahomeserver-max/kempt/releases/latest/download/install.sh | bash
#   curl -fsSL https://github.com/infrahomeserver-max/kempt/releases/latest/download/install.sh | bash -s -- --purge
#
#   --purge | --fresh   also delete your Kempt data (settings, notes, calendar, checklist, notes key): a first-run clean install
#   --version vX.Y.Z    install that release instead of the newest
#   --repo OWNER/REPO   use another GitHub repository
#   -h, --help          this text
#
# The download is checked against the release's SHA256SUMS and test-unpacked before the installed copy is touched.
# KEMPT_LIMIT_RATE=500k slows the download (to watch the progress bar). To read this script first:
#   curl -fsSLO <URL>/install.sh ; less install.sh ; bash install.sh
set -euo pipefail

REPO="${KEMPT_REPO:-infrahomeserver-max/kempt}"; TAG=""; PURGE=0
while [ $# -gt 0 ]; do
  case "$1" in
    --purge|--fresh) PURGE=1 ;;
    --version) TAG="${2:?--version needs a tag such as v0.2.0}"; shift ;;
    --repo) REPO="${2:?--repo needs owner/repo}"; shift ;;
    -h|--help) sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown option: $1 (try --help)" >&2; exit 2 ;;
  esac
  shift
done

TTY=0; [ -t 2 ] && TTY=1
if [ "$TTY" = 1 ]; then B=$'\033[1;34m'; E=$'\033[1;31m'; Z=$'\033[0m'; else B=""; E=""; Z=""; fi
say() { printf '%s==>%s %s\n' "$B" "$Z" "$*" >&2; }
die() { printf '%serror:%s %s\n' "$E" "$Z" "$*" >&2; exit 1; }

CURL_PID=""; TMP=""
cursor_on() { if [ "$TTY" = 1 ]; then printf '\033[?25h' >&2; fi; }
cleanup() { cursor_on; if [ -n "$TMP" ]; then rm -rf "$TMP"; fi; return 0; }
on_signal() { if [ -n "$CURL_PID" ]; then kill "$CURL_PID" 2>/dev/null || true; fi; cursor_on; printf '\n' >&2; cleanup; exit 130; }
trap cleanup EXIT; trap on_signal INT TERM

# ---- progress bar --------------------------------------------------------------------------------------
UTF=0; case "${LC_ALL:-${LC_CTYPE:-${LANG:-}}}" in *UTF-8*|*utf-8*|*UTF8*|*utf8*) UTF=1 ;; esac
human() { awk -v b="$1" 'BEGIN{ if (b>=1073741824) printf "%.1f GB", b/1073741824; else if (b>=1048576) printf "%.1f MB", b/1048576; else if (b>=1024) printf "%.0f KB", b/1024; else printf "%d B", b }'; }
mmss() { awk -v s="$1" 'BEGIN{ s=int(s); printf "%d:%02d", s/60, s%60 }'; }
bar() {  # bar PERCENT*100 WIDTH  ->  █████▍░░░░  (sub-cell precision, eighths)
  local p="$1" w="$2" units full part i out=""
  units=$(( p * w * 8 / 10000 )); full=$(( units / 8 )); part=$(( units % 8 ))
  [ "$full" -gt "$w" ] && full="$w" && part=0
  if [ "$UTF" = 1 ]; then
    local partial=( "" "▏" "▎" "▍" "▌" "▋" "▊" "▉" ) fill="█" rest="░"
    for ((i = 0; i < full; i++)); do out+="$fill"; done
    local used="$full"; if [ "$part" -gt 0 ] && [ "$full" -lt "$w" ]; then out+="${partial[$part]}"; used=$((full + 1)); fi
    for ((i = used; i < w; i++)); do out+="$rest"; done
  else
    for ((i = 0; i < full; i++)); do out+="#"; done
    for ((i = full; i < w; i++)); do out+="-"; done
  fi
  printf '%s' "$out"
}
now() { if [ -n "${EPOCHREALTIME:-}" ]; then echo "${EPOCHREALTIME/,/.}"; else date +%s.%N; fi; }
fsize() { stat -c %s "$1" 2>/dev/null || echo 0; }

# download URL OUTFILE LABEL
download() {
  local url="$1" out="$2" label="$3" total=0 limit=() rc=0 start last size=0 spd=0 pct tick=0
  [ -n "${KEMPT_LIMIT_RATE:-}" ] && limit=(--limit-rate "$KEMPT_LIMIT_RATE")
  total="$(curl -fsSLI "$url" 2>/dev/null | tr -d '\r' | awk 'tolower($1)=="content-length:"{v=$2} END{print v+0}')" || total=0
  [ -z "$total" ] && total=0
  if [ "$total" -gt 0 ]; then say "Downloading $label ($(human "$total"))"; else say "Downloading $label"; fi
  if [ "$TTY" != 1 ]; then
    curl -fL --retry 3 -sS ${limit[@]+"${limit[@]}"} -o "$out" "$url" || { echo "Download failed. Check your connection and try again." >&2; return 1; }
    echo "Downloaded $(human "$(fsize "$out")")" >&2; return 0
  fi
  local cols w; cols="$(tput cols 2>/dev/null || echo 80)"; case "$cols" in ''|*[!0-9]*) cols=80 ;; esac
  w=$(( cols - 60 )); [ "$w" -lt 16 ] && w=16; [ "$w" -gt 60 ] && w=60
  printf '\033[?25l' >&2
  curl -fL --retry 3 -sS ${limit[@]+"${limit[@]}"} -o "$out" "$url" & CURL_PID=$!
  start="$(now)"; last="$start"; local prev=0 spin='|/-'$'\\' now_t
  while kill -0 "$CURL_PID" 2>/dev/null; do
    sleep 0.1
    size="$(fsize "$out")"; now_t="$(now)"
    spd="$(awk -v o="$spd" -v s="$size" -v p="$prev" -v n="$now_t" -v l="$last" 'BEGIN{ dt=n-l; if (dt<=0) dt=0.001; d=s-p; if (d<0) d=0; r=d/dt; if (o==0) o=r; printf "%.0f", 0.85*o+0.15*r }')"
    prev="$size"; last="$now_t"; tick=$((tick + 1))
    if [ "$total" -gt 0 ]; then
      pct=$(( size * 10000 / total )); [ "$pct" -gt 10000 ] && pct=10000
      local eta="--:--"; [ "$spd" -gt 0 ] && eta="$(mmss "$(awk -v t="$total" -v s="$size" -v v="$spd" 'BEGIN{ x=(t-s)/v; if (x<0) x=0; printf "%d", x }')")"
      printf '\r%s  %3d%%  %s / %s  %s/s  ETA %s\033[K' "$(bar "$pct" "$w")" $(( pct / 100 )) "$(human "$size")" "$(human "$total")" "$(human "$spd")" "$eta" >&2
    else
      printf '\r%s  %s  %s/s\033[K' "${spin:$((tick % 4)):1}" "$(human "$size")" "$(human "$spd")" >&2
    fi
  done
  wait "$CURL_PID" || rc=$?; CURL_PID=""
  if [ "$rc" -ne 0 ]; then printf '\033[?25h\n' >&2; echo "Download failed. Check your connection and try again." >&2; return 1; fi
  size="$(fsize "$out")"; local secs; secs="$(awk -v n="$(now)" -v s="$start" 'BEGIN{ d=n-s; if (d<0.05) d=0.05; printf "%.1f", d }')"
  local avg; avg="$(awk -v z="$size" -v d="$secs" 'BEGIN{ printf "%.0f", z/d }')"
  printf '\r%s  100%%  %s in %ss  (%s/s)\033[K\n\033[?25h' "$(bar 10000 "$w")" "$(human "$size")" "$secs" "$(human "$avg")" >&2
}

# ---- 1. preflight --------------------------------------------------------------------------------------
for c in curl dpkg dpkg-deb sha256sum tar awk stat; do command -v "$c" >/dev/null 2>&1 || die "'$c' is required (this installer is for Debian and Ubuntu)"; done
if [ "$(id -u)" -eq 0 ]; then SUDO=""; else command -v sudo >/dev/null 2>&1 || die "run as root or install sudo"; SUDO="sudo"; fi
if [ -n "${SUDO_USER:-}" ] && [ "$SUDO_USER" != root ]; then UHOME="$(getent passwd "$SUDO_USER" | cut -d: -f6)"; else UHOME="$HOME"; fi

# ---- 2. architecture -----------------------------------------------------------------------------------
ARCH="$(dpkg --print-architecture)"
case "$ARCH" in amd64|arm64) ;; *) die "unsupported CPU architecture '$ARCH' (Kempt provides amd64 and arm64)" ;; esac

# ---- 3. version: the redirect of /releases/latest names the newest published tag (no API, no token) ------
BASE="${KEMPT_BASE_URL:-}"      # test hook: a URL that serves the release files
if [ -z "$BASE" ]; then
  if [ -z "$TAG" ]; then
    url="$(curl -fsSIL -o /dev/null -w '%{url_effective}' "https://github.com/$REPO/releases/latest")" || die "cannot reach github.com/$REPO"
    TAG="${url##*/}"
    case "$TAG" in v[0-9]*) ;; *) die "$REPO has no published release yet" ;; esac
  fi
  BASE="https://github.com/$REPO/releases/download/$TAG"
fi
VER="${TAG#v}"; [ -n "$VER" ] || die "no version given (use --version vX.Y.Z together with KEMPT_BASE_URL)"
DEB="kempt_${VER}-1_${ARCH}.deb"
say "Kempt $VER for $ARCH"

# ---- 4. download ---------------------------------------------------------------------------------------
TMP="$(mktemp -d)"
curl -fsSL "$BASE/SHA256SUMS" -o "$TMP/SHA256SUMS" 2>/dev/null || die "release ${TAG:-$VER} not found (no SHA256SUMS at $BASE)"
curl -fsSIL -o /dev/null "$BASE/$DEB" 2>/dev/null || die "release ${TAG:-$VER} has no $DEB (no package for $ARCH?)"
download "$BASE/$DEB" "$TMP/$DEB" "$DEB" || exit 1

# ---- 5. verify before anything is installed ------------------------------------------------------------
say "Verifying checksum"
grep -F " $DEB" "$TMP/SHA256SUMS" > "$TMP/one.sum" || true
[ "$(wc -l < "$TMP/one.sum")" -eq 1 ] || die "$DEB is not listed in SHA256SUMS; nothing was installed"
( cd "$TMP" && sha256sum -c one.sum >/dev/null ) || die "checksum mismatch for $DEB; nothing was installed"
dpkg-deb --fsys-tarfile "$TMP/$DEB" | tar -t >/dev/null || die "$DEB is truncated or corrupt; nothing was installed"
dpkg-deb -I "$TMP/$DEB" >/dev/null || die "$DEB has no valid control data; nothing was installed"

# ---- 6. replace the installed version ------------------------------------------------------------------
say "Installing"
pkill -x kempt 2>/dev/null || true
if dpkg -s kempt >/dev/null 2>&1; then say "Removing the installed version"; $SUDO apt-get remove -y kempt >/dev/null; fi
if [ "$PURGE" -eq 1 ]; then
  for d in "${XDG_CONFIG_HOME:-$UHOME/.config}/kempt" "$UHOME/.config/kempt" "$UHOME/.config/autostart/kempt.desktop"; do
    [ -e "$d" ] && { say "Removing $d"; rm -rf "$d"; }
  done
fi
$SUDO apt-get install -y "$TMP/$DEB" >/dev/null
say "Installed $(kempt --version 2>/dev/null || echo "Kempt $VER"). Start it with:  kempt gui"
