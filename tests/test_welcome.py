"""Welcome screen: recent files, pinning, restoring the last session, theme variants."""
import os, sys, tempfile
sys.path.insert(0, os.path.dirname(__file__))
from ui_common import *
PASS = FAIL = 0
def poll(f, t=12):
    import time
    end = time.time() + t
    while time.time() < end:
        try:
            if f(): return True
        except Exception: pass
        time.sleep(0.2)
    return False


def check(name, cond, detail=""):
    global PASS, FAIL
    if cond: PASS += 1; print(f"  PASS  {name}")
    else: FAIL += 1; print(f"  FAIL  {name}  {str(detail)[:300]}")
os.environ["KEMPT_CONFIG_DIR"] = tempfile.mkdtemp()
d = tempfile.mkdtemp(); fs = []
for n, t in [("applog.json", '{"a":1}'), ("pom.xml", "<a/>"), ("c.csv", "id\n1")]:
    p = os.path.join(d, n); open(p, "w").write(t); fs.append(p)
with sync_playwright() as pw:
    br = pw.chromium.launch(); g = Gui([]); errs = []
    pg = br.new_page(viewport={"width": 1400, "height": 860}, color_scheme="dark"); pg.set_default_timeout(15000)
    pg.on("pageerror", lambda e: errs.append(str(e)))
    pg.goto(g.url); pg.wait_for_timeout(1200)
    check("empty welcome", "Welcome to Kempt" in pg.inner_text(".wl-dark"))
    pg.evaluate("p => { for (const x of p) rememberRecent(x, false); prefs.set('session', [p[0], p[1]]); }", fs)
    pg.wait_for_timeout(1500)
    pg.reload(); poll(lambda: pg.locator(".wl-dark .row").count() >= 3)
    check("recent listed", pg.locator(".wl-dark .row").count() == 3, pg.locator(".wl-dark .row").count())
    check("sizes filled in", "B" in pg.inner_text(".wl-dark .row >> nth=0"))
    pg.locator(".wl-dark .row .star").nth(1).click(); pg.wait_for_timeout(300)
    check("pinned shows", pg.locator(".wl-dark .pin").count() == 1)
    pg.reload(); pg.wait_for_timeout(1500)
    check("pin kept after reload", pg.locator(".wl-dark .pin").count() == 1)
    pg.evaluate("document.documentElement.dataset.theme='light'")
    check("light variant visible", pg.locator(".wl-light").is_visible() and not pg.locator(".wl-dark").is_visible())
    pg.evaluate("document.documentElement.dataset.theme='dark'")
    check("dark variant visible", pg.locator(".wl-dark").is_visible() and not pg.locator(".wl-light").is_visible())
    check("version shown", "0." in pg.inner_text(".wl-dark .stats"))
    pg.locator(".wl-dark .w-restore").click()
    poll(lambda: pg.locator("#tabs .tab").count() >= 2)
    check("restore opens the two tabs", pg.locator("#tabs .tab").count() == 2, pg.locator("#tabs .tab").count())
    pg.evaluate("() => { for (const t of Tabs.list.slice()) Tabs.close(t); }"); pg.wait_for_timeout(500)
    check("welcome returns", pg.locator("#welcome").count() == 1)
    check("no page errors", not errs, errs)
    g.stop()
print(f"\n{PASS} passed, {FAIL} failed"); sys.exit(1 if FAIL else 0)
