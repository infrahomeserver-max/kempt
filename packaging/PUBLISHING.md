# Publishing Kempt so `apt upgrade` and `brew upgrade` update it

Everything here is static files plus one signature, so GitHub alone is enough.

## One-time setup
1. **Signing key** (proves the packages come from you; keep the private key secret):
   `gpg --batch --pinentry-mode loopback --passphrase '' --quick-generate-key "Kempt apt <you@example.com>" ed25519 sign never`
   then `gpg --export-secret-keys --armor you@example.com | base64 -w0` and save the output as the repository secret **APT_GPG_PRIVATE_KEY**.
2. **GitHub Pages**: Settings → Pages → deploy from branch `gh-pages`, folder `/`. The apt repository will live at `https://infrahomeserver-max.github.io/REPO/apt`.
3. **Homebrew tap** (optional): create an empty repo `infrahomeserver-max/homebrew-kempt`, a token that can push to it, and save it as the secret **HOMEBREW_TAP_TOKEN**.

## Each release
Push a tag (`git tag v0.1.0 && git push --tags`). The Release workflow builds the packages, publishes the GitHub release, then adds the new .deb files to the apt repository and rewrites the Homebrew formula.

## What users run (once)
```
sudo install -d /usr/share/keyrings
curl -fsSL https://infrahomeserver-max.github.io/REPO/apt/kempt-archive-keyring.gpg | sudo tee /usr/share/keyrings/kempt-archive-keyring.gpg >/dev/null
echo 'deb [signed-by=/usr/share/keyrings/kempt-archive-keyring.gpg] https://infrahomeserver-max.github.io/REPO/apt stable main' | sudo tee /etc/apt/sources.list.d/kempt.list
sudo apt update && sudo apt install kempt
```
After that `sudo apt update && sudo apt upgrade` (or the Software Updater) brings every new release. Homebrew: `brew tap infrahomeserver-max/kempt && brew install kempt`, later `brew upgrade`.

## By hand
`KEMPT_GPG_KEY=you@example.com packaging/apt/make-apt-repo.sh dist /path/to/site/apt` builds the same repository locally;
`packaging/homebrew/update-formula.sh 0.1.0 infrahomeserver-max/REPO dist > kempt.rb` writes the formula. `tests/test_packaging.py` checks both against the real `apt`.
