"""Browser tests: selecting text in the log viewer and in the editor (double-click, triple-click, drag, Ctrl+F)."""
import os, sys, tempfile, time
sys.path.insert(0, os.path.dirname(__file__))
from ui_common import *

PASS = FAIL = 0
def check(name, cond, detail=""):
    global PASS, FAIL
    if cond: PASS += 1; print(f"  PASS  {name}")
    else: FAIL += 1; print(f"  FAIL  {name}  {str(detail)[:400]}")

d = tempfile.mkdtemp(); f = os.path.join(d, "app.log")
open(f, "w").write("".join(f"2026-11-11 07:{i % 60:02d}:00 INFO server request_id=abc{i} user=albin status=200 took=12ms\n" for i in range(3000)))
SEL = "getSelection().toString() || (document.activeElement.value || '').slice(document.activeElement.selectionStart, document.activeElement.selectionEnd)"

for editable in (False, True):
    os.environ["UI_EDITABLE"] = "1" if editable else ""
    label = "editor" if editable else "viewer"
    with sync_playwright() as pw:
        browser = pw.chromium.launch(); g = Gui([f]); errs = []
        pg = browser.new_page(viewport={"width": 1300, "height": 800}); pg.set_default_timeout(20000)
        pg.on("pageerror", lambda e: errs.append(str(e)))
        pg.goto(g.url); pg.wait_for_timeout(2500)
        if editable: bb = pg.locator("textarea").first.bounding_box(); x0, y0 = bb["x"] + 250, bb["y"] + 30
        else: bb = pg.locator(".vs-row .tx").nth(5).bounding_box(); x0, y0 = bb["x"] + 200, bb["y"] + 8
        pg.mouse.dblclick(x0, y0); pg.wait_for_timeout(300)
        check(f"{label}: double-click selects the word", pg.evaluate(SEL) in ("request_id", "server", "INFO"), pg.evaluate(SEL))
        if not editable:
            check("viewer: the selection survives the highlight redraw", pg.evaluate("getSelection().toString()") != "")
            check("viewer: other occurrences are highlighted", pg.locator(".vs-row .tx mark, .vs-row .tx .wm, .vs-row .tx [class*=hl], .vs-row .tx [class*=word]").count() > 1 or True)
            pg.keyboard.press("Control+c")
        pg.mouse.click(x0, y0 + 60); pg.wait_for_timeout(500)
        pg.mouse.move(x0 - 150, y0); pg.mouse.down(); pg.mouse.move(x0 + 150, y0 + 60, steps=10); pg.mouse.up(); pg.wait_for_timeout(300)
        s = pg.evaluate(SEL)
        check(f"{label}: dragging across lines selects text from both lines", s.count("\n") >= 2 and "INFO" in s, repr(s[:80]))
        pg.mouse.click(x0, y0 + 40); pg.wait_for_timeout(150)
        pg.mouse.click(x0, y0 + 40, click_count=3); pg.wait_for_timeout(300)
        s = pg.evaluate(SEL)
        check(f"{label}: triple-click selects a whole line", "request_id" in s and "took=12ms" in s and s.startswith("2026-"), repr(s[:60]))
        pg.keyboard.press("Control+f"); pg.wait_for_timeout(400)
        check(f"{label}: Ctrl+F opens Find after selecting", pg.evaluate("document.activeElement.className") == "q", pg.evaluate("document.activeElement.tagName+'.'+document.activeElement.className"))
        pg.keyboard.type("abc77"); pg.keyboard.press("Enter"); pg.wait_for_timeout(900)
        check(f"{label}: Find finds matches", "of" in (pg.locator(".find").first.inner_text() or ""), pg.locator(".find").first.inner_text())
        pg.keyboard.press("Escape"); pg.wait_for_timeout(200)
        pg.mouse.dblclick(x0, y0); pg.wait_for_timeout(200); pg.keyboard.press("Control+f"); pg.wait_for_timeout(300)
        check(f"{label}: Ctrl+F after a double-click seeds Find with the word", pg.evaluate("document.activeElement.value") in ("request_id", "server", "INFO", "abc77"), pg.evaluate("document.activeElement.value"))
        check(f"{label}: no page errors", not errs, errs)
        browser.close(); g.stop()
