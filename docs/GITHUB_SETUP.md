# Publishing Kempt on GitHub — step by step

This guide takes you from the downloaded source archive to a public repository with a working release. Allow about 30 minutes. Nothing here costs money for a public repository.

**What you need:** a free GitHub account, `git` installed (`git --version` to check; on Ubuntu `sudo apt install git`), and the file `kempt-0.1.0-src.tar.gz`.

---

## Step 1. Unpack and personalise

```bash
mkdir -p ~/projects && cd ~/projects
tar xzf ~/Downloads/kempt-0.1.0-src.tar.gz        # creates the folder "kempt"
cd kempt
bash packaging/set-owner.sh YOUR_GITHUB_USERNAME "Your Full Name"
```
This replaces the `OWNER` placeholder with your GitHub user name in `Cargo.toml`, `README.md`, the issue-template link, the Homebrew formula and `PUBLISHING.md`, and puts your name in the `LICENSE` copyright line and the `.deb` copyright file. It deliberately does not touch `src/service.rs` (there `OWNER/kempt` means "update check not configured"; release builds get the real repository automatically).

If you want a different repository name than `kempt`, rename it in the same places (`grep -rn "/kempt" --exclude-dir=target .`) before continuing.

Optional: set a real contact in `packaging/build-deb.sh` (`Maintainer:` line) instead of `noreply@example.com`.

## Step 2. Create the repository on GitHub

1. Sign in at <https://github.com> and click **+** (top right) → **New repository**.
2. **Owner:** your account. **Repository name:** `kempt`.
3. **Description:** `Open, search, fix, format and compare huge JSON, XML and log files — offline, single binary`.
4. Choose **Public**. (The update check, Releases downloads and free build runners all work best on public repositories.)
5. **Leave all three boxes empty:** do *not* add a README, `.gitignore` or licence. The project already has them, and adding them here causes a conflict on your first push.
6. Click **Create repository**. GitHub shows a page with the repository address, for example `https://github.com/YOUR_GITHUB_USERNAME/kempt.git`.

## Step 3. Sign in from your computer

GitHub no longer accepts your account password for `git push`. Pick one:

- **Easiest — GitHub CLI:** `sudo apt install gh` then `gh auth login` (choose GitHub.com → HTTPS → "Login with a web browser"). `git push` then just works.
- **Personal access token:** GitHub → Settings → Developer settings → Personal access tokens → Fine-grained tokens → create one for this repository with *Contents: Read and write* and *Workflows: Read and write*. Use the token as the password when `git push` asks.
- **SSH key:** follow GitHub's "Connecting to GitHub with SSH" page and use the `git@github.com:YOUR_GITHUB_USERNAME/kempt.git` address in Step 4.

## Step 4. First commit and push

```bash
cd ~/projects/kempt
git init -b main
git config user.name  "Your Full Name"
git config user.email "YOUR_ID+YOUR_GITHUB_USERNAME@users.noreply.github.com"   # see GitHub → Settings → Emails for the exact noreply address
git add .
git status                      # CHECK the list against "Files to upload" below. There must be no target/ or dist/
git commit -s -m "Kempt 0.1.0: first public release"
git remote add origin https://github.com/YOUR_GITHUB_USERNAME/kempt.git
git push -u origin main
```
`-s` adds the `Signed-off-by:` line that CONTRIBUTING.md asks of contributors; use it on your own commits too.

Refresh the repository page: you should see the files and the README.

## Step 5. Switch on the repository's safety settings

In the repository: **Settings**.

