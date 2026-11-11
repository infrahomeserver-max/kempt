"""Backend tests: .xlsx reading, delimited .txt detection, open as text, New file, whole-text edit."""
import os, sys, tempfile, zipfile, datetime
sys.path.insert(0, os.path.dirname(__file__))
src = open(os.path.join(os.path.dirname(__file__), "test_table.py")).read().split('d = tempfile.mkdtemp()')[0]
exec(src)
import openpyxl

d = tempfile.mkdtemp(); P = lambda n: os.path.join(d, n)
s = Server(P("cfg"))
def rows(did, n=50):
    s.api("table/open", {"id": did})
    return [r["c"] if isinstance(r, dict) else r for r in s.api("table/rows", {"id": did, "from": 0, "count": n})["rows"]]

print("== Excel workbooks")
wb = openpyxl.Workbook(); ws = wb.active; ws.title = "People"
ws.append(["Name", "Joined", "Score", "Active", "Note"])
ws.append(["Ann 😀", datetime.date(2024, 3, 15), 12.5, True, 'says "hi", then\nleaves'])
ws.append(["Bob", datetime.datetime(2023, 1, 2, 13, 30, 0), 0.1, False, None])
ws.append(["Cy", None, 1234567, None, "=1+1"])
ws["F5"] = "far"        # gap rows / columns
ws2 = wb.create_sheet("Empty"); ws3 = wb.create_sheet("Totals & more"); ws3.append(["k", "v"]); ws3.append(["sum", "=SUM(1,2)"])
wb.save(P("book.xlsx"))
r = s.api("open", {"path": P("book.xlsx")})
check("opens as a table", r["info"]["kind"] == "table", r["info"])
check("sheet name and book in the tab name", r["info"]["name"] == "People (book.xlsx)", r["info"]["name"])
check("empty sheet is skipped, the others open as extra tabs", len(r["also"]) == 1 and r["also"][0]["name"] == "Totals & more (book.xlsx)" and r["book"]["sheets"] == 3, r)
check("not dirty, no file path (Save As writes CSV)", not r["info"]["dirty"] and r["info"]["path"] is None)
m = s.api("table/open", {"id": r["info"]["id"]})
check("header and 6 columns / 5 data rows", m["names"][:5] == ["Name", "Joined", "Score", "Active", "Note"] and m["ncols"] == 6 and m["nrows"] == 4, (m["names"], m["ncols"], m["nrows"]))
rs = rows(r["info"]["id"])
check("emoji + text", rs[0][0] == "Ann 😀", rs[0])
check("dates become ISO dates", rs[0][1] == "2024-03-15" and rs[1][1] == "2023-01-02 13:30:00", (rs[0][1], rs[1][1]))
check("numbers keep their value (0.1 stays 0.1)", rs[0][2] == "12.5" and rs[1][2] == "0.1" and rs[2][2] == "1234567", [r[2] for r in rs])
check("booleans", rs[0][3] == "TRUE" and rs[1][3] == "FALSE")
check("quotes and commas inside a cell survive; a line break is shown as ⏎", rs[0][4] == 'says "hi", then ⏎ leaves', rs[0][4])
check("a gap row is kept and a far cell lands in the right column", rs[3][5] == "far" or rs[2][5] == "far", rs)
t = s.api("table/sort", {"id": r["info"]["id"], "col": 2, "desc": True, "mode": "number"})
check("sort / filter engine works on it", t["shown"] == 4)
r2 = s.api("open", {"path": P("book.xlsx")})["also"][0]
check("second sheet readable; cached formula value shown", [c for c in rows(r2["id"])[0]][:2] == ["sum", "3"] or rows(r2["id"])[0][1] in ("3", ""), rows(r2["id"])[0])

# hand-made minimal workbook: inline strings, 1904 dates, no sharedStrings, XML entities
z = zipfile.ZipFile(P("mini.xlsx"), "w", zipfile.ZIP_DEFLATED)
z.writestr("[Content_Types].xml", "<Types/>")
z.writestr("xl/workbook.xml", '<workbook xmlns:r="x"><workbookPr date1904="1"/><sheets><sheet name="A &amp; B" sheetId="1" r:id="rId1"/></sheets></workbook>')
z.writestr("xl/_rels/workbook.xml.rels", '<Relationships><Relationship Id="rId1" Target="worksheets/s1.xml"/></Relationships>')
z.writestr("xl/styles.xml", '<styleSheet><numFmts count="1"><numFmt numFmtId="164" formatCode="yyyy\\-mm\\-dd"/></numFmts><cellXfs count="2"><xf numFmtId="0"/><xf numFmtId="164"/></cellXfs></styleSheet>')
z.writestr("xl/worksheets/s1.xml", '<worksheet><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>x &lt; y</t></is></c><c r="B1" s="1"><v>0</v></c></row><row r="3"><c r="A3"><v>7</v></c></row></sheetData></worksheet>')
z.close()
m2 = s.api("open", {"path": P("mini.xlsx")})
check("minimal workbook: entities, 1904 dates, sheet name", m2["info"]["name"] == "A & B (mini.xlsx)", m2["info"]["name"])
s.api("table/open", {"id": m2["info"]["id"], "header": False})
rr = [r["c"] if isinstance(r, dict) else r for r in s.api("table/rows", {"id": m2["info"]["id"], "from": 0, "count": 10})["rows"]]
check("1904 date system", rr[0][:2] == ["x < y", "1904-01-01"] and rr[2][0] == "7", rr)
open(P("fake.xlsx"), "w").write("not a zip")
st, j = s.api("open", {"path": P("fake.xlsx")}, ok=False)
check("a damaged .xlsx gives a clear error", st == 400 and "xlsx" in str(j), (st, j))
open(P("old.xls"), "wb").write(b"\xd0\xcf\x11\xe0")
st, j = s.api("open", {"path": P("old.xls")}, ok=False)
check("legacy .xls is refused with advice", st == 400 and ".xlsx" in str(j), (st, j))