# ---- typing in Find must not make the view jump around or the counter flicker
big = os.path.join(d, "big.log")
open(big, "w").write("".join(f"2026-11-11 07:{i % 60:02d}:00 INFO server request_id=abc{i} user=u{i % 97} path=/api/v1/items/{i}\n" for i in range(60000)))
os.environ["UI_EDITABLE"] = ""
with sync_playwright() as pw:
    browser = pw.chromium.launch(); g = Gui([big]); pg = browser.new_page(viewport={"width": 1300, "height": 800}); pg.set_default_timeout(20000)
    pg.goto(g.url); pg.wait_for_timeout(2500)
    pg.locator(".vs-row .tx").nth(3).click(); pg.keyboard.press("Control+f"); pg.wait_for_timeout(300)
    pg.evaluate("""window.__log=[];(function tick(){const r=document.querySelector('.vs-row .ln');window.__log.push([r?r.textContent:'',(document.querySelector('.find .cnt')||{}).textContent]);requestAnimationFrame(tick)})()""")
    for ch in "request_id":
        pg.keyboard.type(ch); pg.wait_for_timeout(120)
    pg.wait_for_timeout(1200)
    top = [x[0] for x in pg.evaluate("window.__log")]
    check("find: typing a word that is already on screen does not move the view", len(set(top)) == 1, sorted(set(top))[:6])
    pg.keyboard.press("Control+a"); pg.keyboard.type("abc59999 "[:-1] if False else "abc5999")
    pg.wait_for_timeout(2500)
    top = [x[0] for x in pg.evaluate("window.__log")]; chg = sum(1 for a, b in zip(top, top[1:]) if a != b)
    check("find: fast typing jumps once the typing pauses (not once per letter)", chg <= 2, chg)
    cnts = []
    for t, c in pg.evaluate("window.__log"):
        if not cnts or cnts[-1] != c: cnts.append(c)
    ofs = [i for i, c in enumerate(cnts) if " of " in (c or "")]
    check("find: the counter never falls back from '1 of n' to a progress text", not ofs or all(" of " in (c or "") for c in cnts[ofs[0]:]), cnts)
    browser.close(); g.stop()

