"""Backend tests: saved settings (no DB), favicon, table view (sort / duplicates / stats), and large-file timing."""
import http.client, json, os, re, subprocess, sys, tempfile, time, random

BIN = os.environ.get("KEMPT_BIN", "/home/claude/kempt/target/release/kempt")
PASS = FAIL = 0
def check(name, cond, detail=""):
    global PASS, FAIL
    if cond: PASS += 1; print(f"  PASS  {name}")
    else: FAIL += 1; print(f"  FAIL  {name}  {str(detail)[:300]}")

class Server:
    def __init__(self, cfg, files=()):
        env = dict(os.environ, KEMPT_CONFIG_DIR=cfg)
        self.p = subprocess.Popen([BIN, "gui", "--no-open", *files], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=env)
        line = ""
        while "http://" not in line:
            line = self.p.stdout.readline()
            if not line and self.p.poll() is not None: raise RuntimeError(self.p.stderr.read())
        m = re.search(r"http://127.0.0.1:(\d+)/\?t=([0-9a-f]+)", line)
        self.port, self.token = int(m.group(1)), m.group(2)
    def req(self, method, path, body=None, headers=None):
        c = http.client.HTTPConnection("127.0.0.1", self.port, timeout=120)
        h = {"Host": f"127.0.0.1:{self.port}"}; h.update(headers or {})
        data = json.dumps(body) if body is not None else None
        c.request(method, path, data, h); r = c.getresponse(); raw = r.read(); ct = r.getheader("Content-Type"); c.close()
        return r.status, raw, ct
    def api(self, name, body=None, ok=True):
        st, raw, _ = self.req("POST", "/api/" + name, body or {}, {"X-Token": self.token})
        j = json.loads(raw)
        if ok and st != 200: raise AssertionError(f"{name}: {st} {j}")
        return j if ok else (st, j)
    def stop(self): self.p.terminate(); self.p.wait(timeout=5)

d = tempfile.mkdtemp(); cfg = os.path.join(d, "cfg")
P = lambda n: os.path.join(d, n)

print("== saved settings survive a restart (file, not a database)")
s = Server(cfg)
check("empty at first", s.api("state/get") == {})
s.api("state/set", {"patch": {"theme": "light", "dir": "/tmp/somewhere", "recent": ["/a/b.json"], "evil": "x"}})
st = s.api("state/get")
check("values stored", st.get("theme") == "light" and st.get("dir") == "/tmp/somewhere" and st["recent"] == ["/a/b.json"], st)
check("unknown keys are ignored", "evil" not in st)
files = os.listdir(cfg)
check("a single JSON file, nothing else", files == ["state.json"], files)
s.stop()
s = Server(cfg)
st = s.api("state/get")
check("same values after restart", st.get("theme") == "light" and st.get("dir") == "/tmp/somewhere", st)
code, raw, _ = s.req("GET", f"/?t={s.token}")
page = raw.decode()
check("page is served with the saved settings embedded", '"theme":"light"' in page.split("const BOOT")[1][:300], page.split("const BOOT")[1][:160])
s.api("state/set", {"patch": {"theme": None}})
check("null removes a key", "theme" not in s.api("state/get"))
code, raw, ct = s.req("GET", "/favicon.svg")
check("favicon.svg is served (tab icon)", code == 200 and ct == "image/svg+xml" and b"<svg" in raw, (code, ct))
code, raw, ct = s.req("GET", "/favicon.png")
check("favicon.png is served", code == 200 and ct == "image/png" and raw[:4] == b"\x89PNG", (code, ct))
code, raw, ct = s.req("GET", f"/?t={s.token}")
check("page links the icon", b'rel="icon"' in raw)

print("== table view")
csv = P("people.csv")
with open(csv, "w") as f:
    f.write('id,name,city,score\n')
    rows = [("1","Ann","Paris","10"),("2","bob","Rome","9.5"),("3","Cy","Paris","100"),("4","Ann","Oslo",""),("5","Dee","Rome","7"),("6","ann","Paris","10")]
    for r in rows: f.write(",".join(r) + "\n")
    f.write('7,"Smith, J","New ""York""",3\n')