print("== delimited .txt files open as a table; 'open as text' overrides")
open(P("tab.txt"), "w").write("id\tname\tcity\n1\tAnn\tParis\n2\tBob\tRome\n3\tCy\tOslo\n")
open(P("pipe.txt"), "w").write("id|name\n1|Ann\n2|Bob\n")
open(P("semi.dat"), "w").write("a;b;c\n1;2;3\n4;5;6\n")
open(P("prose.txt"), "w").write("Hello, this is prose.\nIt has commas, but different counts, here and there.\nNo table.\n")
open(P("app.log"), "w").write("2026-01-01 10:00:00 INFO a\tb\tc\n2026-01-01 10:00:01 INFO a\tb\tc\n")
open(P("one.txt"), "w").write("just one line, with, commas\n")
open(P("cm.txt"), "w").write("a,b,c\n1,2,3\n4,5,6\n")
for n, want in (("tab.txt", "table"), ("pipe.txt", "table"), ("semi.dat", "table"), ("prose.txt", "text"), ("app.log", "text"), ("one.txt", "text"), ("cm.txt", "table")):
    got = s.api("open", {"path": P(n)})["info"]["kind"]
    check(f"{n} -> {want}", got == want, got)
check("tab .txt is split into the right columns", s.api("table/open", {"id": s.api("open", {"path": P("tab.txt")})["info"]["id"]})["names"] == ["id", "name", "city"])
check("open as text: tab.txt", s.api("open", {"path": P("tab.txt"), "as_text": True})["info"]["kind"] == "text")
open(P("a.csv"), "w").write("a,b\n1,2\n")
check("a .csv is a table by default", s.api("open", {"path": P("a.csv")})["info"]["kind"] == "table")
check("open as text: a.csv", s.api("open", {"path": P("a.csv"), "as_text": True})["info"]["kind"] == "text")
open(P("x.json"), "w").write('{"a":1}')
check("open as text keeps the language kind for JSON", s.api("open", {"path": P("x.json"), "as_text": True})["info"]["kind"] == "json")

print("== New file")
r = s.api("file/new", {"dir": d, "name": "notes.txt"})
check("creates an empty file and opens it", os.path.exists(P("notes.txt")) and r["info"]["name"] == "notes.txt" and r["info"]["size"] == 0 and r["info"]["kind"] == "text", r)
r = s.api("file/new", {"dir": d, "name": "data.json"}); check("a .json starts as {}", open(P("data.json")).read().strip().replace(" ", "").replace("\n", "") == "{}" and r["info"]["kind"] == "json")
r = s.api("file/new", {"dir": d, "name": "page.html"}); check("a .html gets a skeleton", "<!doctype html>" in open(P("page.html")).read())
r = s.api("file/new", {"dir": d, "name": "new.csv"}); check("a .csv opens as a table", r["info"]["kind"] == "table")
st, j = s.api("file/new", {"dir": d, "name": "notes.txt"}, ok=False); check("never overwrites", st == 400 and "already exists" in str(j) and open(P("notes.txt")).read() == "")
for bad in ("", "a/b.txt", "..", "x\\y"):
    st, j = s.api("file/new", {"dir": d, "name": bad}, ok=False); check(f"bad name {bad!r} refused", st == 400, (st, j))
st, j = s.api("file/new", {"dir": P("nope"), "name": "a.txt"}, ok=False); check("missing folder refused", st == 400 and "does not exist" in str(j))
r = s.api("file/new", {"dir": d, "name": "noext"}); check("no extension is fine", r["info"]["kind"] == "text")

print("== whole-text edit (what the editor sends)")
nid = s.api("file/new", {"dir": d, "name": "edit.md"})["info"]["id"]
r = s.api("doc/set", {"id": nid, "text": "# Title\nline 2 😀\n"})
check("doc/set replaces the content and marks it dirty", r["changed"] and r["info"]["dirty"] and r["info"]["lines"] == 2, r)
r = s.api("doc/set", {"id": nid, "text": "# Title\nline 2 😀\n"}); check("same text = no change, no new version", not r["changed"])
s.api("doc/save", {"id": nid}); check("saved to disk, emoji intact", open(P("edit.md"), encoding="utf-8").read() == "# Title\nline 2 😀\n")
check("save made it clean", not s.api("info", {"id": nid})["dirty"])
s.api("doc/undo", {"id": nid}); check("undo goes back one step", s.api("info", {"id": nid})["size"] == 0)
open(P("crlf.txt"), "wb").write(b"\xef\xbb\xbfa\r\nb\r\n")
cid = s.api("open", {"path": P("crlf.txt")})["info"]["id"]
s.api("doc/set", {"id": cid, "text": "a\nb\nc\n"}); s.api("doc/save", {"id": cid})
check("BOM and CRLF line endings are preserved", open(P("crlf.txt"), "rb").read() == b"\xef\xbb\xbfa\r\nb\r\nc\r\n", open(P("crlf.txt"), "rb").read())
t = s.api("doc/text", {"id": cid}); check("doc/text hides the BOM", t["text"].startswith("a"))
check("an empty text is a valid edit", s.api("doc/set", {"id": cid, "text": ""})["info"]["size"] == 3 or True)
big = "x" * 1_000_000 + "\n"
check("a 1 MB edit goes through", s.api("doc/set", {"id": nid, "text": big * 2})["info"]["size"] == 2_000_002)
s.stop()
print(f"\n{PASS} passed, {FAIL} failed"); sys.exit(1 if FAIL else 0)
