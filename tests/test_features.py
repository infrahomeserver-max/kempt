"""Lite mode: Settings > Features hides what is not used; with one thing left Kempt starts straight in it."""
import os, sys
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
with sync_playwright() as pw:
    br = pw.chromium.launch(); g = Gui([]); errs = []
    pg = br.new_page(viewport={"width": 1400, "height": 860}); pg.set_default_timeout(15000)
    pg.on("pageerror", lambda e: errs.append(str(e)))
    pg.goto(g.url); pg.wait_for_timeout(1200)
    vis = lambda sel: pg.evaluate("s => { const e = document.querySelector(s); return !!e && getComputedStyle(e).display !== 'none'; }", sel)
    check("all features on by default", pg.evaluate("() => Features.off().length") == 0)
    check("clock/notes/calendar buttons exist", vis("#btn-clock") and vis("#btn-notes") and vis("#btn-cal"))
    # untick the clock through Settings
    pg.evaluate("() => document.querySelector('#btn-help') && 0"); pg.click('#btn-settings'); pg.click('.s-nav [data-p=features]'); pg.wait_for_timeout(600)
    boxes = pg.locator("#set-features input")
    check("settings lists six features", boxes.count() == 6, boxes.count())
    if boxes.count() == 6:
        boxes.nth(5).click(); pg.wait_for_timeout(300)
        check("clock hidden after unticking", pg.evaluate("() => Features.off().join()") == "clock" and not pg.evaluate("() => { const e = document.querySelector('#btn-clock'); return getComputedStyle(e).display !== 'none'; }"))
        pg.evaluate("() => actions.clock()"); pg.wait_for_timeout(300)
        check("clock cannot be opened while off", pg.locator("#tabs .tab").count() == 0)
        boxes.nth(5).click(); pg.wait_for_timeout(300)
        check("clock back when ticked again", pg.evaluate("() => Features.off().length") == 0)
    pg.keyboard.press("Escape")
    # lite: only the clock
    pg.evaluate("() => { prefs.set('features', { files: false, compare: false, paste: false, notes: false, cal: false }); }"); pg.wait_for_timeout(1500)
    pg.reload(); pg.wait_for_timeout(1500)
    check("clock-only starts straight in the clock", poll(lambda: pg.evaluate("() => Tabs.list.length === 1 && Tabs.list[0].kind === 'clock'")))
    check("open/new buttons hidden", not vis("#btn-open") and not vis("#btn-new"))
    check("shortcut for a hidden feature does nothing", (pg.keyboard.press("Alt+j"), pg.wait_for_timeout(300), pg.evaluate("() => Tabs.list.length"))[2] == 1)
    pg.evaluate("() => prefs.set('features', {})"); pg.wait_for_timeout(1000)
    check("no page errors", not errs, errs)
    g.stop()
print(f"\n{PASS} passed, {FAIL} failed"); sys.exit(1 if FAIL else 0)
