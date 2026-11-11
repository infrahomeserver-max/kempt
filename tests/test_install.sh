#!/usr/bin/env bash
# Install / uninstall lifecycle tests. Run from the kempt source dir (needs target/release/kempt).
# usage: tests/test_install.sh [--system] [--tools] [--deb]      (system/deb need root; they modify the machine and restore it)
set -uo pipefail
SRC="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PASS=0; FAIL=0
VER="$(sed -n 's/^version = "\(.*\)"/\1/p' "$SRC/Cargo.toml" | head -1)"
ok()  { PASS=$((PASS+1)); echo "  ok   $1"; }
bad() { FAIL=$((FAIL+1)); echo "  FAIL $1"; [ -n "${2:-}" ] && echo "       $2"; }
chk() { if eval "$2"; then ok "$1"; else bad "$1" "${3:-}"; fi; }
# system-generated desktop caches (mimeinfo.cache, icon-theme.cache) are not ours; ignore them in comparisons
snap() { (cd "$1" && find . -mindepth 1 | grep -v -e '/mimeinfo.cache$' -e '/icon-theme.cache$' | LC_ALL=C sort); }

# a "release bundle" like the one users download: binary + scripts + packaging, no cargo needed
B="$(mktemp -d)"; trap 'rm -rf "$B" "$T" 2>/dev/null' EXIT
cp "$SRC/target/release/kempt" "$SRC/install.sh" "$SRC/uninstall.sh" "$B/"; cp -r "$SRC/packaging" "$B/packaging"
T="$(mktemp -d)"

echo "== per-user install into a clean HOME"
H="$T/home"; mkdir -p "$H"; export HOME="$H"
before="$(snap "$H")"
out="$("$B/install.sh" --user --dry-run 2>&1)"; rc=$?
chk "dry run succeeds" "[ $rc -eq 0 ]"
chk "dry run changes nothing" "[ \"\$(snap $H)\" = \"$before\" ]" "$(snap "$H")"
"$B/install.sh" --user >"$T/inst.log" 2>&1; rc=$?
chk "install exits 0" "[ $rc -eq 0 ]" "$(tail -5 "$T/inst.log")"
chk "binary installed + runs" "[ \"\$($H/.local/bin/kempt --version)\" = 'kempt $VER' ]"
chk "uninstaller installed" "[ -x $H/.local/bin/kempt-uninstall ]"
chk "desktop entry installed with absolute Exec" "grep -q '^Exec=$H/.local/bin/kempt gui %f' $H/.local/share/applications/kempt.desktop"
chk "icon installed" "[ -f $H/.local/share/icons/hicolor/scalable/apps/kempt.svg ]"
chk "manifest lists the binary" "grep -q '^file:$H/.local/bin/kempt\$' $H/.local/share/kempt/manifest.txt"
command -v desktop-file-validate >/dev/null && chk "desktop file validates" "desktop-file-validate $H/.local/share/applications/kempt.desktop"
echo '{"a":1,"b":[1,2]}' > "$T/x.json"; "$H/.local/bin/kempt" format --indent tab "$T/x.json"
chk "installed binary formats JSON" "[ \$(wc -l < $T/x.json) -eq 7 ]"
"$B/install.sh" --user >"$T/inst2.log" 2>&1
chk "re-install (upgrade) works" "grep -q 'upgrading in place' $T/inst2.log"
chk "upgrade does not duplicate manifest entries" "[ \$(grep -c '^file:$H/.local/bin/kempt\$' $H/.local/share/kempt/manifest.txt) -eq 1 ]"
"$H/.local/bin/kempt-uninstall" --dry-run >/dev/null 2>&1
chk "uninstall --dry-run removes nothing" "[ -x $H/.local/bin/kempt ]"
"$H/.local/bin/kempt-uninstall" -y >"$T/un.log" 2>&1; rc=$?
chk "uninstall exits 0" "[ $rc -eq 0 ]" "$(cat "$T/un.log")"
chk "HOME is byte-for-byte back to its starting state" "[ \"\$(snap $H)\" = \"$before\" ]" "left behind: $(snap "$H")"