| Where | What to do |
|---|---|
| **General → About** (the gear on the repo's front page) | Add topics: `json`, `xml`, `log-viewer`, `formatter`, `rust`, `diff`; tick *Releases* |
| **Code security** (or *Advanced Security*) | Enable **Private vulnerability reporting** (this is the address SECURITY.md points to), **Dependabot alerts**, **Dependabot security updates**, **Secret scanning** and **Push protection** |
| **Actions → General → Workflow permissions** | "Read repository contents" is enough; the release job asks for its own write permission |
| **Branches → Add classic branch protection rule** (or *Rules → Rulesets*) | Branch `main`: require a pull request, and require the CI checks to pass before merging. Optional for a solo project, valuable once others contribute |
| **General → Pull Requests** | Optionally tick *Always suggest updating pull request branches* and *Automatically delete head branches* |

## Step 6. Check the automatic tests

Open the **Actions** tab. Pushing to `main` starts **CI** (builds and tests on Ubuntu 22.04/24.04, Ubuntu ARM, macOS Intel/Apple Silicon and Windows) and **Security audit**.

These workflows have never run on GitHub before, so a first-run failure is normal. Click the red job, read the log, and fix the cause (most often a missing tool name or a runner label). Re-run with *Re-run jobs*. When everything is green you have genuinely verified Windows and macOS builds for the first time.

## Step 7. Make the first release

Do this only once the CI is green.

```bash
git tag -a v0.1.0 -m "Kempt 0.1.0"
git push origin v0.1.0
```
The tag must equal `v` + the version in `Cargo.toml` (the workflow refuses a mismatch). The **Release** workflow then builds all packages natively on GitHub, creates a `SHA256SUMS` file and publishes a GitHub Release named `v0.1.0` with the files attached (about 10–15 minutes). Open **Releases** to check that you see:

- `kempt_0.1.0-1_amd64.deb`, `kempt_0.1.0-1_arm64.deb`
- `kempt-0.1.0-linux-x86_64.tar.gz`, `kempt-0.1.0-linux-aarch64.tar.gz`
- `kempt-0.1.0-macos-arm64.tar.gz`, `kempt-0.1.0-macos-x86_64.tar.gz`
- `kempt-0.1.0-windows-x86_64.zip`
- `SHA256SUMS`

Then edit the release text (pencil icon): paste the 0.1.0 section of `CHANGELOG.md`, and download one package yourself to run the install steps from the README as a final test. Because the release builds set the repository automatically, *Settings → General → Check for updates* in the released program will find future releases on its own.

### Next versions
Edit the version in `Cargo.toml` and `Cargo.lock`, add a section to `CHANGELOG.md`, update version strings in `README.md`, commit, then `git tag -a v0.1.1 -m "Kempt 0.1.1" && git push origin main v0.1.1`.
(`python3 tests/test_release_integrity.py` checks that all version strings agree.)

## Step 8 (optional). `apt update` and `brew upgrade` for users

Follow `packaging/PUBLISHING.md`: create a signing key, add the repository secret `APT_GPG_PRIVATE_KEY`, enable GitHub Pages from the `gh-pages` branch, and optionally create the `homebrew-kempt` tap with a `HOMEBREW_TAP_TOKEN` secret. The release workflow does the rest on the next tag. Not required for people to install Kempt from the Releases page.

## Step 9. Tell people

Pin the repository on your profile and consider enabling **Discussions** (Settings → General → Features) for questions.

---

## Files to upload

Upload **everything in the unpacked source folder**, which is exactly what `git add .` picks up, except the build folders. You never upload packages or the `target/` and `dist/` folders (they are in `.gitignore`); the packages are attached to the Release by the workflow.

```
kempt/
├── .github/
│   ├── ISSUE_TEMPLATE/ bug_report.md, feature_request.md, config.yml
│   ├── PULL_REQUEST_TEMPLATE.md
│   ├── dependabot.yml
│   └── workflows/ ci.yml, audit.yml, release.yml
├── .gitignore
├── CHANGELOG.md
├── CODE_OF_CONDUCT.md
├── CONTRIBUTING.md
├── Cargo.lock                 (keep: it pins exact dependency versions for reproducible builds)
├── Cargo.toml
├── LICENSE
├── README.md
├── SECURITY.md
├── THIRD_PARTY_NOTICES.md
├── deny.toml
├── docs/ GITHUB_SETUP.md      (this guide)
├── install.cmd, install.ps1, install.sh
├── uninstall.cmd, uninstall.ps1, uninstall.sh
├── packaging/
│   ├── PUBLISHING.md, build-deb.sh, build-release.sh, set-owner.sh
│   ├── apt/make-apt-repo.sh
│   ├── homebrew/ kempt.rb, update-formula.sh
│   └── kempt.desktop, kempt.icns, kempt.ico, kempt.png, kempt.svg
├── src/                       (all 25 .rs files)
├── tests/                     (all test and benchmark scripts)
└── ui/index.html
```

**Do not upload:** `target/` (build output, hundreds of MB), `dist/` (packages), `__pycache__/`, `*.log`, your personal `~/.config/kempt` settings or notes, any private keys (`*.gpg`, `*.pem`), or tokens. GitHub's push protection will block known token formats, but do not rely on it.

Before the first push, run these quick checks:
```bash
git status --short | head -50                 # nothing from target/ or dist/
git ls-files | wc -l                          # about 90 files
git ls-files | grep -E "^(target|dist)/" || echo "ok: no build output"
du -sh .git                                   # a few MB, not hundreds
```

## If something goes wrong

| Message | Fix |
|---|---|
| `remote: Repository not found` | the address or user name is misspelled, or you are not signed in (Step 3) |
| `Updates were rejected because the remote contains work that you do not have` | you ticked "Add a README" in Step 2. Delete the repository and recreate it empty, or run `git pull origin main --allow-unrelated-histories` and resolve |
| `refusing to allow ... to create or update workflow ... without workflow scope` | your token lacks the *Workflows* permission; create a new one (Step 3) or use `gh auth login` |
| `remote: error: GH013 ... secret scanning` | the push contains something that looks like a key or token. Remove it from the files **and from history** (`git commit --amend` before the first push), and rotate the secret |
| CI job `ubuntu-22.04-arm` or `ubuntu-24.04-arm` never starts | ARM runners are free only on public repositories |
| Release workflow says "tag … does not match Cargo.toml version" | tag name must be `v` + `Cargo.toml` version; delete the tag (`git push origin :refs/tags/vX`, `git tag -d vX`) and retag |
