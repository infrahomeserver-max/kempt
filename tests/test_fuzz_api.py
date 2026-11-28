"""Throws junk at every API route: wrong types, huge numbers, odd paths, special files, many threads at once.
The server must never hang, crash or end up unable to serve a normal request afterwards (a panic that poisons a lock would show here)."""
import http.client, json, os, random, re, subprocess, sys, tempfile, threading, time

BIN = os.environ.get("KEMPT_BIN") or os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "target", "release", "kempt")
PASS = FAIL = 0
def check(name, cond, detail=""):
    global PASS, FAIL
    if cond: PASS += 1; print(f"  PASS  {name}")
    else: FAIL += 1; print(f"  FAIL  {name}  {str(detail)[:300]}")

cfg = tempfile.mkdtemp(); d = tempfile.mkdtemp()
env = dict(os.environ, KEMPT_CONFIG_DIR=cfg)
log = os.path.join(d, "a.log"); js = os.path.join(d, "b.json"); xm = os.path.join(d, "c.xml")
open(log, "w").write("".join(f"2026-11-27 10:{i//60%60:02d}:{i%60:02d} {'ERROR' if i%9==0 else 'INFO'} line {i}\n" for i in range(3000)))
open(js, "w").write(json.dumps({"a": [1, 2, {"b": None}], "c": "x" * 50}))
open(xm, "w").write("<r><a x='1'>t</a><b/></r>")
p = subprocess.Popen([BIN, "gui", "--no-open", log, js, xm], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
line = ""
while "http://" not in line:
    line = p.stdout.readline()
    if not line and p.poll() is not None: sys.exit("server did not start: " + p.stderr.read())
port, token = re.search(r"127.0.0.1:(\d+)/\?t=([0-9a-f]+)", line).groups(); port = int(port)

def call(name, body, timeout=20, raw=None):
    c = http.client.HTTPConnection("127.0.0.1", port, timeout=timeout)
    try:
        c.request("POST", "/api/" + name, raw if raw is not None else json.dumps(body), {"Host": f"127.0.0.1:{port}", "X-Token": token, "Content-Type": "application/json"})
        r = c.getresponse(); b = r.read()
        try: return r.status, json.loads(b)
        except Exception: return r.status, b
    finally: c.close()

st, j = call("startup", {})
ids = [t["info"]["id"] for t in j["tabs"] if t.get("kind") == "doc"]
check("three documents open", len(ids) == 3, ids)

ROUTES = """calendar/get calendar/set diff/apply diff/apply_all diff/apply_hunk diff/close diff/edit diff/find diff/nav diff/new diff/opts diff/rank
diff/rows diff/ruler diff/state diff/text diff/undo doc/close doc/fix doc/format doc/redo doc/rename doc/replace doc/save doc/set doc/text doc/undo
file/new fs gui/open info lines log/range log/time notes/get notes/set open recent/info search/rank search/ruler search/start search/status
search/step startup state/get state/set table/dups table/filter table/open table/rows table/sort table/stats table/values text/format tools/status
update/check workspace""".split()
JUNK = [None, True, -1, 0, 2**63, 2**64, -2**63, 1.5e300, "", "x" * 5000, "../../../etc/passwd", "\u0000", "%00", "a\nb", "𝕏" * 50, [], [None], {}, {"a": {"b": [1]}}, [[[]]], "/dev/zero", "/proc/self/mem", "/", "~", "C:\\Windows\\System32", "-rf", "$(id)", "`id`", "<script>alert(1)</script>"]
KEYS = ["id", "sid", "path", "paths", "from", "to", "at", "count", "start", "end", "line", "q", "spec", "text", "name", "dir", "files", "mode", "indent", "col", "dir", "key", "value", "rows", "n", "limit", "offset", "cid", "wait", "hash", "regex", "pattern", "replace", "old", "new", "view", "tabs", "id1", "id2", "a", "b", "kind", "sort", "desc", "filter", "filters", "version", "repo"]
rnd = random.Random(1234)
bad_status = []
t0 = time.time(); n = 0
for route in ROUTES:
    if route in ("service/quit", "quit", "doc/close"): continue
    for i in range(24):
        body = {k: rnd.choice(JUNK) for k in rnd.sample(KEYS, rnd.randint(0, 6))}
        if i % 3 == 0 and ids: body["id"] = rnd.choice(ids)
        if i % 5 == 0: body["id"] = rnd.choice(JUNK)
        try: st, _ = call(route, body, timeout=25)
        except Exception as e: bad_status.append((route, "EXC " + repr(e), body)); continue
        n += 1
        if st >= 500 and route != "update/check": bad_status.append((route, st, str(body)[:120]))   # update/check answers 502 when offline, by design
print(f"  sent {n} junk requests in {time.time()-t0:.1f}s")
check("no 5xx / hang from junk input on any route", not bad_status, bad_status[:6])
check("server process still running", p.poll() is None)
# raw junk bodies
for raw in [b"", b"{", b"[]", b"null", b"\xff\xfe", b"{" * 100000, b'{"id":' + b"9" * 5000 + b"}", b'{"a":' * 3000 + b"1" + b"}" * 3000]:
    try: st, _ = call("info", None, raw=raw.decode("latin-1") if isinstance(raw, bytes) else raw)
    except Exception as e: st = repr(e)
    if not (isinstance(st, int) and st < 500): check("raw body %r handled" % raw[:12], False, st)
check("raw junk bodies handled without 5xx", True)
# the server still does real work (no poisoned lock)
st, r = call("lines", {"id": ids[0], "from": 0, "count": 3}); check("lines still served after the fuzz", st == 200 and len(r["lines"]) == 3, (st, r))
st, r = call("search/start", {"id": ids[0], "spec": {"q": "ERROR"}}); check("search still starts", st == 200, (st, r))
st, r = call("log/time", {"id": ids[0], "at": "10:10"}); check("log/time still works", st == 200 and "line" in r, (st, r))
st, r = call("state/get", {}); check("state still readable", st == 200)
# special files must be refused, not read
for path in ["/dev/zero", "/dev/null", "/dev/urandom", "/proc/self/mem", "/proc/kcore", d]:
    try: st, r = call("open", {"path": path}, timeout=15)
    except Exception as e: st, r = "HANG/EXC", repr(e)
    ok_ = st in (400, 404) or (st == 200 and isinstance(r, dict) and r.get("info", {}).get("lines") == 0 and path.startswith("/proc/"))   # /proc files look like empty regular files
    check("open %s is refused or empty (got %s)" % (path, st), ok_, (st, r))
if hasattr(os, "mkfifo"):
    ff = os.path.join(d, "fifo"); os.mkfifo(ff)
    try: st, r = call("open", {"path": ff}, timeout=10)
    except Exception as e: st, r = "HANG", repr(e)
    check("a FIFO is refused, not waited on (got %s)" % st, st == 400, r)
    st, r = call("info", {"id": ids[0]}, timeout=10); check("server still answers after the FIFO", st == 200)
# concurrency: mixed requests from many threads
errs = []
def worker(k):
    r2 = random.Random(k)
    for _ in range(40):
        route = r2.choice(["lines", "search/start", "search/status", "info", "log/time", "state/get", "recent/info", "table/rows", "doc/text"])
        body = {"id": r2.choice(ids), "from": r2.randint(0, 3000), "count": r2.randint(1, 50), "spec": {"q": r2.choice(["ERROR", "line 1", "zzz", "(a"])}, "at": "10:05", "text": "x"}
        try:
            st, _ = call(route, body, timeout=30)
            if st >= 500: errs.append((route, st))
        except Exception as e: errs.append((route, repr(e)))
ts = [threading.Thread(target=worker, args=(k,)) for k in range(16)]
t1 = time.time(); [t.start() for t in ts]; [t.join(120) for t in ts]
check("16 threads x 40 mixed requests: no errors (%.1fs)" % (time.time() - t1), not errs and not any(t.is_alive() for t in ts), errs[:4])
st, r = call("lines", {"id": ids[1], "from": 0, "count": 2}); check("still healthy after the concurrency run", st == 200)
# slow / stalled connections must not stop other clients
import socket
socks = []
for _ in range(40):
    s = socket.create_connection(("127.0.0.1", port)); s.sendall(b"POST /api/info HTTP/1.1\r\nHost: 127.0.0.1:%d\r\nContent-Length: 100\r\n\r\n" % port); socks.append(s)
t2 = time.time()
try: st, r = call("info", {"id": ids[0]}, timeout=20)
except Exception as e: st = repr(e)
check("40 stalled connections do not block a normal request (%.1fs)" % (time.time() - t2), st == 200, st)
for s in socks: s.close()
check("process alive at the end", p.poll() is None)
p.terminate(); p.wait(timeout=10)
print(f"{PASS} passed, {FAIL} failed"); sys.exit(1 if FAIL else 0)
