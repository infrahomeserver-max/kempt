"""A reloaded page (browser discarded the tab, laptop woke from sleep) takes back the documents the server still holds."""
import os, sys, tempfile
sys.path.insert(0, os.path.dirname(__file__))
from ui_common import *
PASS = FAIL = 0
def check(name, cond, detail=""):
    global PASS, FAIL
    if cond: PASS += 1; print(f"  PASS  {name}")
    else: FAIL += 1; print(f"  FAIL  {name}  {str(detail)[:300]}")
def poll(f, t=12):
    import time
    end = time.time() + t
    while time.time() < end:
        try:
            if f(): return True
        except Exception: pass
        time.sleep(0.2)
    return False
d = tempfile.mkdtemp(); fs = []
for n in ("a.txt", "b.txt"):
    p = os.path.join(d, n); open(p, "w").write(f"hello {n}\nline two\n"); fs.append(p)
os.environ["UI_EDITABLE"] = "1"
with sync_playwright() as pw:
    br = pw.chromium.launch(); g = Gui([]); errs = []
    pg = br.new_page(viewport={"width": 1400, "height": 860}); pg.set_default_timeout(15000)
    pg.on("pageerror", lambda e: errs.append(str(e)))
    pg.goto(g.url); pg.wait_for_timeout(1000)
    for p in fs: pg.evaluate("p => actions.openPath(p)", p)
    check("two tabs open", poll(lambda: pg.locator("#tabs .tab").count() == 2))
    pg.evaluate("() => { Tabs.activate(Tabs.list[0]); }"); pg.wait_for_timeout(1200)   # session state is saved after a short delay
    pg.reload()
    check("tabs come back after a reload", poll(lambda: pg.locator("#tabs .tab").count() == 2), pg.locator("#tabs .tab").count())
    check("same order", pg.evaluate("() => Tabs.list.map(t => t.info.name).join(',')") == "a.txt,b.txt")
    check("active tab remembered", pg.evaluate("() => Tabs.active === Tabs.list[0]"))
    # a second window must not steal them
    pg2 = br.new_page(viewport={"width": 1400, "height": 860}); pg2.goto(g.url); pg2.wait_for_timeout(1500)
    check("second window starts empty", pg2.locator("#tabs .tab").count() == 0, pg2.locator("#tabs .tab").count())
    check("first window kept its tabs", pg.locator("#tabs .tab").count() == 2)
    check("no page errors", not errs, errs)
    g.stop()
print(f"\n{PASS} passed, {FAIL} failed"); sys.exit(1 if FAIL else 0)