o = s.api("open", {"path": csv})
did = o["info"]["id"]
check("csv is kind table", o["info"]["kind"] == "table", o["info"])
t = s.api("table/open", {"id": did})
check("delimiter sniffed, header names", t["delim"] == "," and t["names"] == ["id", "name", "city", "score"] and t["nrows"] == 7, t)
r = s.api("table/rows", {"id": did, "from": 0, "count": 10})
check("rows parsed with quotes", r["rows"][6]["c"] == ["7", "Smith, J", 'New "York"', "3"], r["rows"][6])
check("row numbers are file line numbers", [x["n"] for x in r["rows"]][:2] == [2, 3])
t = s.api("table/sort", {"id": did, "col": 3, "desc": False})
ids = [x["c"][0] for x in s.api("table/rows", {"id": did, "from": 0, "count": 10})["rows"]]
check("numeric sort ascending, blank last", ids == ["7", "5", "2", "1", "6", "3", "4"], ids)
s.api("table/sort", {"id": did, "col": 3, "desc": True})
ids = [x["c"][0] for x in s.api("table/rows", {"id": did, "from": 0, "count": 10})["rows"]]
check("numeric sort descending, blank still last", ids == ["3", "1", "6", "2", "5", "7", "4"], ids)
s.api("table/sort", {"id": did, "col": 1, "desc": False})
names = [x["c"][1] for x in s.api("table/rows", {"id": did, "from": 0, "count": 10})["rows"]]
check("text sort ignores case", [n.lower() for n in names] == sorted(n.lower() for n in names), names)
t = s.api("table/sort", {"id": did, "col": None})
check("clear sort", t["sort"] is None and s.api("table/rows", {"id": did, "from": 0, "count": 1})["rows"][0]["c"][0] == "1")
t = s.api("table/dups", {"id": did, "col": 2})
check("duplicates in city: Paris x3 + Rome x2", t["dup"]["rows"] == 5 and t["dup"]["groups"] == 2 and t["shown"] == 5, t["dup"])
rows_ = s.api("table/rows", {"id": did, "from": 0, "count": 10})["rows"]
check("grouped together with group size", [x["c"][2] for x in rows_] == ["Paris", "Paris", "Paris", "Rome", "Rome"] and rows_[0]["k"] == 3 and rows_[3]["k"] == 2, rows_)
t = s.api("table/dups", {"id": did, "col": 1, "ci": True})
check("ignore-case duplicates in name (Ann/ann x3)", t["dup"]["rows"] == 3, t["dup"])
t = s.api("table/dups", {"id": did, "col": 1, "ci": False})
check("case-sensitive duplicates in name (Ann x2)", t["dup"]["rows"] == 2, t["dup"])
t = s.api("table/dups", {"id": did, "col": None})
check("no duplicate whole rows -> empty", t["dup"]["rows"] == 0)
t = s.api("table/dups", {"id": did, "off": True})
check("show all rows again", t["dup"] is None and t["shown"] == 7)
stt = s.api("table/stats", {"id": did, "col": 3})
check("column stats", stt["blank"] == 1 and stt["unique"] == 5 and stt["dup_rows"] == 2 and stt["numeric"]["n"] == 6 and stt["numeric"]["sum"] == 139.5 and stt["numeric"]["max"] == 100, stt)
check("full (unclipped) cells for copy", len(s.api("table/rows", {"id": did, "from": 0, "count": 1, "full": True})["rows"]) == 1)
tsv = P("t.tsv"); open(tsv, "w").write("a\tb\n1\t2\n3\t4\n")
t2 = s.api("table/open", {"id": s.api("open", {"path": tsv})["info"]["id"]})
check("tsv uses tab", t2["delim"] == "\t" and t2["ncols"] == 2, t2)
nh = s.api("table/open", {"id": did, "header": False})
check("no header: columns are A, B, C…", nh["names"][:3] == ["A", "B", "C"] and nh["nrows"] == 8, nh)
log = P("pipe.log"); open(log, "w").write("x|y|z\n1|2|3\n")
t3 = s.api("table/open", {"id": s.api("open", {"path": log})["info"]["id"]})
check("any text file can be shown as a table (auto-detect |)", t3["delim"] == "|", t3)

print("== sort kinds")
mc = P("modes.csv")
open(mc, "w").write("""id,txt,num,nat,ver,date,len,freq
1,banana,"$1,200.50",file10,1.10.0,03/11/2026,aaa,x
2,Apple,(5),file2,1.2.9,2026-06-01,b,y
3,cherry,12%,File1,v1.2.10,15 Mar 2025,cccc,x
4,apple,  7 ,file20,2.0,"Dec 25, 2024",dd,z
5,Banana,abc,file3,1.2,,,x
""")
mid = s.api("open", {"path": mc})["info"]["id"]
s.api("table/open", {"id": mid})
def order(col, mode, desc=False):
    m = s.api("table/sort", {"id": mid, "col": col, "desc": desc, "mode": mode})
    return "".join(x["c"][0] for x in s.api("table/rows", {"id": mid, "from": 0, "count": 10})["rows"]), m["sort"]
