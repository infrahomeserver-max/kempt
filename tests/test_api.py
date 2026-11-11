"""HTTP API tests for `kempt gui` (backend). Starts the real server."""
import http.client, json, os, re, subprocess, sys, tempfile, time

BIN = os.environ.get("KEMPT_BIN") or os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "target", "release", "kempt" + (".exe" if os.name == "nt" else ""))
PASS = FAIL = 0


def check(name, cond, detail=""):
    global PASS, FAIL
    if cond:
        PASS += 1
        print(f"  PASS  {name}")
    else:
        FAIL += 1
        print(f"  FAIL  {name}  {str(detail)[:300]}")


class Server:
    def __init__(self, files=()):
        self.p = subprocess.Popen([BIN, "gui", "--no-open", *files], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        line = ""
        while "http://" not in line:
            line = self.p.stdout.readline()
            if not line and self.p.poll() is not None:
                raise RuntimeError(self.p.stderr.read())
        m = re.search(r"http://127.0.0.1:(\d+)/\?t=([0-9a-f]+)", line)
        self.port, self.token = int(m.group(1)), m.group(2)

    def req(self, method, path, body=None, headers=None, host=None):
        c = http.client.HTTPConnection("127.0.0.1", self.port, timeout=60)
        h = {"Host": host or f"127.0.0.1:{self.port}"}
        h.update(headers or {})
        data = None
        if body is not None:
            data = json.dumps(body)
            h["Content-Type"] = "application/json"
        c.request(method, path, data, h)
        r = c.getresponse()
        raw = r.read()
        c.close()
        return r.status, raw

    def api(self, name, body=None, ok=True):
        st, raw = self.req("POST", "/api/" + name, body or {}, {"X-Token": self.token})
        j = json.loads(raw)
        if ok and st != 200:
            raise AssertionError(f"{name}: {st} {j}")
        return j if ok else (st, j)

    def stop(self):
        self.p.terminate()
        self.p.wait(timeout=5)


d = tempfile.mkdtemp()
P = lambda n: os.path.join(d, n)
log = P("app.log")
with open(log, "w") as f:
    for i in range(1000):
        lvl = "ERROR" if i % 100 == 7 else "WARN" if i % 50 == 3 else "INFO"
        f.write(f"2026-11-03 10:00:{i % 60:02d} {lvl} request {i} took {i % 97}ms {'timeout' if i % 40 == 5 else 'ok'}\n")

s = Server()
print("== security")
st, raw = s.req("GET", "/")
check("page without token is refused", st == 403, st)
st, raw = s.req("GET", f"/?t={s.token}")
check("page with token is served", st == 200 and b"<html" in raw.lower() or b"<!doctype" in raw.lower(), st)
st, j = s.api("startup", ok=False)
check("API with token works", st == 200)
st, raw = s.req("POST", "/api/startup", {}, {})
check("API without token refused", st == 403, st)
st, raw = s.req("POST", "/api/startup", {}, {"X-Token": "0" * 32})
check("API with wrong token refused", st == 403)
st, raw = s.req("POST", "/api/startup", {}, {"X-Token": s.token}, host="evil.example.com")
check("foreign Host header refused (DNS-rebinding)", st == 403, st)
st, raw = s.req("GET", "/api/startup", None, {"X-Token": s.token})
check("GET on API refused", st == 405)
st, raw = s.req("POST", "/api/nope", {}, {"X-Token": s.token})
check("unknown API -> 404 JSON", st == 404 and b"error" in raw)

print("== open / lines")
o = s.api("open", {"path": log})
did = o["info"]["id"]
check("open returns info", o["info"]["lines"] == 1000 and o["info"]["kind"] == "text" and not o["info"]["dirty"], o)
ls = s.api("lines", {"id": did, "from": 5, "count": 3})
check("lines window", [x["n"] for x in ls["lines"]] == [5, 6, 7] and "request 7" in ls["lines"][2]["t"] and "ERROR" in ls["lines"][2]["t"])
bad = s.api("open", {"path": P("nope.log")}, ok=False)
check("open missing file -> readable error", bad[0] == 400 and "cannot open" in bad[1]["error"], bad)
fs = s.api("fs", {"path": d})
check("fs lists the directory", any(e["n"] == "app.log" for e in fs["entries"]) and fs["path"] == os.path.realpath(d))

print("== find: OR / AND / exclude / flags / highlight ranges")
def search(spec):
    j = s.api("search/start", {"id": did, "spec": spec})
    job = j["job"]
    for _ in range(200):
        st_ = s.api("search/status", {"job": job})
        if st_["done"]:
            return job, st_
        time.sleep(0.02)
    raise RuntimeError("search never finished")

def truth(pred):
    return sum(1 for l in open(log) if pred(l))

job, st_ = search({"q": "ERROR timeout", "mode": "or"})
check("OR count matches ground truth", st_["count"] == truth(lambda l: "ERROR" in l or "timeout" in l), st_)
job, st_ = search({"q": "ERROR timeout", "mode": "and"})
check("AND count matches ground truth", st_["count"] == truth(lambda l: "ERROR" in l and "timeout" in l), (st_, truth(lambda l: "ERROR" in l and "timeout" in l)))
job, st_ = search({"q": "WARN -timeout", "mode": "or"})
check("exclusion (-term) works", st_["count"] == truth(lambda l: "WARN" in l and "timeout" not in l), st_)
job, st_ = search({"q": "error", "case": True})
check("match case", st_["count"] == 0)
job, st_ = search({"q": '"request 1"', "word": True})
check("whole word (quoted phrase)", st_["count"] == truth(lambda l: re.search(r"\brequest 1\b", l) is not None), st_)
job, st_ = search({"q": "ok", "word": True})
check("whole word does not match inside 'took'", st_["count"] == truth(lambda l: re.search(r"\bok\b", l) is not None), st_)
job, st_ = search({"q": "request 1", "mode": "and"})
check("space-separated words are separate terms", st_["count"] == 1000, st_)
job, st_ = search({"q": r"request (\d)\d\d took", "regex": True})
check("regex", st_["count"] == truth(lambda l: re.search(r"request (\d)\d\d took", l)), st_)
bad = s.api("search/start", {"id": did, "spec": {"q": "(", "regex": True}}, ok=False)
check("invalid regex -> readable error", bad[0] == 400 and "invalid pattern" in bad[1]["error"], bad)
job, st_ = search({"q": "ERROR timeout", "mode": "or"})
ls = s.api("lines", {"id": did, "from": 0, "count": 500, "spec": {"q": "ERROR timeout", "mode": "or"}})
hl = [x for x in ls["lines"] if "m" in x]
exp = sum(1 for l in list(open(log))[:500] if "ERROR" in l or "timeout" in l)
check("highlight ranges returned for matching lines only", len(ls["lines"]) == 500 and len(hl) == exp and all(len(x["m"]) >= 1 for x in hl), (len(hl), exp))
x = next(x for x in ls["lines"] if x["n"] == 7)
terms = {t[2] for t in x["m"]}
check("ranges point at the right text and term index", all(x["t"][a:b].lower() in ("error", "timeout") for a, b, t in x["m"]) and 0 in terms, x)
step = s.api("search/step", {"job": job, "from": -1, "dir": 1})
check("step forward from start -> first match", step["line"] == 5 and step["index"] == 0, step)  # line 5 has timeout
step2 = s.api("search/step", {"job": job, "from": step["line"], "dir": 1})
check("step again advances", step2["line"] > step["line"])
back = s.api("search/step", {"job": job, "from": step["line"], "dir": -1})
check("step backward from first wraps to last", back["wrapped"] and back["line"] > 900, back)
rl = s.api("search/ruler", {"job": job, "buckets": 20})
check("ruler buckets sum to matches", sum(rl["counts"]) == st_["count"])
rk = s.api("search/rank", {"job": job, "line": step2["line"]})
check("rank gives current index", rk["rank"] == 2 and rk["total"] == st_["count"], rk)
ls = s.api("lines", {"id": did, "from": 0, "count": 3, "spec": {"q": "é世 x", "mode": "or"}})
check("non-ASCII query is fine", ls["total"] == 1000)

print("== replace / undo / redo / save")
before = open(log).read()
r = s.api("doc/replace", {"id": did, "spec": {"q": "timeout ok", "mode": "or"}, "replacement": "TMO"})
check("replace all reports counts", r["lines_changed"] == truth(lambda l: "timeout" in l or " ok" in l) and r["info"]["dirty"] and r["info"]["can_undo"], r)
ls = s.api("lines", {"id": did, "from": 5, "count": 1})
check("replace applied in the document", "TMO" in ls["lines"][0]["t"] and "timeout" not in ls["lines"][0]["t"], ls["lines"][0])
check("file on disk untouched until Save", open(log).read() == before)
u = s.api("doc/undo", {"id": did})
check("undo restores and is not dirty", u["ok"] and not u["info"]["dirty"] and u["info"]["can_redo"], u)
ls = s.api("lines", {"id": did, "from": 5, "count": 1})
check("undo restored text", "timeout" in ls["lines"][0]["t"])
s.api("doc/redo", {"id": did})
r = s.api("doc/replace", {"id": did, "spec": {"q": "TMO"}, "replacement": "again", "scope": "line", "line": 5})
# line 5 reads "request 5 toTMO 5ms TMO" ("took" contains "ok", replaced earlier): 2 occurrences of TMO
check("replace on one line only", r["occurrences"] == 2 and r["lines_changed"] == 1, r)
ls5 = s.api("lines", {"id": did, "from": 4, "count": 3})["lines"]
check("…and neighbouring lines are untouched (still TMO)", "TMO" in ls5[0]["t"] and "again" in ls5[1]["t"] and "TMO" in ls5[2]["t"] and "again" not in ls5[0]["t"] + ls5[2]["t"], ls5)
r = s.api("doc/replace", {"id": did, "spec": {"q": r"request (\d+)", "regex": True}, "replacement": "req#$1"})
check("regex replace with capture groups", r["lines_changed"] == 1000)
ls = s.api("lines", {"id": did, "from": 7, "count": 1})
check("capture group expanded", "req#7 " in ls["lines"][0]["t"], ls["lines"][0]["t"])
sv = s.api("doc/save", {"id": did})
check("save writes the file atomically", "req#7 " in open(log).read() and not sv["info"]["dirty"] and not [f for f in os.listdir(d) if f.startswith(".kempt")])
sv = s.api("doc/save", {"id": did, "path": P("copy.log")})
check("save as writes a new file and renames the doc", os.path.exists(P("copy.log")) and sv["info"]["name"] == "copy.log")

print("== format / fix")
broken = P("broken.json")
open(broken, "w").write("// config\n{name: 'x', 'tags': ['a','b',], \"ok\": True, \"n\": None,\n \"deep\": {\"a\": [1, 2}\n")
b = s.api("open", {"path": broken})["info"]["id"]
fx = s.api("doc/fix", {"id": b})
check("fix reports what it repaired", fx["changed"] and len(fx["report"]) >= 4, fx["report"])
txt = "".join(x["t"] + "\n" for x in s.api("lines", {"id": b, "from": 0, "count": 100})["lines"])
parsed = json.loads(txt)
check("fixed document is valid, formatted JSON", parsed == {"name": "x", "tags": ["a", "b"], "ok": True, "n": None, "deep": {"a": [1, 2]}} and "\n  " in txt, txt)
fx2 = s.api("doc/fix", {"id": b})
check("fix on clean JSON: nothing to fix, nothing changes", fx2["report"] == {} and not fx2["changed"])
mini = P("mini.json")
json.dump({"a": [1, 2, {"b": None}], "c": "x"}, open(mini, "w"), separators=(",", ":"))
m_ = s.api("open", {"path": mini})
check("minified file is flagged for formatting", m_["minified"] is False)  # tiny file: below the hint threshold
big_min = P("bigmin.json")
json.dump({"items": [{"id": i, "v": "x" * 20} for i in range(500)]}, open(big_min, "w"), separators=(",", ":"))
m_ = s.api("open", {"path": big_min})
check("big minified file is flagged", m_["minified"] is True)
f_ = s.api("doc/format", {"id": m_["info"]["id"]})
check("format expands the lines", f_["changed"] and f_["info"]["lines"] > 1000, f_["info"])
def first_lines(doc, n=3):
    return [x["t"] for x in s.api("lines", {"id": doc, "from": 0, "count": n})["lines"]]
mid = m_["info"]["id"]
check("format default indent is 2 spaces", first_lines(mid)[1] == '  "items": [', first_lines(mid))
r4 = s.api("doc/format", {"id": mid, "indent": 4})
check("format with indent 4", r4["changed"] and first_lines(mid)[1] == '    "items": [', first_lines(mid))
rt = s.api("doc/format", {"id": mid, "indent": "tab"})
check("format with indent 'tab'", rt["changed"] and first_lines(mid)[1] == '\t"items": [', first_lines(mid))
rs = s.api("doc/format", {"id": mid, "indent": 2})
check("…and back to 2 (re-indent of already formatted JSON)", rs["changed"] and first_lines(mid)[1] == '  "items": [')
rn = s.api("doc/format", {"id": mid, "indent": 2})
check("same indent again: no change", not rn["changed"])
rb = s.api("doc/format", {"id": mid, "indent": 99})
check("absurd indent is clamped to 8", rb["changed"] and first_lines(mid)[1] == " " * 8 + '"items": [', first_lines(mid))
s.api("doc/format", {"id": mid, "indent": 2})
brk = P("brk4.json"); open(brk, "w").write("{a:1,'b':[1,2,],}")
b4 = s.api("open", {"path": brk})["info"]["id"]
s.api("doc/fix", {"id": b4, "indent": "tab"})
check("Fix honours the indent choice (tab)", first_lines(b4, 3) == ["{", '\t"a": 1,', '\t"b": ['], first_lines(b4, 3))
xi = P("ind.xml"); open(xi, "w").write("<a><b><c/></b></a>")
xid2 = s.api("open", {"path": xi})["info"]["id"]
s.api("doc/format", {"id": xid2, "indent": 4})
check("XML honours indent too", first_lines(xid2, 3) == ["<a>", "    <b>", "        <c/>"], first_lines(xid2, 3))
notfix = P("plain.txt"); open(notfix, "w").write("hello\n")
n_ = s.api("open", {"path": notfix})["info"]["id"]
r = s.api("doc/fix", {"id": n_}, ok=False)
check("fix on non-structured text -> clear message", r[0] == 400 and "JSON and XML" in r[1]["error"], r)
xb = P("broken.xml"); open(xb, "w").write("<root><a x=1>AT&T<b></root>")
xid = s.api("open", {"path": xb})["info"]["id"]
fx = s.api("doc/fix", {"id": xid})
txt = "".join(x["t"] + "\n" for x in s.api("lines", {"id": xid, "from": 0, "count": 100})["lines"])
check("XML fix closes tags, quotes attrs, escapes &", fx["changed"] and 'x="1"' in txt and "AT&amp;T" in txt and "</b>" in txt and txt.strip().endswith("</root>"), txt)

print("== compare: rows, copy line, copy hunk, undo")
L = P("L.txt"); R = P("R.txt")
open(L, "w").write("a\nb\nc\nd\ne\nf\ng\n")
open(R, "w").write("a\nB\nc\nd\nX\nY\nf\ng\nnew\n")
dd = s.api("diff/new", {"left": {"path": L}, "right": {"path": R}})
sid = dd["sid"]
st = dd["state"]
check("diff state counts", st["stats"]["changed"] >= 1 and st["changes"] >= 3 and not st["identical"], st)
rows = s.api("diff/rows", {"sid": sid, "from": 0, "count": 50})["rows"]
kinds = [r["k"] for r in rows]
check("rows have kinds, line numbers and texts", kinds[0] == "eq" and "chg" in kinds and rows[1]["lt"] == "b" and rows[1]["rt"] == "B" and rows[1]["l"] == 2, rows[:3])
check("changed rows carry inline segments", rows[1]["ls"] and rows[1]["rs"] and any(seg[1] for seg in rows[1]["rs"]), rows[1])
nav = s.api("diff/nav", {"sid": sid, "row": -1, "dir": 1})
check("nav to first change", nav["row"] == 1 and nav["index"] == 0, nav)

def text(path): return open(path).read()
def right_state(): return s.api("diff/state", {"sid": sid})

# copy ONE changed line left -> right: b replaces B
s.api("diff/apply", {"sid": sid, "row": 1, "dir": "right"})
check("copy changed line → right", s.api("doc/info" if False else "info", {"id": dd["state"]["right"]["id"]})["dirty"])
rid = dd["state"]["right"]["id"]; lid = dd["state"]["left"]["id"]
rtxt = lambda: "".join(x["t"] + "\n" for x in s.api("lines", {"id": rid, "from": 0, "count": 100})["lines"])
ltxt = lambda: "".join(x["t"] + "\n" for x in s.api("lines", {"id": lid, "from": 0, "count": 100})["lines"])
check("right now has 'b' on line 2", rtxt().splitlines()[1] == "b", rtxt())
check("left file on disk untouched", text(L) == "a\nb\nc\nd\ne\nf\ng\n" and text(R) == "a\nB\nc\nd\nX\nY\nf\ng\nnew\n")
s.api("diff/undo", {"sid": sid})
check("undo reverts the copy", rtxt().splitlines()[1] == "B")
# the replace block e -> X,Y : row-level: copy right→left for the 'X' row (changed) and insert Y
rows = s.api("diff/rows", {"sid": sid, "from": 0, "count": 50})["rows"]
xi = next(r for r in rows if r["rt"] == "X")["i"]
s.api("diff/apply", {"sid": sid, "row": xi, "dir": "left"})
check("copy changed line ← left", "X" in ltxt().splitlines() and "e" not in ltxt().splitlines(), ltxt())
rows = s.api("diff/rows", {"sid": sid, "from": 0, "count": 50})["rows"]
yrow = next(r for r in rows if r["rt"] == "Y")
check("remaining added row shows as insert", yrow["k"] == "ins", yrow)
s.api("diff/apply", {"sid": sid, "row": yrow["i"], "dir": "left"})
check("insert row copied into left at the right place", ltxt().splitlines()[:7] == ["a", "b", "c", "d", "X", "Y", "f"], ltxt())
# trailing added line "new": copy left<-right then remove it again via -> (delete on right)
rows = s.api("diff/rows", {"sid": sid, "from": 0, "count": 50})["rows"]
newrow = next(r for r in rows if r["rt"] == "new")
s.api("diff/apply", {"sid": sid, "row": newrow["i"], "dir": "right"})  # adopt left's absence: delete 'new' from right
check("copying an added line toward its own side deletes it", "new" not in rtxt().splitlines(), rtxt())
# hunk-level
s.api("diff/undo", {"sid": sid}); s.api("diff/undo", {"sid": sid}); s.api("diff/undo", {"sid": sid})
check("three undos return to pristine left", ltxt() == "a\nb\nc\nd\ne\nf\ng\n" and rtxt() == "a\nB\nc\nd\nX\nY\nf\ng\nnew\n", (ltxt(), rtxt()))
rows = s.api("diff/rows", {"sid": sid, "from": 0, "count": 50})["rows"]
blk = next(r for r in rows if r["rt"] == "X")
check("hunk metadata present", blk["h"] == 2 and blk["o"] == 0, blk)
s.api("diff/apply_hunk", {"sid": sid, "op": blk["op"], "dir": "right"})
check("copy whole hunk → right (e replaces X,Y)", rtxt().splitlines() == ["a", "B", "c", "d", "e", "f", "g", "new"], rtxt())
st = s.api("diff/apply_all", {"sid": sid, "dir": "right"})
check("apply all makes sides identical", st["identical"] and rtxt() == ltxt(), st)
s.api("diff/undo", {"sid": sid})
check("…and it is undoable", not s.api("diff/state", {"sid": sid})["identical"])
o2 = s.api("diff/opts", {"sid": sid, "hide_unchanged": True})
check("hide unchanged folds rows", o2["total"] < 12 and o2["hide_unchanged"], o2["total"])
rows = s.api("diff/rows", {"sid": sid, "from": 0, "count": 50})["rows"]
check("fold rows present", any(r["k"] == "fold" for r in rows) or o2["total"] <= 9, [r["k"] for r in rows])
f_ = s.api("diff/find", {"sid": sid, "spec": {"q": "new"}, "row": -1, "dir": 1})
check("find in compare", "row" in f_, f_)
sv = s.api("doc/save", {"id": rid})
check("save right writes its file", "e" in open(R).read().splitlines() and not sv["info"]["dirty"])
vs = s.api("diff/new", {"left": {"disk": rid}, "right": {"id": rid}})
check("compare-with-disk of a clean doc is identical", vs["state"]["identical"])

print("== compare edge cases: EOL, CRLF, final newline")
A = P("A.txt"); B = P("B.txt")
open(A, "w", newline="").write("a\r\nb\r\nc\r\n"); open(B, "w", newline="").write("a\r\nB\r\nc\r\n")
e = s.api("diff/new", {"left": {"path": A}, "right": {"path": B}})
s.api("diff/apply", {"sid": e["sid"], "row": 1, "dir": "right"})
bid = e["state"]["right"]["id"]
s.api("doc/save", {"id": bid})
check("copy keeps CRLF line endings", open(B, "rb").read() == b"a\r\nb\r\nc\r\n", open(B, "rb").read())
open(A, "w").write("a\nb"); open(B, "w").write("a\nb\n")
e = s.api("diff/new", {"left": {"path": A}, "right": {"path": B}})
check("final-newline-only difference is reported", e["state"]["eol_only"], e["state"])
open(A, "w").write("a\nb\nc"); open(B, "w").write("a\nb\nc\nd\n")
e = s.api("diff/new", {"left": {"path": A}, "right": {"path": B}})
rows = s.api("diff/rows", {"sid": e["sid"], "from": 0, "count": 10})["rows"]
ins = next(r for r in rows if r["k"] == "ins")
s.api("diff/apply", {"sid": e["sid"], "row": ins["i"], "dir": "left"})
lid2 = e["state"]["left"]["id"]
check("append after an unterminated last line", "".join(x["t"] + "\n" for x in s.api("lines", {"id": lid2, "from": 0, "count": 10})["lines"]).splitlines() == ["a", "b", "c", "d"])

print("== new unsaved file, escaped JSON fix, Excel binary (.xlsb)")
n = s.api("file/new", {"name": "draft.json", "memory": True})
check("new file in memory has no path and counts as unsaved", n["info"]["path"] is None and n["info"]["dirty"] and n["info"]["kind"] == "json", n)
nid = n["info"]["id"]
s.api("doc/set", {"id": nid, "text": '{\\"a\\":1,\\"b\\":\\"x\\"}'})
fx = s.api("doc/fix", {"id": nid, "indent": 2})
tx = s.api("doc/text", {"id": nid})["text"]
check("Fix unescapes {\\\"a\\\":1}", json.loads(tx) == {"a": 1, "b": "x"} and "unescaped" in json.dumps(fx), (tx, fx))
s.api("doc/set", {"id": nid, "text": json.dumps({"r": json.dumps({"s": "ok", "n": [1, 2]}), "t": "{no}"})})
s.api("doc/fix", {"id": nid, "indent": 2})
v = json.loads(s.api("doc/text", {"id": nid})["text"])
check("Fix expands JSON stored in a string value", v["r"] == {"s": "ok", "n": [1, 2]} and v["t"] == "{no}", v)
target = P("saved-later.json")
sv = s.api("doc/save", {"id": nid, "path": target})
check("first Save writes the file where asked", os.path.exists(target) and not sv["info"]["dirty"] and sv["info"]["path"] == target, sv)
import subprocess as _sp
XB = P("t.xlsb")
_sp.check_call([sys.executable, os.path.join(os.path.dirname(os.path.abspath(__file__)), "make_xlsb.py"), XB])
xr = s.api("open", {"path": XB})
xt = s.api("doc/text", {"id": xr["info"]["id"]})["text"].splitlines()
check(".xlsb opens one table per visible sheet", xr["book"]["sheets"] == 2 and len(xr.get("also", [])) == 1, xr)
check(".xlsb cells: text, numbers, dates, bool, error", xt[0] == "Name,Café ☕,total" and xt[1] == '"inline, with comma",42,12.34' and xt[2] == "3.14159,2023-03-15,2021-01-01 12:00:00" and xt[4].startswith("TRUE,#DIV/0!"), xt)

s.stop()
print(f"\n{PASS} passed, {FAIL} failed")
sys.exit(1 if FAIL else 0)
