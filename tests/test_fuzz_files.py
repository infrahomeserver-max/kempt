"""Feeds damaged, hostile and odd files to the viewer, Fix, Format, search, table view and the `format` command.
Nothing may panic (exit 101 / 'panicked'), hang (>30 s) or get a 5xx; the server must stay usable throughout."""
import http.client, json, os, random, re, subprocess, sys, tempfile, time

BIN = os.environ.get("KEMPT_BIN") or os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "target", "release", "kempt")
PASS = FAIL = 0
def check(name, cond, detail=""):
    global PASS, FAIL
    if cond: PASS += 1; print(f"  PASS  {name}")
    else: FAIL += 1; print(f"  FAIL  {name}  {str(detail)[:400]}")
rnd = random.Random(20261128)
d = tempfile.mkdtemp(); cfg = tempfile.mkdtemp(); env = dict(os.environ, KEMPT_CONFIG_DIR=cfg)

GOOD_JSON = json.dumps({"users": [{"id": i, "name": "n%d" % i, "tags": ["a", "b"], "x": {"y": None, "z": 1.5e10}} for i in range(30)]}, indent=1)
GOOD_XML = "<?xml version='1.0'?><root a='1'>" + "".join(f"<item id='{i}'><n>t{i}</n><!-- c --><![CDATA[x<y]]></item>" for i in range(30)) + "</root>"
GOOD_LOG = "".join(f"2026-11-27 10:{i//60%60:02d}:{i%60:02d}.{i%1000:03d} {['INFO','WARN','ERROR'][i%3]} [t{i%4}] msg {i} {{\"k\": {i}}}\n" for i in range(300))
GOOD_CSV = "a,b,c\n" + "".join(f'{i},"q ""{i}""",x{i}\n' for i in range(50))

def mutate(s: bytes) -> bytes:
    b = bytearray(s)
    for _ in range(rnd.randint(1, 12)):
        if not b: break
        k = rnd.randint(0, 5); i = rnd.randrange(len(b))
        if k == 0: del b[i:i + rnd.randint(1, 40)]
        elif k == 1: b[i:i] = bytes(rnd.choice(b'{}[]<>"\',:\\/&;=\n\r\t \x00\xff\xfe') for _ in range(rnd.randint(1, 6)))
        elif k == 2: b[i] = rnd.randrange(256)
        elif k == 3: b[i:i] = b[max(0, i - 30):i]          # duplicate a piece
        elif k == 4: b = b[:i]                               # truncate
        else: b[i:i] = rnd.choice([b"\xef\xbb\xbf", b"\r\n", b"\xe2\x80\xa8", b"\xed\xa0\x80", b"\xf0\x9f\x98\x80", b"\\u0000", b"\\ud800"])
    return bytes(b)

cases = {}
for i in range(40):
    for ext, good in (("json", GOOD_JSON), ("xml", GOOD_XML), ("log", GOOD_LOG), ("csv", GOOD_CSV)):
        cases[f"m{i}_{ext}.{ext}"] = mutate(good.encode())
cases.update({
    "empty.json": b"", "only_bom.json": b"\xef\xbb\xbf", "nul.log": b"\x00" * 5000, "binary.log": bytes(rnd.randrange(256) for _ in range(200000)),
    "deep.json": b"[" * 20000 + b"]" * 20000, "deep_obj.json": b'{"a":' * 5000 + b"1" + b"}" * 5000, "deep.xml": b"<a>" * 20000 + b"</a>" * 20000,
    "longline.log": b"x" * 6_000_000 + b"\n", "longline.json": b'{"a":"' + b"y" * 4_000_000 + b'"}', "manyempty.log": b"\n" * 2_000_000,
    "crlf.json": GOOD_JSON.replace("\n", "\r\n").encode(), "cr_only.log": GOOD_LOG.replace("\n", "\r").encode(), "utf16.json": GOOD_JSON.encode("utf-16"),
    "latin1.log": "caf\xe9 \xfc\xdf\n".encode("latin-1") * 1000, "bigint.json": b'{"a": ' + b"9" * 3000 + b', "b": 1e999999, "c": -0.0}',
    "dupkeys.json": b'{"a":1,"a":2,"a":{"a":[1,1,1]}}', "trailing.json": b'{"a":1,}', "single.json": b"'single'", "comments.json": b'{/*c*/"a":1//x\n}',
    "ws_only.log": b" \t \r\n" * 1000, "ndjson.json": b'{"a":1}\n{"a":2}\n{bad}\n[1,2\n', "xml_bomb.xml": b'<!DOCTYPE l [<!ENTITY a "aaaaaaaaaa"><!ENTITY b "&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;"><!ENTITY c "&b;&b;&b;&b;&b;&b;&b;&b;&b;&b;">]><l>&c;&c;&c;&c;&c;</l>',
    "xml_ext.xml": b'<!DOCTYPE x [<!ENTITY e SYSTEM "file:///etc/passwd">]><x>&e;</x>', "gz_trunc.log.gz": b"\x1f\x8b\x08\x00\x00\x00\x00\x00\x00\x03" + b"\x00" * 20,
})
paths = {}
for name, data in cases.items():
    p = os.path.join(d, name); open(p, "wb").write(data); paths[name] = p