echo "== existing files in shared directories are left alone"
H="$T/home2"; mkdir -p "$H/.local/bin" "$H/.local/share/applications"; export HOME="$H"
echo keep > "$H/.local/bin/mine"; echo keep > "$H/.local/share/applications/other.desktop"
before="$(snap "$H")"
"$B/install.sh" --user >/dev/null 2>&1; "$H/.local/bin/kempt-uninstall" -y >/dev/null 2>&1
chk "unrelated files kept and nothing else left" "[ \"\$(snap $H)\" = \"$before\" ]" "$(diff <(echo "$before") <(snap "$H"))"

echo "== custom prefix"
P="$T/opt/kempt-test"
"$B/install.sh" --prefix "$P" >/dev/null 2>&1
chk "installed under custom prefix" "[ -x $P/bin/kempt ] && [ -f $P/share/kempt/manifest.txt ]"
"$P/bin/kempt-uninstall" -y >/dev/null 2>&1
chk "custom prefix cleaned (only the pre-existing parent remains)" "[ ! -e $P ]" "$(find "$T/opt" 2>/dev/null)"

echo "== uninstall without a manifest falls back to known paths"
H="$T/home3"; mkdir -p "$H"; export HOME="$H"; before="$(snap "$H")"
"$B/install.sh" --user >/dev/null 2>&1; rm "$H/.local/share/kempt/manifest.txt"
"$B/uninstall.sh" --user -y >/dev/null 2>&1
chk "fallback removes binary, desktop entry, icon" "[ ! -e $H/.local/bin/kempt ] && [ ! -e $H/.local/share/applications/kempt.desktop ] && [ ! -e $H/.local/share/icons/hicolor/scalable/apps/kempt.svg ]" "$(snap "$H")"

echo "== uninstall with nothing installed"
H="$T/home4"; mkdir -p "$H"; export HOME="$H"
out="$("$B/uninstall.sh" -y 2>&1)"; rc=$?
chk "clear message and non-zero exit" "[ $rc -ne 0 ] && echo '$out' | grep -qi 'does not seem to be installed'"

echo "== no-desktop option"
H="$T/home5"; mkdir -p "$H"; export HOME="$H"
"$B/install.sh" --user --no-desktop >/dev/null 2>&1
chk "no desktop entry / icon" "[ ! -e $H/.local/share/applications ] && [ ! -e $H/.local/share/icons ]"
"$H/.local/bin/kempt-uninstall" -y >/dev/null 2>&1; chk "clean afterwards" "[ -z \"\$(snap $H)\" ]" "$(snap "$H")"