o, so = order(1, "alpha");   check("alphabetical ignores case (Apple, apple, banana, Banana, cherry)", o == "24153", o)
o, _ = order(1, "lex");      check("lexicographic is exact (Apple, Banana, apple, banana, cherry)", o == "25413", o)
o, _ = order(1, "lex", True); check("lexicographic descending", o == "31452", o)
o, so = order(2, "number"); check("numeric reads $1,200.50 / (5) / 12% / ' 7 '; text last", o == "24315", o)
check("sort reports the mode used", so["mode"] == "number", so)
o, so = order(2, "auto");   check("automatic: mixed column falls back to alphabetical", so["mode"] == "alpha", so)
o, so = order(0, "auto");   check("automatic: all-number column is numeric", so["mode"] == "number", so)
o, _ = order(3, "natural"); check("natural: File1, file2, file3, file10, file20", o == "32514", o)
o, _ = order(3, "alpha");   check("(plain alphabetical puts file10 before file2)", o.index("1") < o.index("2"), o)
o, _ = order(4, "version"); check("version: 1.2, 1.2.9, v1.2.10, 1.10.0, 2.0", o == "52314", o)
o, _ = order(5, "date");    check("date, day first: Dec 2024, Mar 2025, Jun 2026, 03/11/2026 = 3 Nov 2026, blank last", o == "43215", o)
o, _ = order(5, "date_us"); check("date, US month first: 03/11/2026 = 11 March 2026 now sorts before June", o == "43125", o)
o, _ = order(5, "date", True); check("date descending keeps blank last", o[-1] == "5" and o[0] == "1", o)
o, _ = order(6, "length");  check("length: b, dd, aaa, cccc, blank last", o == "24135", o)
o, _ = order(7, "freq", True); check("frequency descending: the three x first", set(o[:3]) == set("135"), o)
o, _ = order(7, "freq", False); check("frequency ascending: singletons first", set(o[:2]) == set("24"), o)
dd_ = s.api("table/dups", {"id": mid, "col": 7})
o2, _ = order(0, "number", True)
check("sorting works inside a duplicates list", o2 == "531", o2)
s.api("table/dups", {"id": mid, "off": True}); s.api("table/sort", {"id": mid, "col": None})

print("== sort by several columns")
mc2 = P("multi.csv")
open(mc2, "w").write("id,city,score\n1,Rome,5\n2,Oslo,9\n3,Rome,9\n4,Oslo,5\n5,Rome,\n6,,3\n")
m2 = s.api("open", {"path": mc2})["info"]["id"]; s.api("table/open", {"id": m2})
def ids2(keys):
    m = s.api("table/sort", {"id": m2, "keys": keys})
    return "".join(x["c"][0] for x in s.api("table/rows", {"id": m2, "from": 0, "count": 10})["rows"]), m
o, m = ids2([{"col": 1, "mode": "alpha"}, {"col": 2, "desc": True, "mode": "number"}])
check("by city, then by score descending (blanks last at each level)", o == "243156", o)
check("meta lists every level, most important first", [k["col"] for k in m["sorts"]] == [1, 2] and m["sort"]["col"] == 1, m["sorts"])
o, _ = ids2([{"col": 2, "mode": "number"}, {"col": 1, "mode": "alpha", "desc": True}])
check("order of the columns matters: by score, then city descending", o == "614325", o)
o, _ = ids2([{"col": 1, "mode": "alpha"}])
check("a single key still works", o == "241356", o)
o, m = ids2([])
check("no keys clears the sort", o == "123456" and m["sort"] is None and m["sorts"] == [], (o, m["sorts"]))
m = s.api("table/sort", {"id": m2, "col": 2, "desc": True, "mode": "number"})
check("legacy {col, desc, mode} still works", m["sort"]["col"] == 2 and len(m["sorts"]) == 1, m)
s.api("table/sort", {"id": m2, "col": None})

print("== search: spaces, quotes and symbols inside one item")
sl = P("find.txt")
open(sl, "w").write('{"status": "ok", "n": 1}\nvalue -1 here\nconnection timeout\nconnection  timeout\nplain line\n')
fid = s.api("open", {"path": sl})["info"]["id"]
def hits(spec):
    j = s.api("search/start", {"id": fid, "spec": spec})["job"]
    while True:
        st = s.api("search/status", {"job": j})
        if st["done"]: return st["count"]
        time.sleep(0.02)
