#!/usr/bin/env bash
# Fill in your GitHub account and name before the first push.
#   packaging/set-owner.sh GITHUB_USER "Your Full Name"
set -euo pipefail
U="${1:?usage: set-owner.sh GITHUB_USER \"Your Full Name\"}"; N="${2:?usage: set-owner.sh GITHUB_USER \"Your Full Name\"}"
[[ "$U" =~ ^[A-Za-z0-9-]{1,39}$ ]] || { echo "GitHub user names use letters, digits and hyphens only" >&2; exit 1; }
[[ "$N" != *[\&\\/\|]* ]] || { echo "name must not contain & \\ / |" >&2; exit 1; }
cd "$(dirname "${BASH_SOURCE[0]}")/.."
# src/service.rs deliberately keeps the OWNER/kempt placeholder (it means "update check not configured"): do not touch it
for f in Cargo.toml README.md .github/ISSUE_TEMPLATE/config.yml packaging/homebrew/kempt.rb packaging/PUBLISHING.md; do
  sed -i "s#OWNER#$U#g" "$f"
done
sed -i "s#^Copyright (c) 2026 Kempt contributors#Copyright (c) 2026 $N#" LICENSE
sed -i "s#^Copyright: 2026 Kempt contributors#Copyright: 2026 $N#" packaging/build-deb.sh
echo "Done. Remaining placeholders (should only be src/, tests/ and the update scripts):"
grep -rn "OWNER" --exclude-dir=target --exclude-dir=dist --exclude-dir=.git . | cut -c1-110 || true
