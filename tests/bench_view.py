import os, sys, time
sys.path.insert(0, os.path.dirname(__file__))
from tui_harness import Session

B = "/tmp/big"


def mem(pid):
    d = {}
    for l in open(f"/proc/{pid}/status"):
        if l.split(":")[0] in ("VmRSS", "VmHWM", "RssAnon", "RssFile"):
            d[l.split(":")[0]] = int(l.split()[1]) / 1024
    return d


def wait_for(s, needle, timeout=60):
    t = time.time()
    while time.time() - t < timeout:
        s.pump(0.02)
        if needle in s.text():
            return time.time() - t
    return None


def report(tag, t):
    print(f"  {tag:58s} {'TIMEOUT' if t is None else f'{t * 1000:8.0f} ms'}", flush=True)


print("== VIEWER on big.log (325 MB, 2.3M lines)")
t0 = time.time()
s = Session(["view", f"{B}/big.log"], cols=140, rows=40)
t = wait_for(s, "request 1 handled")
report("open -> first screen painted", t)
m = mem(s.pid)
print(f"      memory right after open: anon {m['RssAnon']:.0f} MB, file-backed (page cache, reclaimable) {m['RssFile']:.0f} MB")
s.pump(1.5)
print(f"      status after indexing: {s.line(39).strip()[:100]}")

t = time.time(); s.send("G", 0.0); r = wait_for(s, "request 2299999 handled"); report("G (jump to last line)", r)
t = time.time(); s.send("g", 0.0); r = wait_for(s, "request 1 handled"); report("g (jump to first line)", r)
s.send(":1700000\r", 0.0); r = wait_for(s, "request 1699999 handled"); report("goto line 1,700,000", r)

s.send("g", 0.3)
# search for something rare near the end -> forces long scan
s.send("/request 2250001 handled\r", 0.0); r = wait_for(s, "request 2250001 handled"); report("search 'request 2250001 handled' (scan ~98% of file)", r)
r = wait_for(s, " match", 10)
# wait for match-count thread
t = time.time()
while time.time() - t < 30 and "matches" not in s.line(39) and "match" not in s.line(39):
    s.pump(0.1)
print(f"      status: {s.line(39).strip()[:120]}")

s.send("&", 0.2); s.send("\x15", 0.1)
s.send("ERROR\r", 0.0)
t = time.time()
while time.time() - t < 60:
    s.pump(0.05)
    st = s.line(39)
    if "filter /ERROR/" in st and "…" not in st:
        break
report("filter 'ERROR' over whole file -> complete", time.time() - t if "…" not in s.line(39) else None)
print(f"      status: {s.line(39).strip()[:120]}")
s.send("&", 0.2); s.send("\x15", 0.1); s.send("\r", 0.5)
s.send("t2026-11-03 12:00:00\r", 0.0); r = wait_for(s, "2026-11-03 12:00:00"); report("timestamp jump to 12:00:00 (binary search)", r)
m = mem(s.pid)
print(f"      memory after all operations: anon {m['RssAnon']:.0f} MB (index+filter+UI), peak total incl. page cache {m['VmHWM']:.0f} MB")
s.send("q", 0.2); s.wait_exit(); s.kill()

print("\n== VIEWER on big.json (250 MB single-line minified -> formatted on open)")
s = Session(["view", f"{B}/big.json"], cols=140, rows=40)
t = wait_for(s, '"data": [', 120)
report("open (stream-format 250MB to temp, then show)", t)
print(f"      status: {s.line(39).strip()[:100]}")
m = mem(s.pid)
print(f"      memory: anon {m['RssAnon']:.0f} MB, file-backed {m['RssFile']:.0f} MB")
s.send("G", 0.0); r = wait_for(s, "]", 10); report("G to the end of the formatted view", r)
s.send("q", 0.2); s.wait_exit(); s.kill()

print("\n== VIEWER --raw on big.json (one 250 MB line) stays responsive")
s = Session(["view", "--raw", f"{B}/big.json"], cols=140, rows=40)
t = wait_for(s, '{"data":[{"id":0', 30)
report("open raw single 250MB line -> first screen", t)
s.send("l" * 50, 0.5)
print(f"      scrolled sideways OK: {'yes' if s.text() else 'no'}")
s.send("q", 0.2); s.wait_exit(); s.kill()
