#!/usr/bin/env bash
# One safe way to release Kempt.
#
#   scripts/release.sh            # patch release: 0.2.7 -> 0.2.8
#   scripts/release.sh minor      # 0.2.7 -> 0.3.0
#   scripts/release.sh 1.0.0      # an explicit version
#   scripts/release.sh --dry-run  # do all the checks and show what would change, write nothing
#
# Day to day: write what changed under "## [Unreleased]" in CHANGELOG.md, commit and push as usual, wait for CI to go green,
# then run this script from the main branch. It stops with a clear message if anything is not right.
set -euo pipefail

BUMP=patch; DRY=0; YES=0
for a in "$@"; do
  case "$a" in
    --dry-run) DRY=1 ;; --yes|-y) YES=1 ;; -h|--help) sed -n 2,11p "$0"; exit 0 ;;
    patch|minor|major) BUMP="$a" ;; [0-9]*.[0-9]*.[0-9]*) BUMP="$a" ;;
    *) echo "unknown argument: $a (try --help)" >&2; exit 2 ;;
  esac
done
cd "$(git rev-parse --show-toplevel)"
die() { echo "error: $*" >&2; exit 1; }
say() { echo "==> $*"; }

CUR=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
[[ $CUR =~ ^([0-9]+)\.([0-9]+)\.([0-9]+)$ ]] || die "cannot read the version from Cargo.toml"
IFS=. read -r MA MI PA <<<"$CUR"
case "$BUMP" in
  patch) NEW="$MA.$MI.$((PA + 1))" ;; minor) NEW="$MA.$((MI + 1)).0" ;; major) NEW="$((MA + 1)).0.0" ;; *) NEW="$BUMP" ;;
esac
TAG="v$NEW"; say "release $CUR -> $NEW ($TAG)"

# ---- checks: everything that has gone wrong by hand before ----
[[ $(git rev-parse --abbrev-ref HEAD) == main ]] || die "you are on '$(git rev-parse --abbrev-ref HEAD)'; releases are made from main"
git diff --quiet && git diff --cached --quiet || die "you have uncommitted changes. Commit them first (git add -A && git commit -m ...), push, wait for CI, then run this again.
$(git status --short)"
[[ -z $(git ls-files --others --exclude-standard) ]] || die "there are untracked files; add or remove them first:
$(git ls-files --others --exclude-standard)"
git fetch --quiet origin --tags
LOCAL=$(git rev-parse HEAD); REMOTE=$(git rev-parse origin/main)
if [[ $LOCAL != "$REMOTE" ]]; then
  if git merge-base --is-ancestor "$LOCAL" "$REMOTE"; then die "this clone is BEHIND origin/main. Run: git pull --ff-only"
  elif git merge-base --is-ancestor "$REMOTE" "$LOCAL"; then die "you have commits that are not pushed yet. Run: git push, wait for CI to go green, then run this again."
  else die "this clone and origin/main have diverged. Run: git pull --rebase, then push and wait for CI."; fi
fi
git rev-parse -q --verify "refs/tags/$TAG" >/dev/null && die "tag $TAG already exists. Pick another version."
grep -q '^## \[Unreleased\]' CHANGELOG.md || die "CHANGELOG.md has no '## [Unreleased]' section. Add one above the newest version and list what changed."
python3 - <<'PY' || die "the [Unreleased] section of CHANGELOG.md is empty: write what changed first."
import re, sys
s = open("CHANGELOG.md", encoding="utf-8").read()
m = re.search(r"^## \[Unreleased\][^\n]*\n(.*?)(?=^## \[|\Z)", s, re.M | re.S)
body = [l for l in (m.group(1) if m else "").splitlines() if l.strip() and not l.startswith("#")]
sys.exit(0 if body else 1)
PY

