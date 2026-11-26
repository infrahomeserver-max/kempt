"""Ctrl+click tabs to compare them; selecting a word highlights it everywhere."""
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
d = tempfile.mkdtemp(); fs = []
for i, t in enumerate(['{"name":"alpha","v":1}\nalpha again', '{"name":"alpha","v":2}', 'plain third alpha']):
    p = os.path.join(d, f"f{i}.txt" if i == 2 else f"f{i}.json"); open(p, "w").write(t); fs.append(p)
with sync_playwright() as pw:
    br = pw.chromium.launch(); g = Gui(fs, ["--tabs"]); errs = []
    pg = br.new_page(viewport={"width": 1300, "height": 800}); pg.set_default_timeout(20000)
    pg.on("pageerror", lambda e: errs.append(str(e)))
    pg.goto(g.url); pg.wait_for_timeout(2000)
    tabs = pg.locator("#tabs .tab"); check("three tabs", tabs.count() == 3, tabs.count())
    tabs.nth(0).click(modifiers=["Control"]); tabs.nth(1).click(modifiers=["Control"])
    check("two marked, bar shown", pg.locator("#tabs .tab.mk").count() == 2 and pg.locator("#cmpbar:not([hidden])").count() == 1)
    pg.locator("#cb-go").click(); pg.wait_for_timeout(1500)
    check("compare tab opened", pg.locator("#tabs .tab").count() == 4, pg.locator("#tabs .tab").count())
    check("bar cleared", pg.locator("#cmpbar:not([hidden])").count() == 0)
    # context menu
    pg.locator("#tabs .tab").nth(0).click(button="right"); pg.wait_for_timeout(200)
    check("context menu lists tabs", pg.locator(".ctxm button").count() >= 3)
    pg.locator(".ctxm button", has_text="f2.txt").click(); pg.wait_for_timeout(1500)
    check("compare from menu", pg.locator("#tabs .tab").count() == 5, pg.locator("#tabs .tab").count())
    # same-word highlight
    pg.locator("#tabs .tab").nth(0).click(); pg.wait_for_timeout(500)
    pg.evaluate("""() => { const t=document.querySelector('#main .show .ce-ta'); t.focus(); const i=t.value.indexOf('again'); t.setSelectionRange(i,i+5); }""")
    pg.wait_for_timeout(500)
    n = pg.evaluate("CSS.highlights.has('kword') ? [...CSS.highlights.get('kword')].length : 0")
    check("selected word highlighted", n >= 1, str(n) + " " + str(pg.evaluate("[document.activeElement.tagName, getSelection().toString(), typeof Highlight, !!CSS.highlights]")))
    check("no page errors", not errs, errs)
    g.stop()
print(f"\n{PASS} passed, {FAIL} failed"); sys.exit(1 if FAIL else 0)