echo "== OS detection (simulated systems, --dry-run)"
mkrel() { printf 'ID=%s\nID_LIKE="%s"\nVERSION_ID="%s"\nPRETTY_NAME="%s"\n' "$2" "$3" "$4" "$5" > "$T/os-$1"; echo "$T/os-$1"; }
sim() {  # sim OS ARCH GLIBC OSRELEASE-file [extra args...]  -> prints the dry-run output
  local fos="$1" farch="$2" glibc="$3" rel="$4"; shift 4
  KEMPT_FAKE_OS="$fos" KEMPT_FAKE_ARCH="$farch" KEMPT_FAKE_GLIBC="$glibc" KEMPT_OS_RELEASE="$rel" KEMPT_FAKE_MACVER="${MACV:-14.5}" HOME="$T/simhome" "$B/install.sh" --user --dry-run "$@" 2>&1
}
mkdir -p "$T/simhome"
U2204="$(mkrel u2204 ubuntu debian 22.04 'Ubuntu 22.04.5 LTS')"; U2404="$(mkrel u2404 ubuntu debian 24.04 'Ubuntu 24.04.1 LTS')"
D11="$(mkrel d11 debian '' 11 'Debian GNU/Linux 11 (bullseye)')"; F40="$(mkrel f40 fedora '' 40 'Fedora Linux 40')"
ARCHF="$(mkrel arch arch '' '' 'Arch Linux')"; MINT="$(mkrel mint linuxmint 'ubuntu debian' 21.3 'Linux Mint 21.3')"; ALP="$(mkrel alp alpine '' 3.20 'Alpine Linux')"
o="$(sim Linux x86_64 2.35 "$U2204" --with-tools)"
chk "Ubuntu 22.04: detected as Ubuntu 22.04 / apt / glibc 2.35" "echo \"\$o\" | grep -q 'Ubuntu 22.04.*glibc 2.35\|libc 2.35' && echo \"\$o\" | grep -q 'package manager: apt'" "$o"
chk "Ubuntu 22.04: prebuilt binary is accepted (glibc 2.35 >= 2.34)" "! echo \"\$o\" | grep -qi 'building from source'" "$o"
chk "Ubuntu 22.04: asks apt for python3-venv, nodejs, npm, shfmt, clang-format" "echo \"\$o\" | grep -q 'apt install.*python3-venv' && echo \"\$o\" | grep -q 'nodejs' && echo \"\$o\" | grep -q 'clang-format'" "$o"
chk "Ubuntu 22.04: old Node 12 -> uses prettier@2.8.8" "echo \"\$o\" | grep -q 'prettier@2.8.8'" "$o"
o="$(sim Linux x86_64 2.39 "$U2404" --with-tools)"
chk "Ubuntu 24.04: current prettier" "echo \"\$o\" | grep -q 'npm install prettier ' && ! echo \"\$o\" | grep -q 'prettier@'" "$o"
o="$(sim Linux aarch64 2.35 "$U2204")"
chk "ARM64 Ubuntu detected (arm64)" "echo \"\$o\" | grep -q 'arm64'" "$o"
o="$(sim Linux x86_64 2.31 "$D11" 2>&1)"; rc=$?
chk "Debian 11 (glibc 2.31) bundle: refuses prebuilt binary with a clear reason" "echo \"\$o\" | grep -qi 'glibc 2.34 or newer'" "$o"
cp -r "$SRC/src" "$B/src_tmp" 2>/dev/null; rm -rf "$B/src_tmp"
o="$(cd "$SRC" && KEMPT_FAKE_OS=Linux KEMPT_FAKE_ARCH=x86_64 KEMPT_FAKE_GLIBC=2.31 KEMPT_OS_RELEASE="$D11" HOME="$T/simhome" "$SRC/install.sh" --user --dry-run 2>&1)"
chk "Debian 11 with source tree: falls back to building from source" "echo \"\$o\" | grep -qi 'building from source'" "$o"
o="$(sim Linux x86_64 2.40 "$F40" --with-tools)"
chk "Fedora: dnf with clang-tools-extra" "echo \"\$o\" | grep -q 'package manager: dnf' && echo \"\$o\" | grep -q 'dnf install.*clang-tools-extra'" "$o"
o="$(sim Linux x86_64 2.40 "$ARCHF" --with-tools)"
chk "Arch: pacman with clang" "echo \"\$o\" | grep -q 'package manager: pacman' && echo \"\$o\" | grep -q 'pacman install.*clang'" "$o"
o="$(sim Linux x86_64 2.35 "$MINT" --with-tools)"
chk "Linux Mint recognised as apt-based (ID_LIKE)" "echo \"\$o\" | grep -q 'package manager: apt'" "$o"
o="$(sim Linux x86_64 musl "$ALP")"
chk "Alpine/musl: explains the binary needs glibc" "echo \"\$o\" | grep -qi 'musl'" "$o"
o="$(sim Linux x86_64 2.35 "$U2204" --with-tools --no-system-packages)"
chk "--no-system-packages: no OS package installs, tells you what is needed" "! echo \"\$o\" | grep -q 'apt install' && echo \"\$o\" | grep -qi 'not installing OS packages'" "$o"
o="$(MACV=14.5 sim Darwin arm64 '' "$U2204" --with-tools)"
chk "macOS 14 on Apple Silicon: Homebrew, app bundle, no .desktop file" "echo \"\$o\" | grep -q 'macOS 14.5' && echo \"\$o\" | grep -q 'package manager: brew' && echo \"\$o\" | grep -q 'Kempt.app' && ! echo \"\$o\" | grep -q 'kempt.desktop' && echo \"\$o\" | grep -q 'brew install.*shfmt'" "$o"
o="$(MACV=10.15 sim Darwin x86_64 '' "$U2204" 2>&1)"
chk "macOS 10.15 is rejected (11+ required)" "echo \"\$o\" | grep -qi 'macOS 11 or newer'" "$o"
o="$(sim FreeBSD x86_64 '' "$U2204" 2>&1)"
chk "unsupported OS: points Windows users to install.ps1" "echo \"\$o\" | grep -qi 'unsupported system'" "$o"
o="$(sim Linux riscv64 2.39 "$U2404" 2>&1)"
chk "unsupported CPU is rejected" "echo \"\$o\" | grep -qi 'unsupported CPU'" "$o"

