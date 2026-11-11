import json, os, stat, subprocess, sys, tempfile
BIN = "/home/claude/kempt/target/release/kempt"
PASS = FAIL = 0


def check(name, cond, detail=""):
    global PASS, FAIL
    if cond:
        PASS += 1
        print(f"  PASS  {name}")
    else:
        FAIL += 1
        print(f"  FAIL  {name}  {detail}")


def run(args, inp=None, **kw):
    return subprocess.run([BIN] + args, input=inp, capture_output=True, **kw)


d = tempfile.mkdtemp()
P = lambda n: os.path.join(d, n)

print("== robustness of in-place formatting")
# corrupt a large file at the very END: original must stay untouched, no temp litter
src = "/tmp/big/big.json"
bad = P("corrupt.json")
with open(src, "rb") as a, open(bad, "wb") as b:
    while True:
        c = a.read(1 << 24)
        if not c:
            break
        b.write(c)
with open(bad, "r+b") as f:
    f.seek(-6, 2)
    f.truncate()          # chop the closing "]}" and more -> invalid
before = os.path.getsize(bad)
r = run(["format", "-q", bad])
check("corrupt 250MB file: exit code 2 and error reported", r.returncode == 2 and b"error" in r.stderr, r.stderr[-200:])
check("corrupt file left byte-identical in size", os.path.getsize(bad) == before)
check("no temp files left behind", [f for f in os.listdir(d) if f.startswith(".kempt-")] == [], str(os.listdir(d)))
os.remove(bad)

# symlink: target is formatted, link stays a link
real = P("real.json"); link = P("link.json")
open(real, "w").write('{"a":1}')
os.symlink(real, link)
r = run(["format", "-q", link])
check("symlink is preserved as a symlink", os.path.islink(link), "link replaced by a regular file")
check("symlink target got formatted", open(real).read() == '{\n  "a": 1\n}\n', open(real).read())

# permissions preserved
pf = P("perm.json"); open(pf, "w").write('{"a":1}'); os.chmod(pf, 0o640)
run(["format", "-q", pf])
check("file mode preserved (0640)", stat.S_IMODE(os.stat(pf).st_mode) == 0o640, oct(os.stat(pf).st_mode))

# read-only directory => error, original untouched
ro = P("ro"); os.mkdir(ro); f = os.path.join(ro, "x.json"); open(f, "w").write('{"a":1}'); os.chmod(ro, 0o500)
if os.geteuid() != 0:
    r = run(["format", "-q", f]); check("read-only dir gives an error, file intact", r.returncode == 2 and open(f).read() == '{"a":1}')
else:
    print("  SKIP  read-only dir test (running as root)")
os.chmod(ro, 0o700)

print("== unusual inputs")
deep = "[" * 200000 + "]" * 200000
r = run(["format", "-", "--lang", "json", "--minify"], inp=deep.encode())
check("200,000-deep nesting handled (no stack overflow)", r.returncode == 0 and r.stdout.strip() == deep.encode())
huge_str = json.dumps({"k": "x" * 50_000_000}).encode()
r = run(["format", "-", "--lang", "json"], inp=huge_str)
check("one 50 MB string value streams through", r.returncode == 0 and len(r.stdout) > 50_000_000)
r = run(["format", "-", "--lang", "json", "--minify"], inp=b'{"k":"\xff\xfe invalid utf8 \xc3"}')
check("invalid UTF-8 inside strings passes through byte-exact", r.returncode == 0 and b"\xff\xfe invalid utf8 \xc3" in r.stdout)
r = run(["format", "-", "--lang", "json"], inp=b'   \n\t ')
check("whitespace-only input -> empty output, ok", r.returncode == 0 and r.stdout == b"")
r = run(["format", "-", "--lang", "json"], inp=b'{"a":1}\r\n{"b":[2]}\r\n')
check("CRLF NDJSON handled", r.returncode == 0 and r.stdout.count(b'"a": 1') == 1 and b"\r" not in r.stdout)
r = run(["format", "-", "--lang", "json"], inp=b'{"a":1e999,"b":-0,"c":123456789012345678901234567890}')
check("big/odd numbers preserved verbatim (no float rounding)", b"1e999" in r.stdout and b"123456789012345678901234567890" in r.stdout and b"-0" in r.stdout)
r = run(["format", "-", "--lang", "xml"], inp=b'<a:r xmlns:a="u" x="1>2" y=\'<\'><a:i>t</a:i></a:r>')
check("XML namespaces + '>' and '<' inside attribute values", r.returncode == 0 and b'x="1>2"' in r.stdout and b"y='<'" in r.stdout and r.stdout.count(b"\n") == 3, r.stdout)
r = run(["format", "-", "--lang", "xml"], inp=b'<r>  <![CDATA[ a ]]>  <!-- c --> </r>')
check("XML CDATA/comment kept", b"<![CDATA[ a ]]>" in r.stdout and b"<!-- c -->" in r.stdout)
r = run(["format", "-", "--lang", "xml"], inp="<r>héllo 世界</r>".encode())
check("XML UTF-8 preserved", "héllo 世界".encode() in r.stdout)

