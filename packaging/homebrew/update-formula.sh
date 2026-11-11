#!/usr/bin/env bash
# Rewrites packaging/homebrew/kempt.rb for a release: version, download URLs and sha256 of the four tarballs.
#   packaging/homebrew/update-formula.sh VERSION OWNER/REPO DIST_DIR > kempt.rb      (then commit kempt.rb to your homebrew-kempt tap)
set -euo pipefail
VER=${1:?VERSION}; REPO=${2:?OWNER/REPO}; DIST=${3:?DIST_DIR}
HERE=$(cd "$(dirname "$0")" && pwd)
python3 - "$VER" "$REPO" "$DIST" "$HERE/kempt.rb" <<'PY'
import hashlib, re, sys, os
ver, repo, dist, tpl = sys.argv[1:5]
s = open(tpl).read()
def sha(name):
    p = os.path.join(dist, f"kempt-{ver}-{name}.tar.gz")
    if not os.path.exists(p): sys.exit(f"missing {p}")
    return hashlib.sha256(open(p, "rb").read()).hexdigest()
s = s.replace('OWNER', repo.split('/')[0]); s = re.sub(r'github\.com/[^/"]+/kempt', f"github.com/{repo}", s)
s = re.sub(r'version "[^"]*"', f'version "{ver}"', s)
def block(m):
    name = re.search(r'kempt-[\d.]+-([a-z0-9_]+-[a-z0-9_]+)\.tar\.gz', m.group(0)).group(1)
    url = f"https://github.com/{repo}/releases/download/v{ver}/kempt-{ver}-{name}.tar.gz"
    return f'url "{url}"\n      sha256 "{sha(name)}"'
s = re.sub(r'url "[^"]+"\n\s+sha256 "[^"]+"', block, s)
sys.stdout.write(s)
PY