for a in "$@"; do
 case "$a" in
 --tools)
  echo "== --with-tools (real npm/pip/apt) and --purge-tools"
  H="$T/home6"; mkdir -p "$H"; export HOME="$H"; before="$(snap "$H")"
  "$B/install.sh" --user --with-tools >"$T/tools.log" 2>&1
  tail -12 "$T/tools.log"
  echo '<p>hi</p><div><span>x</span></div>' > "$T/t.html"; printf 'def f( a,b ):\n  return a+b\n' > "$T/t.py"
  # PATH without any other prettier/ruff/sqlformat, only node: proves kempt finds its private copies
  mkdir -p "$T/nodeonly"; ln -sf "$(command -v node)" "$T/nodeonly/node"
  FPATH="/usr/bin:/bin:$T/nodeonly"
  chk "prettier found only in private dir (not on PATH)" "! PATH=$FPATH command -v prettier >/dev/null"
  chk "prettier works from the private tools dir" "PATH=$FPATH $H/.local/bin/kempt format $T/t.html >/dev/null 2>&1 && [ \$(wc -l < $T/t.html) -gt 1 ]"
  chk "ruff works from the private tools dir" "PATH=$FPATH $H/.local/bin/kempt format $T/t.py >/dev/null 2>&1 && grep -q 'def f(a, b):' $T/t.py"
  chk "tools live inside the prefix" "[ -d $H/.local/share/kempt/tools ]"
  installed_apt="$(sed -n 's/^apt://p' "$H/.local/share/kempt/manifest.txt" | tr '\n' ' ')"
  echo "  (apt packages added by installer: ${installed_apt:-none})"
  "$H/.local/bin/kempt-uninstall" -y --purge-tools >"$T/un2.log" 2>&1
  chk "uninstall --purge-tools leaves HOME as it was" "[ \"\$(snap $H)\" = \"$before\" ]" "$(diff <(echo "$before") <(snap "$H") | head)"
  for p in $installed_apt; do chk "apt package $p removed again" "! dpkg -s $p >/dev/null 2>&1"; done ;;
 --ps)
  echo "== Windows installer (PowerShell, file logic in test mode)"
  PW="${PWSH:-$(command -v pwsh || true)}"
  if [ -z "$PW" ]; then echo "  (pwsh not found: skipped)"; else
   export DOTNET_SYSTEM_GLOBALIZATION_INVARIANT=1 KEMPT_TEST_NOWIN=1
   WB="$T/winbundle"; mkdir -p "$WB" "$T/winroot/Programs"; echo keep > "$T/winroot/Programs/other.txt"
   cp "${WIN_EXE:-$SRC/target/release/kempt}" "$WB/kempt.exe"; cp "$SRC/install.ps1" "$SRC/uninstall.ps1" "$SRC/uninstall.cmd" "$SRC/README.md" "$SRC/packaging/kempt.ico" "$WB/"
   wb="$(snap "$T/winroot")"
   "$PW" -NoProfile -File "$WB/install.ps1" -Prefix "$T/winroot/Programs/Kempt" -DryRun >/dev/null 2>&1
   chk "PowerShell dry run changes nothing" "[ \"\$(snap $T/winroot)\" = \"$wb\" ]"
   "$PW" -NoProfile -File "$WB/install.ps1" -Prefix "$T/winroot/Programs/Kempt" >"$T/ps1.log" 2>&1
   chk "install.ps1 installs exe, uninstaller, icon and manifest" "[ -f $T/winroot/Programs/Kempt/kempt.exe ] && [ -f $T/winroot/Programs/Kempt/uninstall.ps1 ] && [ -f $T/winroot/Programs/Kempt/kempt.ico ] && grep -q '^file:.*kempt.exe' $T/winroot/Programs/Kempt/manifest.txt" "$(tail -3 "$T/ps1.log")"
   "$PW" -NoProfile -File "$WB/install.ps1" -Prefix "$T/winroot/Programs/Kempt" >"$T/ps2.log" 2>&1
   chk "install.ps1 upgrade in place, no duplicate manifest lines" "grep -q 'upgrading' $T/ps2.log && [ \$(grep -c '^file:.*kempt.exe' $T/winroot/Programs/Kempt/manifest.txt) -eq 1 ]"
   "$PW" -NoProfile -File "$T/winroot/Programs/Kempt/uninstall.ps1" -Yes >"$T/ps3.log" 2>&1
   chk "uninstall.ps1 restores the folder exactly (other files kept)" "[ \"\$(snap $T/winroot)\" = \"$wb\" ]" "$(diff <(echo "$wb") <(snap "$T/winroot"))"
  fi ;;
 --system)
  echo "== system install into /usr/local (root)"
  before="$(snap /usr/local)"
  "$B/install.sh" --system >/dev/null 2>&1
  chk "installed to /usr/local/bin" "[ -x /usr/local/bin/kempt ] && kempt --version | grep -q $VER"
  kempt-uninstall -y >/dev/null 2>&1
  chk "/usr/local restored exactly" "[ \"\$(snap /usr/local)\" = \"$before\" ]" "$(diff <(echo "$before") <(snap /usr/local))" ;;
 --deb)
  echo "== .deb package (dpkg)"
  DEB="$("$SRC/packaging/build-deb.sh" "$SRC/target/release/kempt" "$T")"
  chk "deb lints (dpkg-deb --info)" "dpkg-deb -I $DEB >/dev/null"
  before="$(snap /usr/share | grep -v 'icon-theme.cache' )"; binb="$(ls /usr/bin | wc -l)"
  dpkg -i "$DEB" >"$T/dpkg.log" 2>&1; chk "dpkg -i succeeds" "[ $? -eq 0 ]" "$(tail -3 "$T/dpkg.log")"
  chk "kempt on PATH and runs" "[ \"\$(/usr/bin/kempt --version)\" = 'kempt $VER' ]"
  chk "listed as installed" "dpkg -s kempt | grep -q 'Status: install ok installed'"
  dpkg -r kempt >/dev/null 2>&1; chk "dpkg -r removes the binary" "[ ! -e /usr/bin/kempt ]"
  dpkg -P kempt >/dev/null 2>&1; chk "dpkg -P purges" "! dpkg -s kempt >/dev/null 2>&1"
  after="$(snap /usr/share | grep -v 'icon-theme.cache')"
  chk "/usr/share back to the starting state" "[ \"$after\" = \"$before\" ]" "$(diff <(echo "$before") <(echo "$after") | head)"
  chk "/usr/bin back to the starting count" "[ $(ls /usr/bin | wc -l) -eq $binb ]" ;;
 esac
done
echo; echo "install tests: $PASS passed, $FAIL failed"; [ $FAIL -eq 0 ]