print("== pipes / process behaviour")
r = subprocess.run(f"{BIN} diff /tmp/big/big.log /tmp/big/big_30.log | head -2", shell=True, capture_output=True)
check("diff piped into head exits quietly (no panic/BrokenPipe noise)", b"panicked" not in r.stderr and b"Broken pipe" not in r.stderr and r.stdout.count(b"\n") == 2, r.stderr[-200:])
r = subprocess.run(f"{BIN} view /tmp/big/big.log | head -3", shell=True, capture_output=True)
check("view piped prints plain text to head", r.stdout.count(b"\n") == 3 and b"\x1b" not in r.stdout and not r.stderr, r.stderr[-200:])
r = run(["view", "--plain", "-F", "ERROR", "/tmp/big/big.log"])
nerr = r.stdout.count(b"\n")
check("view --plain -F ERROR filters ~3% of 2.3M lines", 60000 < nerr < 80000, str(nerr))
r = run(["view", "--plain", "/tmp/big/huge.json"], )
check("view of 1 GB minified JSON prints formatted (piped, bounded memory)", r.returncode == 0 and r.stdout.startswith(b"{\n"), r.stdout[:40])

b = P("bin.dat"); open(b, "wb").write(os.urandom(5000) + b"\0" * 10)
r = run(["view", b])
check("binary file refused with a clear message", r.returncode == 2 and b"binary" in r.stderr, r.stderr)

g = P("a.log.gz")
subprocess.run(f"printf 'one\\ntwo ERROR\\nthree\\n' | gzip > {g}", shell=True)
r = run(["view", "--plain", "-F", "ERROR", g])
check("gzip log viewable/filterable", r.stdout == b"two ERROR\n", r.stdout)

print("== diff edge cases")
e1, e2 = P("e1"), P("e2"); open(e1, "w").write(""); open(e2, "w").write("x\n")
r = run(["diff", e1, e2]); check("empty vs non-empty diff works", r.returncode == 1 and b"+x" in r.stdout, r.stdout)
r = run(["diff", e1, e1]); check("empty vs empty identical", r.returncode == 0)
r = run(["diff", "--patch", P("p"), "-w", e1, e2]); check("--patch with -w refused (can't apply exactly)", r.returncode == 2 and b"cannot be combined" in r.stderr, r.stderr)
r = run(["diff", "--mode", "format", e1, e2]); check("bad --mode format on text -> clear error", r.returncode == 2, r.stderr)
a, bq = P("q1.json"), P("q2.json")
open(a, "w").write('{"x":[1,2,3],"y":{"z":null}}'); open(bq, "w").write('{"y":{"z":null},"x":[1,2,3]}')
r = run(["diff", a, bq]); check("JSON equal modulo key order -> exit 0", r.returncode == 0, r.stdout)
r = run(["diff", "--mode", "text", a, bq]); check("--mode text sees them as different", r.returncode == 1)

print(f"\n{PASS} passed, {FAIL} failed")
sys.exit(1 if FAIL else 0)
