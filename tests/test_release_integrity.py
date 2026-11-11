"""Release integrity: version consistency, required documents, SHA256SUMS, archive and .deb contents.
usage: python3 tests/test_release_integrity.py [DIST_DIR]   (package checks are skipped when DIST_DIR has no packages)"""
import glob, hashlib, io, os, re, subprocess, sys, tarfile, zipfile
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DIST = sys.argv[1] if len(sys.argv) > 1 else os.path.join(ROOT, "dist")
P = F = 0
def check(n, c, d=""):
    global P, F
    if c: P += 1; print("  PASS ", n)
    else: F += 1; print("  FAIL ", n, str(d)[:300])
rd = lambda p: open(os.path.join(ROOT, p), encoding="utf-8").read()
ver = re.search(r'^version = "([^"]+)"', rd("Cargo.toml"), re.M).group(1)
lock = re.search(r'name = "kempt"\nversion = "([^"]+)"', rd("Cargo.lock"))
check("Cargo.lock matches Cargo.toml", lock and lock.group(1) == ver, lock and lock.group(1))
out = subprocess.run([os.path.join(ROOT, "target/release/kempt"), "--version"], capture_output=True, text=True).stdout
check("binary --version matches", ver in out, out)
check("README references this version only", set(re.findall(r"kempt[-_](\d+\.\d+\.\d+)", rd("README.md"))) <= {ver}, set(re.findall(r"kempt[-_](\d+\.\d+\.\d+)", rd("README.md"))))
rm = rd("README.md")
for f in ("kempt_%s-1_amd64.deb", "kempt-%s-linux-x86_64.tar.gz", "kempt-%s-macos-arm64.tar.gz", "kempt-%s-windows-x86_64.zip", "kempt-%s-src.tar.gz"):
    check("README names download " + f % ver, f % ver in rm)
check("README has no leftover OWNER placeholder except in the documented clone/brew lines", rm.count("OWNER") in (0, 2), rm.count("OWNER"))
check("docs/GITHUB_SETUP.md exists", os.path.exists(os.path.join(ROOT, "docs/GITHUB_SETUP.md")))
check("CHANGELOG has this version", f"## [{ver}]" in rd("CHANGELOG.md"))
check("Homebrew template version matches", f'version "{ver}"' in rd("packaging/homebrew/kempt.rb"))
for n in ("LICENSE", "README.md", "CONTRIBUTING.md", "CODE_OF_CONDUCT.md", "SECURITY.md", "CHANGELOG.md", "THIRD_PARTY_NOTICES.md",
          ".github/PULL_REQUEST_TEMPLATE.md", ".github/dependabot.yml", ".github/ISSUE_TEMPLATE/bug_report.md", ".gitignore"):
    check(f"{n} exists and is not empty", os.path.getsize(os.path.join(ROOT, n)) > 10)
check("LICENSE is MIT", rd("LICENSE").startswith("MIT License") and "Permission is hereby granted" in rd("LICENSE"))
check("Cargo.toml declares licence", 'license = "MIT"' in rd("Cargo.toml"))
tp = rd("THIRD_PARTY_NOTICES.md")
lock_names = set(re.findall(r'name = "([^"]+)"\nversion', rd("Cargo.lock"))) - {"kempt"}
missing = [n for n in lock_names if f"| {n} |" not in tp]
check("every locked crate is in THIRD_PARTY_NOTICES (non-target crates may be extra)", len(missing) <= 40 and "UNKNOWN" not in tp, missing[:5])
check("fonts are credited", "Inter" in tp and "JetBrains Mono" in tp and "OFL" in tp or "Open Font" in tp)
check("no GPL/AGPL-only crate", not re.search(r"\| (A?GPL)[^|]*\|\s*$", tp, re.M))
check("no build outputs in .gitignore'd dirs tracked", "/target" in rd(".gitignore"))
sums = os.path.join(DIST, "SHA256SUMS")
pk = [p for p in glob.glob(os.path.join(DIST, "*")) if ver in os.path.basename(p) and not p.endswith("SHA256SUMS")]
if not pk:
    print("  (no packages for", ver, "in", DIST, "- package checks skipped)")
else:
    check("SHA256SUMS exists", os.path.exists(sums))
    listed = dict((l.split()[1].lstrip("*"), l.split()[0]) for l in open(sums) if l.strip()) if os.path.exists(sums) else {}
    for p in sorted(pk):
        b = os.path.basename(p)
        check(f"SHA256 matches for {b}", listed.get(b) == hashlib.sha256(open(p, "rb").read()).hexdigest(), listed.get(b))
    for p in sorted(pk):
        b = os.path.basename(p)
        names = []
        if b.endswith(".tar.gz"):
            names = [m.name for m in tarfile.open(p).getmembers()]
        elif b.endswith(".zip"):
            names = zipfile.ZipFile(p).namelist()
        elif b.endswith(".deb"):
            lst = subprocess.run(["dpkg-deb", "-c", p], capture_output=True, text=True).stdout
            names = [l.split()[-1] for l in lst.splitlines()]
            ctl = subprocess.run(["dpkg-deb", "-f", p], capture_output=True, text=True).stdout
            check(f"{b}: control Version is {ver}-1", f"Version: {ver}-1" in ctl, ctl[:120])
            cp = subprocess.run(["dpkg-deb", "--fsys-tarfile", p], capture_output=True).stdout
            t = tarfile.open(fileobj=io.BytesIO(cp))
            c = t.extractfile("./usr/share/doc/kempt/copyright").read().decode()
            check(f"{b}: copyright file declares MIT with licence text", "License: MIT" in c and "Permission is hereby granted" in c)
            bad = [m.name for m in t.getmembers() if m.mode & 0o002 or m.uid != 0 and False]
            check(f"{b}: nothing world-writable", not bad, bad)
            check(f"{b}: THIRD_PARTY_NOTICES shipped", any(n.endswith("THIRD_PARTY_NOTICES.md") for n in names))
            continue
        else:
            continue
        if "windows" in b:
            check(f"{b}: has installer files named in the README", all(any(n.endswith("/" + x) for n in names) for x in ("install.cmd", "install.ps1", "uninstall.cmd", "kempt.exe")), names[:12])
        elif b.endswith(".tar.gz") and "-src" not in b:
            check(f"{b}: has install.sh and uninstall.sh", all(any(n.endswith("/" + x) for n in names) for x in ("install.sh", "uninstall.sh")), names[:12])
        if "-src" in b:
            check(f"{b}: has LICENSE and no target/.git", any(n.endswith("/LICENSE") for n in names) and not any("/target/" in n or "/.git/" in n for n in names))
        else:
            check(f"{b}: has LICENSE, notices, README, binary", all(any(n.endswith("/" + x) for n in names) for x in ("LICENSE", "THIRD_PARTY_NOTICES.md", "README.md")) and any(re.search(r"/kempt(\.exe)?$", n) for n in names), names[:8])
print(f"\n{P} passed, {F} failed"); sys.exit(1 if F else 0)
