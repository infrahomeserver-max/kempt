"""Packaging tests: the signed apt repository works with the real apt, and the Homebrew formula is filled in correctly."""
import os, sys, shutil, subprocess, tempfile, glob, hashlib, re
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
PASS = FAIL = 0
def check(name, cond, detail=""):
    global PASS, FAIL
    if cond: PASS += 1; print(f"  PASS  {name}")
    else: FAIL += 1; print(f"  FAIL  {name}  {str(detail)[:400]}")
def sh(cmd, **kw): return subprocess.run(cmd, shell=True, capture_output=True, text=True, **kw)

d = tempfile.mkdtemp(); gh = os.path.join(d, "gnupg"); os.makedirs(gh, mode=0o700)
env = dict(os.environ, GNUPGHOME=gh, KEMPT_GPG_KEY="t@example.invalid")
debs = sorted(glob.glob(os.path.join(ROOT, "dist", "kempt_*_amd64.deb")), key=os.path.getmtime)
check("there are packages to publish (run build-release.sh first)", bool(debs))
src = os.path.join(d, "debs"); os.makedirs(src)
for ext in ("amd64", "arm64"):
    c = sorted(glob.glob(os.path.join(ROOT, "dist", f"kempt_*_{ext}.deb")), key=os.path.getmtime)
    if c: shutil.copy(c[-1], src)
ver = re.search(r"kempt_(.+)_amd64", os.path.basename(sorted(glob.glob(src + "/*_amd64.deb"))[0])).group(1)
r = subprocess.run(["gpg", "--batch", "--pinentry-mode", "loopback", "--passphrase", "", "--quick-generate-key", "Kempt test <t@example.invalid>", "ed25519", "sign", "never"], env=env, capture_output=True, text=True)
check("a throw-away signing key can be made", r.returncode == 0, r.stderr)
out = os.path.join(d, "repo"); mk = os.path.join(ROOT, "packaging/apt/make-apt-repo.sh")
r = subprocess.run(["bash", mk, src, out], env=env, capture_output=True, text=True)
check("make-apt-repo.sh builds the repository", r.returncode == 0 and os.path.exists(out + "/dists/stable/InRelease") and os.path.exists(out + "/kempt-archive-keyring.gpg"), r.stdout + r.stderr)
check("it signs Release (InRelease and Release.gpg)", subprocess.run(["gpg", "--verify", out + "/dists/stable/InRelease"], env=env, capture_output=True).returncode == 0 and subprocess.run(["gpg", "--verify", out + "/dists/stable/Release.gpg", out + "/dists/stable/Release"], env=env, capture_output=True).returncode == 0)
rel = open(out + "/dists/stable/Release").read()
check("Release lists both architectures and checksums of every index", "Architectures: amd64 arm64" in rel and "main/binary-amd64/Packages.gz" in rel and "SHA256:" in rel, rel[:300])

if shutil.which("apt-get") and os.geteuid() == 0:
    def apt(repo, key, sub="update"):
        T = os.path.join(d, "apt" + str(abs(hash((repo, key))))); os.makedirs(T + "/state/lists/partial", exist_ok=True); os.makedirs(T + "/cache/archives/partial", exist_ok=True); os.makedirs(T + "/parts", exist_ok=True)
        open(T + "/sources.list", "w").write(f"deb [signed-by={key}] file:{repo} stable main\n")
        o = f"-o Dir::Etc::sourcelist={T}/sources.list -o Dir::Etc::sourceparts={T}/parts -o Dir::State::lists={T}/state/lists -o Dir::Cache={T}/cache -o Debug::NoLocking=1"
        u = sh(f"apt-get {o} update 2>&1"); p = sh(f"apt-cache {o} policy kempt 2>&1")
        return u.stdout, p.stdout
    u, p = apt(out, out + "/kempt-archive-keyring.gpg")
    check("the real apt accepts the signed repository and offers the package", f"Candidate: {ver}" in p, u + p)
    # a second repo with the same files but a different key must be refused
    gh2 = os.path.join(d, "gnupg2"); os.makedirs(gh2, mode=0o700); env2 = dict(env, GNUPGHOME=gh2, KEMPT_GPG_KEY="o@example.invalid")
    subprocess.run(["gpg", "--batch", "--pinentry-mode", "loopback", "--passphrase", "", "--quick-generate-key", "Other <o@example.invalid>", "ed25519", "sign", "never"], env=env2, capture_output=True)
    other = os.path.join(d, "other"); subprocess.run(["bash", mk, src, other], env=env2, capture_output=True)
    u, p = apt(out, other + "/kempt-archive-keyring.gpg")
    check("a repository signed by a different key is refused", "Candidate:" not in p or "Candidate: (none)" in p, u + p)
    # tampering with the package list must be caught
    bad = os.path.join(d, "bad"); shutil.copytree(out, bad)
    pk = bad + "/dists/stable/main/binary-amd64/Packages"; open(pk, "a").write("\n# tampered\n"); sh(f"gzip -9nkf {pk}")
    u, p = apt(bad, bad + "/kempt-archive-keyring.gpg")
    check("a tampered package list is rejected (checksum mismatch)", "Candidate: (none)" in p or "Candidate:" not in p or "Hash Sum mismatch" in u or "mismatch" in u.lower(), u + p)
else:
    print("  SKIP  apt checks (need apt-get and root)")

dist = os.path.join(d, "dist"); os.makedirs(dist)
for name in ("macos-arm64", "macos-x86_64", "linux-x86_64", "linux-aarch64"):
    open(os.path.join(dist, f"kempt-9.8.7-{name}.tar.gz"), "wb").write(name.encode() * 50)
r = subprocess.run(["bash", os.path.join(ROOT, "packaging/homebrew/update-formula.sh"), "9.8.7", "alice/kempt", dist], capture_output=True, text=True)
f = r.stdout
check("the Homebrew formula is filled in (version, owner, urls)", r.returncode == 0 and 'version "9.8.7"' in f and "github.com/alice/kempt/releases/download/v9.8.7/kempt-9.8.7-macos-arm64.tar.gz" in f and "OWNER" not in f, r.stderr + f[:300])
ok = all(hashlib.sha256(n.encode() * 50).hexdigest() in f for n in ("macos-arm64", "macos-x86_64", "linux-x86_64", "linux-aarch64")) and "REPLACE_WITH" not in f
check("with the real sha256 of each tarball", ok, f)
r = subprocess.run(["bash", os.path.join(ROOT, "packaging/homebrew/update-formula.sh"), "9.8.7", "alice/kempt", d], capture_output=True, text=True)
check("a missing tarball is an error, not a half-filled formula", r.returncode != 0 and "missing" in r.stderr, r.stderr)
print(f"\n{PASS} passed, {FAIL} failed"); sys.exit(1 if FAIL else 0)
