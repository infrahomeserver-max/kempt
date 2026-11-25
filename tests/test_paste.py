"""Paste & compare: beautify and compare text typed into the page, nothing written to a file."""
import os, sys, tempfile
sys.path.insert(0, os.path.dirname(__file__))
from ui_common import *

PASS = FAIL = 0
def check(name, cond, detail=""):
    global PASS, FAIL
    if cond: PASS += 1; print(f"  PASS  {name}")
    else: FAIL += 1; print(f"  FAIL  {name}  {str(detail)[:300]}")

os.environ["UI_EDITABLE"] = "1"
cfg = tempfile.mkdtemp(); os.environ["KEMPT_CONFIG_DIR"] = cfg
before = set(os.listdir(tempfile.gettempdir()))
with sync_playwright() as pw:
    browser = pw.chromium.launch(); g = Gui([]); errs = []
    pg = browser.new_page(viewport={"width": 1300, "height": 800}); pg.set_default_timeout(20000)
    pg.on("pageerror", lambda e: errs.append(str(e)))
    pg.goto(g.url); pg.wait_for_timeout(1500)
    pg.evaluate("document.getElementById('btn-paste').click()"); pg.wait_for_selector(".pc")
    check("Tools menu opens the paste panel", pg.locator(".pc .pane").count() == 2)
    L, R = pg.locator("[data-s=left] textarea"), pg.locator("[data-s=right] textarea")
    L.fill('{"id":"general","type":"tab","items":[1,2,3],"cfg":{"a":1,"b":[true,null]}}')
    pg.locator("[data-s=left] .b-fmt").click(); pg.wait_for_timeout(600)
    t = L.input_value()
    check("beautify indents minified JSON", t.count("\n") > 8 and '  "id": "general"' in t, t[:80])
    check("status line names the kind", "JSON" in pg.locator("[data-s=left] .st").inner_text())
    R.fill("{'id': 'general', 'type': 'tab', 'items': [1,2,4,], 'cfg': {'a': 1, 'b': [true, null,]}, // note\n}")
    pg.locator("[data-s=right] .b-fmt").click(); pg.wait_for_timeout(600)
    t = R.input_value()
    check("beautify repairs broken JSON (quotes, trailing commas, comment)", t.count("\n") > 8 and '"id": "general"' in t and ",\n  }" not in t and "'" not in t, t[:120])
    pg.locator(".pc .b-cmp").click(); pg.wait_for_selector(".tab.active .tl, .tab.active", timeout=8000); pg.wait_for_timeout(1000)
    check("compare opens a diff tab", pg.locator("#tabs .tab").count() == 2 and pg.locator(".chg").first.is_visible())
    chg = pg.locator(".chg").first.inner_text()
    check("diff finds the one changed value", "1" in chg, chg)
    check("diff rows are shown", pg.locator(".drow").count() > 5, pg.locator(".drow").count())
    # XML and plain text
    pg.evaluate("[...document.querySelectorAll('#tabs .tab')][0].click()"); pg.wait_for_timeout(300)
    L.fill("<a><b x='1'>hi</b><c/></a>"); pg.locator("[data-s=left] .b-fmt").click(); pg.wait_for_timeout(500)
    check("beautify indents XML", L.input_value().count("\n") >= 3, L.input_value())
    L.fill("just some text"); pg.locator("[data-s=left] .b-fmt").click(); pg.wait_for_timeout(400)
    check("plain text is left alone", L.input_value() == "just some text")
    # plain text compare with Ctrl+Enter, swap, clear
    L.fill("one\ntwo\nthree\n"); R.fill("one\n2\nthree\n"); L.focus(); pg.keyboard.press("Control+Enter"); pg.wait_for_timeout(1200)
    check("Ctrl+Enter compares plain text", pg.locator("#tabs .tab").count() == 3 and pg.locator(".drow").count() >= 3)
    pg.evaluate("[...document.querySelectorAll('#tabs .tab')][0].click()"); pg.wait_for_timeout(300)
    pg.locator(".pc .b-swap").click(); check("swap exchanges the sides", L.input_value().startswith("one\n2") and R.input_value().startswith("one\ntwo"))
    pg.locator("[data-s=left] .b-clr").click(); check("clear empties a side", L.input_value() == "")
    check("no page errors", not errs, errs[:2])
    browser.close(); g.stop()
check("nothing was written to the config folder as a file", not any(f.endswith((".json", ".txt", ".xml")) and f.startswith(("left", "right")) for f in os.listdir(cfg)), os.listdir(cfg))
print(f"\n{PASS} passed, {FAIL} failed"); sys.exit(1 if FAIL else 0)