check('"quotes" keep a phrase together (matches the single-space line only)', hits({"q": '"connection timeout"'}) == 1)
check("without quotes it is two items (either word, 2 lines)", hits({"q": "connection timeout"}) == 2)
check("without quotes, AND needs both words (2 lines, double space ok)", hits({"q": "connection timeout", "mode": "and"}) == 2)
check("one-phrase mode: whole text is one item, spaces included", hits({"q": "connection timeout", "phrase": True}) == 1)
check("one-phrase mode keeps quotes literal", hits({"q": '"status": "ok"', "phrase": True}) == 1)
check("one-phrase mode: a leading dash is just a dash", hits({"q": "-1 here", "phrase": True}) == 1)
check('escaped quote \\" searches for a literal quote', hits({"q": '\\"status\\":'}) == 1)
check("plain -1 means: exclude lines containing 1", hits({"q": "-1"}) == 3)
check("escaped \\-1 searches for the text -1", hits({"q": "\\-1"}) == 1)
check("quoted \"-1\" also searches for the text -1", hits({"q": '"-1"'}) == 1)
check("symbols are literal in normal mode: [0-9] is not a pattern (but is in regex mode)", hits({"q": "[0-9]"}) == 0 and hits({"q": "[0-9]", "regex": True}) == 2)

print("== large file")
big = P("big.csv"); N = 1_000_000
random.seed(1)
t0 = time.time()
with open(big, "w") as f:
    f.write("id,name,amount,city\n")
    cities = ["Paris","Rome","Oslo","Lima","Kyiv","Doha"]
    for i in range(N):
        f.write(f"{i},user{random.randint(0,400000)},{random.random()*1000:.2f},{cities[i%6]}\n")
size = os.path.getsize(big)
t0 = time.time(); b = s.api("open", {"path": big}); to = time.time() - t0
bid = b["info"]["id"]
t0 = time.time(); m = s.api("table/open", {"id": bid}); tt = time.time() - t0
check(f"open {size>>20} MB csv ({N:,} rows) fast", to < 5 and tt < 2 and m["nrows"] == N, (to, tt))
print(f"      open {to:.2f}s, table/open {tt:.2f}s")
t0 = time.time(); s.api("table/rows", {"id": bid, "from": 500000, "count": 100}); tr = time.time() - t0
check("random page fetch is instant", tr < 0.5, tr); print(f"      page fetch {tr*1000:.0f} ms")
t0 = time.time(); s.api("table/sort", {"id": bid, "col": 2, "desc": True}); ts = time.time() - t0
top = s.api("table/rows", {"id": bid, "from": 0, "count": 2})["rows"]
check("numeric sort of 1M rows", ts < 8 and float(top[0]["c"][2]) >= float(top[1]["c"][2]), (ts, top))
print(f"      numeric sort 1M rows {ts:.2f}s")
t0 = time.time(); s.api("table/sort", {"id": bid, "col": 1, "desc": False}); ts = time.time() - t0
top = s.api("table/rows", {"id": bid, "from": 0, "count": 3})["rows"]
check("text sort of 1M rows", ts < 15 and top[0]["c"][1].lower() <= top[1]["c"][1].lower() <= top[2]["c"][1].lower(), (ts, top))
print(f"      text sort 1M rows {ts:.2f}s")
t0 = time.time(); dd = s.api("table/dups", {"id": bid, "col": 1}); td = time.time() - t0
check("duplicate listing over 1M rows", td < 8 and dd["dup"]["rows"] > 100000, (td, dd["dup"]))
print(f"      duplicates 1M rows {td:.2f}s -> {dd['dup']['rows']:,} rows in {dd['dup']['groups']:,} groups")
for mode, col in (("natural", 1), ("version", 1), ("lex", 1), ("freq", 1), ("length", 1), ("number", 0)):
    t0 = time.time(); s.api("table/sort", {"id": bid, "col": col, "desc": False, "mode": mode}); tm = time.time() - t0
    check(f"{mode} sort of 1M rows under 10 s", tm < 10, tm); print(f"      {mode} sort 1M rows {tm:.2f}s")
t0 = time.time(); stt = s.api("table/stats", {"id": bid, "col": 2}); tt = time.time() - t0
check("column stats over 1M rows", tt < 5 and stt["numeric"]["n"] == N, (tt, stt))
print(f"      column stats {tt:.2f}s")
rss = int(re.search(r"VmRSS:\s+(\d+)", open(f"/proc/{s.p.pid}/status").read()).group(1)) // 1024
check("server memory stays modest (<400 MB)", rss < 400, rss); print(f"      server RSS {rss} MB")
s.stop()
print(f"\n{PASS} passed, {FAIL} failed")
sys.exit(1 if FAIL else 0)
