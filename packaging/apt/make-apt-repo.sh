#!/usr/bin/env bash
# Builds a signed apt repository (static files, host them anywhere: GitHub Pages, S3, any web server).
#   packaging/apt/make-apt-repo.sh DEB_DIR OUT_DIR
# Needs: dpkg-scanpackages (package dpkg-dev), gpg, gzip. Set KEMPT_GPG_KEY to the key id / e-mail of the signing key
# (create one once with: gpg --quick-generate-key "Kempt apt <you@example.com>" ed25519 sign never; add --batch --pinentry-mode loopback
# --passphrase '' for a key without a passphrase, which is what an unattended CI job needs).
# Debs already in OUT_DIR/pool are kept, so each release adds to the repository and old versions stay installable.
set -euo pipefail
DEBS=${1:?usage: make-apt-repo.sh DEB_DIR OUT_DIR}; OUT=${2:?usage: make-apt-repo.sh DEB_DIR OUT_DIR}
KEY=${KEMPT_GPG_KEY:?set KEMPT_GPG_KEY to the signing key id or e-mail}
command -v dpkg-scanpackages >/dev/null || { echo "dpkg-scanpackages not found (apt install dpkg-dev)"; exit 1; }
SUITE=${KEMPT_APT_SUITE:-stable}
mkdir -p "$OUT/pool/main"
for f in "$DEBS"/kempt_*.deb; do [ -e "$OUT/pool/main/$(basename "$f")" ] || cp "$f" "$OUT/pool/main/"; done
cd "$OUT"
D="dists/$SUITE"
rm -rf "$D"; mkdir -p "$D/main"
ARCHS=""
for arch in amd64 arm64; do
  ls pool/main/kempt_*_"$arch".deb >/dev/null 2>&1 || continue
  ARCHS="$ARCHS $arch"
  mkdir -p "$D/main/binary-$arch"
  dpkg-scanpackages --arch "$arch" pool/main > "$D/main/binary-$arch/Packages" 2>/dev/null
  gzip -9nk "$D/main/binary-$arch/Packages"
done
[ -n "$ARCHS" ] || { echo "no kempt_*_amd64.deb / _arm64.deb in $DEBS"; exit 1; }
{
  echo "Origin: Kempt"; echo "Label: Kempt"; echo "Suite: $SUITE"; echo "Codename: $SUITE"
  echo "Date: $(LC_ALL=C date -Ru)"; echo "Architectures:${ARCHS}"; echo "Components: main"
  echo "Description: Kempt apt repository"
  for h in MD5Sum:md5sum SHA256:sha256sum; do
    echo "${h%%:*}:"
    (cd "$D" && find main -type f | LC_ALL=C sort | while read -r f; do
       printf ' %s %s %s\n' "$(${h##*:} "$f" | cut -d' ' -f1)" "$(stat -c %s "$f")" "$f"; done)
  done
} > "$D/Release"
gpg --batch --yes --local-user "$KEY" --clearsign -o "$D/InRelease" "$D/Release"
gpg --batch --yes --local-user "$KEY" --armor --detach-sign -o "$D/Release.gpg" "$D/Release"
gpg --export "$KEY" > kempt-archive-keyring.gpg          # binary keyring: what apt's signed-by= wants
gpg --armor --export "$KEY" > kempt-archive-keyring.asc
echo "Repository written to $OUT ($(ls pool/main | wc -l) packages, signed by $KEY)."
echo "Users add it with:"
echo "  sudo install -d /usr/share/keyrings && curl -fsSL <URL>/kempt-archive-keyring.gpg | sudo tee /usr/share/keyrings/kempt-archive-keyring.gpg >/dev/null"
echo "  echo 'deb [signed-by=/usr/share/keyrings/kempt-archive-keyring.gpg] <URL> $SUITE main' | sudo tee /etc/apt/sources.list.d/kempt.list"
echo "  sudo apt update && sudo apt install kempt"