# CI must be green on the commit that is about to be released
ci_state() { # prints success / failure / pending / unknown
  command -v gh >/dev/null && gh auth status >/dev/null 2>&1 || { echo unknown; return; }
  local j; j=$(gh run list --commit "$1" --json workflowName,status,conclusion --limit 20 2>/dev/null) || { echo unknown; return; }
  python3 - "$j" <<'PY'
import json, sys
runs = [r for r in json.loads(sys.argv[1]) if r["workflowName"] in ("CI", "Security audit")]
if not runs: print("pending")
elif any(r["status"] != "completed" for r in runs): print("pending")
elif all(r["conclusion"] == "success" for r in runs): print("success")
else: print("failure")
PY
}
wait_ci() { # $1 = commit
  local st; st=$(ci_state "$1")
  case "$st" in
    success) say "CI and Security audit are green on ${1:0:7}"; return 0 ;;
    failure) die "CI failed on ${1:0:7}. Fix it first." ;;
    unknown) if [[ ${CI_OK:-} == 1 ]]; then return 0; fi
             read -r -p "==> Cannot check CI from here. Is CI AND Security audit green on ${1:0:7} on GitHub? Type yes: " r; [[ $r == yes ]] || die "stopped: wait for CI to go green." ;;
    pending) say "waiting for CI on ${1:0:7} ..."; while [[ $(ci_state "$1") == pending ]]; do sleep 20; done; wait_ci "$1" ;;
  esac
}
wait_ci "$LOCAL"

# ---- changes ----
if (( DRY )); then say "dry run: all checks passed. It would bump to $NEW, move [Unreleased] to [$NEW] in CHANGELOG.md, build, commit, push, wait for CI and tag $TAG."; exit 0; fi
TODAY=$(date +%F)
python3 - "$NEW" "$TODAY" <<'PY'
import re, sys
new, today = sys.argv[1:3]
s = open("CHANGELOG.md", encoding="utf-8").read()
s = re.sub(r"^## \[Unreleased\][^\n]*\n", f"## [Unreleased]\n\n## [{new}] - {today}\n", s, count=1, flags=re.M)
open("CHANGELOG.md", "w", encoding="utf-8").write(s)
PY
OLDRE=${CUR//./\\.}
sed -i "0,/^version = \"$OLDRE\"/s//version = \"$NEW\"/" Cargo.toml
for f in install.sh install.ps1 README.md; do [[ -f $f ]] && sed -i "s/$OLDRE/$NEW/g" "$f"; done
say "building (this also updates Cargo.lock)"; cargo build --release 2>&1 | tail -2
BIN="${CARGO_TARGET_DIR:-target}/release/kempt"; [[ $("$BIN" --version) == "kempt $NEW" ]] || die "the built program reports '$("$BIN" --version)', expected 'kempt $NEW'"
LEFT=$(grep -rn --include=*.toml --include=*.sh --include=*.ps1 --include=*.cmd --include=*.md "$OLDRE" . 2>/dev/null | grep -v 'CHANGELOG\|THIRD_PARTY\|target/\|Cargo.lock' || true)
[[ -z $LEFT ]] || { echo "warning: the old version $CUR is still mentioned:"; echo "$LEFT"; }
git --no-pager diff --stat
if (( ! YES )); then read -r -p "==> Commit, push and tag $TAG? [y/N] " r; [[ $r == y || $r == Y ]] || { say "stopped. Undo with: git checkout -- . "; exit 1; }; fi

git add -A && git commit -q -m "Release $NEW" && git push -q origin main
REL=$(git rev-parse HEAD); say "pushed ${REL:0:7}; waiting for CI on it"
wait_ci "$REL"
git tag "$TAG" && git push -q origin "$TAG"
say "tagged $TAG. The Release workflow is now building the packages (about 5 minutes):"
echo "    $(git remote get-url origin | sed -E 's#(git@github.com:|https://github.com/)#https://github.com/#; s#\.git$##')/actions"
echo "    Linux:   curl -fsSL https://github.com/infrahomeserver-max/kempt/releases/latest/download/install.sh | bash"
echo "    Windows: irm https://github.com/infrahomeserver-max/kempt/releases/latest/download/install.ps1 | iex"
