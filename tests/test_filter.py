"""Backend tests: column filters, multi-column duplicates, custom separator."""
import os, sys, tempfile
sys.path.insert(0, os.path.dirname(__file__))
src = open(os.path.join(os.path.dirname(__file__), "test_table.py")).read().split('d = tempfile.mkdtemp()')[0]
exec(src)

d = tempfile.mkdtemp(); cfg = os.path.join(d, "cfg")
csv = os.path.join(d, "a.csv")
open(csv, "w").write("name,city,age\nAnn,Paris,30\nBob,Rome,25\nann,Paris,41\nCy,,19\nBob,Rome,70\nDee,Paris,30\n")
s = Server(cfg)
did = s.api("open", {"path": csv})["info"]["id"]
s.api("table/open", {"id": did})
def rows(m=None):
    n = (m or {}).get("shown", 99)
    return [r["c"] for r in s.api("table/rows", {"id": did, "from": 0, "count": 50})["rows"]]
def F(*fs): return s.api("table/filter", {"id": did, "filters": list(fs)})

m = F({"col": 1, "op": "eq", "v": "paris"})
check("equals (case-insensitive)", m["shown"] == 3 and m["nrows"] == 6, m["shown"])
m = F({"col": 0, "op": "contains", "v": "an"}); check("contains", [r[0] for r in rows()] == ["Ann", "ann"])
m = F({"col": 0, "op": "contains", "v": "an", "ci": False}); check("contains, case-sensitive", [r[0] for r in rows()] == ["ann"])
m = F({"col": 1, "op": "blank"}); check("blank", [r[0] for r in rows()] == ["Cy"])
m = F({"col": 1, "op": "nblank"}); check("not blank", m["shown"] == 5)
m = F({"col": 0, "op": "starts", "v": "B"}); check("starts with", m["shown"] == 2)
m = F({"col": 0, "op": "ends", "v": "e"}); check("ends with", [r[0] for r in rows()] == ["Dee"])
m = F({"col": 0, "op": "ncontains", "v": "b"}); check("not contains", m["shown"] == 4)
m = F({"col": 2, "op": "gt", "v": "30"}); check("number >", [r[2] for r in rows()] == ["41", "70"])
m = F({"col": 2, "op": "le", "v": "30"}); check("number <=", m["shown"] == 4)
m = F({"col": 2, "op": "between", "v": "40", "v2": "20"}); check("between (either order)", [r[2] for r in rows()] == ["30", "25", "30"], rows())
m = F({"col": 1, "op": "in", "list": ["Rome", ""]}); check("value list incl. blank", sorted(r[0] for r in rows()) == ["Bob", "Bob", "Cy"])
m = F({"col": 1, "op": "eq", "v": "Paris"}, {"col": 2, "op": "ge", "v": "30"})
check("two columns combine with AND", [r[0] for r in rows()] == ["Ann", "ann", "Dee"], rows())
st, j = s.api("table/filter", {"id": did, "filters": [{"col": 2, "op": "gt", "v": "abc"}]}, ok=False)
check("a word in a number filter is rejected", st == 400 and "not a number" in str(j), (st, j))
v = s.api("table/values", {"id": did, "col": 1})
check("value list ignores that column's own filter", {x["v"]: x["n"] for x in v["values"]} is not None, v)
vv = {x["v"]: x["n"] for x in v["values"]}
check("value list counts (others' filters applied)", vv == {"Paris": 3, "Rome": 1}, vv)

# filter + sort + dups together
F({"col": 1, "op": "eq", "v": "Paris"})
m = s.api("table/sort", {"id": did, "col": 2, "desc": True, "mode": "number"})
check("sort within filtered rows", [r[2] for r in rows()] == ["41", "30", "30"] and m["shown"] == 3, rows())
m = s.api("table/dups", {"id": did, "cols": [1, 2]})
check("duplicates over 2 columns, inside the filter", m["dup"]["rows"] == 2 and m["dup"]["groups"] == 1, m["dup"])
m = F()
check("clearing filters redoes duplicates on all rows", m["shown"] == m["dup"]["rows"], m["dup"])
s.api("table/dups", {"id": did, "off": True}); s.api("table/sort", {"id": did, "keys": []})

print("== multi-column duplicates")
def dup(cols, ci=False): return s.api("table/dups", {"id": did, "cols": cols, "ci": ci})["dup"]
check("name+city (case-sensitive)", dup([0, 1])["rows"] == 2, dup([0, 1]))        # Bob/Rome x2
check("name+city (ignore case)", dup([0, 1], True)["rows"] == 4)                  # + Ann/ann Paris
check("city+age", dup([1, 2])["rows"] == 2)                                       # Paris/30 twice
check("single column still works", dup([1])["rows"] == 5)
check("blank keys are never duplicates", dup([1])["groups"] == 2, dup([1]))       # Paris, Rome (Cy blank ignored)
check("whole line when no columns", s.api("table/dups", {"id": did, "cols": []})["dup"]["rows"] == 0)
s.api("table/dups", {"id": did, "off": True})

print("== separators")
for name, sep, text in (("p.psv", "|", "a|b|c\n1|2|3\n"), ("c.txt", "^", "a^b^c\n1^2^3\n"), ("t.txt", "~", "a~b~c\n1~2~3\n")):
    fp = os.path.join(d, name); open(fp, "w").write(text)
    i = s.api("open", {"path": fp})["info"]["id"]
    m = s.api("table/open", {"id": i, "delim": sep})
    check(f"custom separator {sep!r}", m["ncols"] == 3 and m["delim"] == sep, m)
fp = os.path.join(d, "auto.psv"); open(fp, "w").write("a|b\n1|2\n")
check("pipe auto-detected", s.api("table/open", {"id": s.api("open", {"path": fp})["info"]["id"]})["delim"] == "|")
s.stop()
print(f"\n{PASS} passed, {FAIL} failed"); sys.exit(1 if FAIL else 0)