# --- CLI: kempt format must not panic or hang on any of them
bad = []
for name, p in paths.items():
    if name.endswith((".gz",)): continue
    q = p + ".copy" + os.path.splitext(p)[1]; open(q, "wb").write(open(p, "rb").read())
    try: r = subprocess.run([BIN, "format", q], capture_output=True, timeout=60, env=env)
    except subprocess.TimeoutExpired: bad.append((name, "TIMEOUT")); continue
    if r.returncode not in (0, 1, 2) or b"panicked" in r.stderr: bad.append((name, r.returncode, r.stderr[-200:]))
check("`kempt format` never panics, hangs or crashes on %d odd files" % len(paths), not bad, bad[:5])
bad = []
for name in ("empty.json", "binary.log", "deep.json", "xml_bomb.xml", "xml_ext.xml", "longline.log", "utf16.json"):
    for cmd in ("view", "diff"):
        args = [BIN, cmd, paths[name]] + ([paths["m0_json.json"]] if cmd == "diff" else [])
        try: r = subprocess.run(args, capture_output=True, timeout=60, env=env, stdin=subprocess.DEVNULL)
        except subprocess.TimeoutExpired: bad.append((cmd, name, "TIMEOUT")); continue
        if r.returncode not in (0, 1, 2) or b"panicked" in r.stderr: bad.append((cmd, name, r.returncode, r.stderr[-150:]))
check("`kempt view` / `kempt diff` survive odd files", not bad, bad[:5])
check("XML external entity file was NOT read (no passwd content leaked by format)", b"root:" not in open(os.path.join(d, "xml_ext.xml.copy.xml"), "rb").read() if os.path.exists(os.path.join(d, "xml_ext.xml.copy.xml")) else True)

# --- the server
sp = subprocess.Popen([BIN, "gui", "--no-open"], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
line = ""
while "http://" not in line:
    line = sp.stdout.readline()
    if not line and sp.poll() is not None: sys.exit("server did not start: " + sp.stderr.read())
port, token = re.search(r"127.0.0.1:(\d+)/\?t=([0-9a-f]+)", line).groups(); port = int(port)
def call(name, body, timeout=40):
    c = http.client.HTTPConnection("127.0.0.1", port, timeout=timeout)
    try:
        c.request("POST", "/api/" + name, json.dumps(body), {"Host": f"127.0.0.1:{port}", "X-Token": token, "Content-Type": "application/json"})
        r = c.getresponse(); b = r.read()
        try: return r.status, json.loads(b)
        except Exception: return r.status, b
    finally: c.close()
fails = []; opened = 0; t0 = time.time()
SPECS = [{"q": "a"}, {"q": "(a|b)+c", "regex": True}, {"q": "(((a*)*)*)*b", "regex": True}, {"q": "a{1000}", "regex": True}, {"q": "(?i)ERR", "regex": True}, {"q": "[", "regex": True}, {"q": "\\", "regex": True}, {"q": "x" * 4000}, {"q": "é", "case": True}, {"q": "a b c", "mode": "and"}]
for name, p in paths.items():
    if name.startswith("copy"): continue
    try: st, r = call("open", {"path": p})
    except Exception as e: fails.append((name, "open", repr(e))); continue
    if st >= 500: fails.append((name, "open", st)); continue
    if st != 200: continue
    opened += 1; i = r["info"]["id"]
    ops = [("lines", {"id": i, "from": 0, "count": 50}), ("lines", {"id": i, "from": max(0, r["info"]["lines"] - 3), "count": 50}), ("doc/fix", {"id": i}), ("doc/format", {"id": i, "indent": 2}),
           ("doc/undo", {"id": i}), ("table/open", {"id": i}), ("log/time", {"id": i, "at": "10:30"}), ("log/range", {"id": i, "from": "10:00", "to": "10:10"})]
    for sp_ in rnd.sample(SPECS, 4): ops.append(("search/start", {"id": i, "spec": sp_}))
    for route, body in ops:
        t1 = time.time()
        try: st2, _ = call(route, body)
        except Exception as e: fails.append((name, route, repr(e))); continue
        if st2 >= 500: fails.append((name, route, st2))
        if time.time() - t1 > 25: fails.append((name, route, "slow %.0fs" % (time.time() - t1)))
    call("doc/close", {"id": i})
print(f"  opened {opened} of {len(paths)} files, {time.time()-t0:.0f}s")
check("no 5xx, hang or crash on any operation for any file", not fails, fails[:8])
check("server still running", sp.poll() is None)
st, r = call("open", {"path": os.path.join(d, "m0_log.log")}); check("server still opens a normal file afterwards", st == 200, (st, r))
sp.terminate(); sp.wait(timeout=10)
print(f"{PASS} passed, {FAIL} failed"); sys.exit(1 if FAIL else 0)