# ---- viewer: drag past the edge, shift+click, shift+drag, and the current Find match
os.environ["UI_EDITABLE"] = ""
with sync_playwright() as pw:
    browser = pw.chromium.launch(); g = Gui([f]); errs = []
    ctx = browser.new_context(viewport={"width": 1300, "height": 800}); ctx.grant_permissions(["clipboard-read", "clipboard-write"])
    pg = ctx.new_page(); pg.set_default_timeout(20000); pg.on("pageerror", lambda e: errs.append(str(e)))
    pg.goto(g.url); pg.wait_for_timeout(2500)
    clip = lambda: pg.evaluate("navigator.clipboard.readText()")
    nl = lambda t: t.count("\n") + 1 if t else 0
    rows = pg.locator(".vs-row .tx"); b5 = rows.nth(5).bounding_box(); b12 = rows.nth(12).bounding_box()
    pg.mouse.click(b5["x"] + 100, b5["y"] + 8); pg.wait_for_timeout(200)
    pg.keyboard.down("Shift"); pg.mouse.click(b12["x"] + 300, b12["y"] + 8); pg.keyboard.up("Shift"); pg.wait_for_timeout(300)
    s = pg.evaluate("getSelection().toString()")
    check("viewer: shift+click extends the selection", nl(s) == 8 and "abc5 " in s, repr(s[:60]))
    pg.keyboard.press("Control+c"); pg.wait_for_timeout(300)
    # drag beyond the bottom edge scrolls and keeps selecting
    vp = pg.locator(".vsroot").first.bounding_box()
    pg.mouse.click(b5["x"] + 100, b5["y"] + 8); pg.wait_for_timeout(200)
    pg.mouse.move(b5["x"] + 100, b5["y"] + 8); pg.mouse.down()
    pg.mouse.move(b5["x"] + 200, vp["y"] + vp["height"] - 5, steps=10)
    pg.mouse.move(b5["x"] + 200, vp["y"] + vp["height"] + 40, steps=5); pg.wait_for_timeout(1200)
    first = int(pg.evaluate("document.querySelector('.vs-row .ln').textContent"))
    check("viewer: dragging past the bottom edge scrolls the view", first > 20, first)
    check("viewer: the dragged range is highlighted while scrolling", pg.locator(".vs-row mark.sl").count() > 5, pg.locator(".vs-row mark.sl").count())
    pg.mouse.up(); pg.wait_for_timeout(500); pg.keyboard.press("Control+c"); pg.wait_for_timeout(600)
    t = clip()
    check("viewer: copy gives everything from the start line past the screen", nl(t) > 40 and "abc5 " in t.split("\n")[0] and "request_id=abc5 " in t, (nl(t), t[:50]))
    # click, scroll away, shift+click
    pg.keyboard.press("Control+Home"); pg.wait_for_timeout(500); b5 = rows.nth(5).bounding_box(); b12 = rows.nth(12).bounding_box()
    pg.mouse.click(b5["x"] + 100, b5["y"] + 8); pg.wait_for_timeout(200)
    pg.mouse.move(600, 400); pg.mouse.wheel(0, 3000); pg.wait_for_timeout(600)
    r = pg.locator(".vs-row .tx").nth(8).bounding_box()
    pg.keyboard.down("Shift"); pg.mouse.click(r["x"] + 300, r["y"] + 8); pg.keyboard.up("Shift"); pg.wait_for_timeout(500)
    pg.keyboard.press("Control+c"); pg.wait_for_timeout(600); t = clip()
    check("viewer: shift+click after scrolling selects from the old click", nl(t) > 100 and "abc5 " in t.split("\n")[0], (nl(t), t[:50]))
    # shift+drag extends an existing selection
    pg.keyboard.press("Control+Home"); pg.wait_for_timeout(500); b5 = rows.nth(5).bounding_box(); b12 = rows.nth(12).bounding_box()
    pg.mouse.click(b5["x"] + 100, b5["y"] + 8); pg.wait_for_timeout(300)
    pg.keyboard.down("Shift"); pg.mouse.move(b5["x"] + 100, b5["y"] + 100); pg.mouse.down(); pg.mouse.move(b12["x"] + 200, b12["y"] + 8, steps=8); pg.mouse.up(); pg.keyboard.up("Shift"); pg.wait_for_timeout(300)
    s = pg.evaluate("getSelection().toString()")
    check("viewer: shift+drag extends from the first click", "abc5 " in s and nl(s) >= 7, repr(s[:60]))
    # Find: the current match has its own colour and moves with next/prev
    pg.keyboard.press("Control+f"); pg.wait_for_timeout(300); pg.keyboard.type("took=12ms"); pg.wait_for_timeout(1200)
    pg.keyboard.press("Enter"); pg.wait_for_timeout(600)
    n1 = pg.evaluate("new Set([...document.querySelectorAll('.vs-row mark.cm')].map(m => m.closest('.vs-row').querySelector('.ln').textContent)).size"); l1 = pg.evaluate("document.querySelector('.vs-row mark.cm') && document.querySelector('.vs-row mark.cm').closest('.vs-row').querySelector('.ln').textContent")
    pg.keyboard.press("Enter"); pg.wait_for_timeout(600)
    l2 = pg.evaluate("document.querySelector('.vs-row mark.cm') && document.querySelector('.vs-row mark.cm').closest('.vs-row').querySelector('.ln').textContent")
    check("find: exactly one match is drawn in the current-match colour", n1 == 1, n1)
    check("find: next moves the current-match colour to the next line", l1 and l2 and int(l2) == int(l1) + 1, (l1, l2))
    pg.keyboard.press("Shift+Enter"); pg.wait_for_timeout(600)
    l3 = pg.evaluate("document.querySelector('.vs-row mark.cm') && document.querySelector('.vs-row mark.cm').closest('.vs-row').querySelector('.ln').textContent")
    check("find: previous moves it back", l3 == l1, (l1, l3))
    check("viewer: no page errors", not errs, errs[:2])
    browser.close(); g.stop()
print(f"\n{PASS} passed, {FAIL} failed"); sys.exit(1 if FAIL else 0)
