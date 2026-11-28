"""First-run introduction: shown once for a new user, can be skipped, and can be opened again."""
import os, sys, tempfile
sys.path.insert(0, os.path.dirname(__file__))
from ui_common import *
PASS = FAIL = 0
def check(name, cond, detail=""):
    global PASS, FAIL
    if cond: PASS += 1; print(f"  PASS  {name}")
    else: FAIL += 1; print(f"  FAIL  {name}  {str(detail)[:300]}")
def poll(f, t=10):
    import time
    end = time.time() + t
    while time.time() < end:
        try:
            if f(): return True
        except Exception: pass
        time.sleep(0.2)
    return False
os.environ["KEMPT_CONFIG_DIR"] = tempfile.mkdtemp()
with sync_playwright() as pw:
    br = pw.chromium.launch(); g = Gui([]); errs = []
    pg = br.new_page(viewport={"width": 1400, "height": 860}); pg.set_default_timeout(15000)
    pg.on("pageerror", lambda e: errs.append(str(e)))
    pg.goto(g.url + '&tour=1')
    check("a new user sees the introduction", poll(lambda: pg.locator(".tour").count() == 1))
    check("it has seven steps", pg.locator(".tour-dots i").count() == 7, pg.locator(".tour-dots i").count())
    check("first step is the welcome", "Welcome to Kempt" in pg.inner_text(".tour h2"))
    check("Back is hidden on the first step", pg.locator(".t-back").evaluate("e => getComputedStyle(e).visibility") == "hidden")
    pg.keyboard.press("ArrowRight"); pg.wait_for_timeout(150)
    check("arrow keys move on", "Open anything" in pg.inner_text(".tour h2"))
    pg.click(".t-back"); check("Back works", "Welcome" in pg.inner_text(".tour h2"))
    for _ in range(6): pg.click(".t-next")
    check("the last step offers to start", "Start using Kempt" in pg.inner_text(".t-next"))
    pg.click(".t-next"); pg.wait_for_timeout(300)
    check("finishing closes it", pg.locator(".tour").count() == 0)
    pg.wait_for_timeout(1200); pg.goto(g.url + '&tour=1'); pg.wait_for_timeout(2500)
    check("it does not come back by itself", pg.locator(".tour").count() == 0)
    pg.click("#btn-settings"); pg.click(".s-nav [data-p=about]"); pg.click("#set-tour")
    check("Settings > About shows it again", poll(lambda: pg.locator(".tour").count() == 1))
    pg.keyboard.press("Escape"); pg.wait_for_timeout(200)
    check("Escape closes it", pg.locator(".tour").count() == 0)
    pg.click("#btn-help"); pg.wait_for_timeout(500)
    pg.get_by_role("button", name="Introduction").click()
    check("Help > Introduction opens it", poll(lambda: pg.locator(".tour").count() == 1))
    pg.click(".t-skip"); pg.wait_for_timeout(200)
    check("Skip closes it", pg.locator(".tour").count() == 0)
    # someone who already uses Kempt (recent files) is not interrupted
    pg.evaluate("() => { prefs.set('tourDone', false); prefs.set('recent', ['/x/a.json']); }"); pg.wait_for_timeout(800); pg.goto(g.url + '&tour=1'); pg.wait_for_timeout(2500)
    check("an existing user is not interrupted", pg.locator(".tour").count() == 0)
    # an automated browser never gets it unless asked (so other UI tests are not blocked)
    pg.evaluate("() => { prefs.set('tourDone', false); prefs.set('recent', []); }"); pg.wait_for_timeout(800); pg.goto(g.url); pg.wait_for_timeout(2500)
    check("not shown to an automated browser by default", pg.locator(".tour").count() == 0)
    check("no page errors", not errs, errs)
    g.stop()
print(f"\n{PASS} passed, {FAIL} failed"); sys.exit(1 if FAIL else 0)
