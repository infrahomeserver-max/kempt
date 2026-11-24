"""Calendar: drag an event to another day (month), another time and day (week), cancel with Escape, and move one occurrence of a repeating event."""
import os, sys, json, tempfile
sys.path.insert(0, os.path.dirname(__file__))
from ui_common import *

PASS = FAIL = 0
def check(name, cond, detail=""):
    global PASS, FAIL
    if cond: PASS += 1; print(f"  PASS  {name}")
    else: FAIL += 1; print(f"  FAIL  {name}  {str(detail)[:400]}")

os.environ["UI_EDITABLE"] = "1"
cfg = tempfile.mkdtemp(); os.environ["KEMPT_CONFIG_DIR"] = cfg
with sync_playwright() as pw:
    browser = pw.chromium.launch(); g = Gui([]); errs = []
    pg = browser.new_page(viewport={"width": 1300, "height": 900}); pg.set_default_timeout(20000)
    pg.on("pageerror", lambda e: errs.append(str(e)))
    pg.goto(g.url); pg.wait_for_timeout(1500)
    pg.evaluate("document.getElementById('btn-cal').click()"); pg.wait_for_selector(".kc-month")
    def reopen(view):
        pg.reload(); pg.wait_for_timeout(1200); pg.evaluate("document.getElementById('btn-cal').click()"); pg.wait_for_selector(".kc")
        pg.evaluate("document.querySelector('.kc').focus()"); pg.keyboard.press({"month": "m", "week": "w"}[view])
        pg.wait_for_selector(".kc-month" if view == "month" else ".kc-cols"); pg.wait_for_timeout(300)
    def events(): return pg.evaluate("CAL.events.filter(e=>!e.x)")
    def drag(src, dst, dx=0, dy=0, steps=8, release=True):
        a = src.bounding_box(); b = dst.bounding_box()
        pg.mouse.move(a["x"] + a["width"] / 2, a["y"] + a["height"] / 2); pg.mouse.down()
        pg.mouse.move(a["x"] + a["width"] / 2 + 8, a["y"] + a["height"] / 2 + 8)
        pg.mouse.move(b["x"] + b["width"] / 2 + dx, b["y"] + b["height"] / 2 + dy, steps=steps)
        if release: pg.mouse.up()
        pg.wait_for_timeout(250)
    # make a timed event in the middle of the month so every cell is a valid target
    mid = pg.evaluate("(()=>{const d=new Date(); d.setDate(15); d.setHours(9,30,0,0); return d.getTime()})()")
    pg.evaluate(f"CAL.put({{id:'t1',t:'Drag me',s:{mid},e:{mid}+3600000,u:1}}); CAL.flush();")
    reopen("month")
    chip = pg.locator(".kc-chip", has_text="Drag me").first
    check("event shows on day 15", chip.count() == 1)
    cells = pg.locator(".kc-cell:not(.out)")
    target = cells.nth(19)  # day 20
    drag(chip, target)
    e = [x for x in events() if x["id"] == "t1"][0]
    d = pg.evaluate(f"new Date({e['s']}).getDate()")
    check("month drag moves the event to day 20", d == 20, d)
    check("month drag keeps the time of day", pg.evaluate(f"(()=>{{const x=new Date({e['s']}); return x.getHours()*60+x.getMinutes()}})()") == 9 * 60 + 30)
    check("month drag keeps the duration", e["e"] - e["s"] == 3600000, e["e"] - e["s"])
    check("a drag does not open the editor", pg.locator(".kc-dlg").count() == 0)
    # a plain click still opens it
    pg.locator(".kc-chip", has_text="Drag me").first.click(); pg.wait_for_selector(".kc-dlg"); pg.keyboard.press("Escape"); pg.wait_for_timeout(150)
    check("a click still opens the editor", True)
    # Escape cancels a drag in progress
    chip = pg.locator(".kc-chip", has_text="Drag me").first
    drag(chip, cells.nth(2), release=False); pg.keyboard.press("Escape"); pg.mouse.up(); pg.wait_for_timeout(250)
    e2 = [x for x in events() if x["id"] == "t1"][0]
    check("Escape cancels the drag", e2["s"] == e["s"])
    # week view: drag to another time and day
    # a second event today, so the week view that opens shows it
    now = pg.evaluate("(()=>{const d=new Date(); d.setHours(10,0,0,0); return d.getTime()})()")
    pg.evaluate(f"CAL.put({{id:'w1',t:'Week mover',s:{now},e:{now}+3600000,u:2}}); CAL.flush();")
    reopen("week")
    ev = pg.locator(".kc-ev", has_text="Week mover").first
    cols = pg.locator(".kc-col"); n = cols.count()
    ci = pg.evaluate("[...document.querySelectorAll('.kc-col')].findIndex(c=>c.contains(document.querySelector('.kc-ev')))")
    nxt = cols.nth((ci + 1) % n)
    drag(ev, nxt, dy=96)   # two hours lower (48 px per hour)
    w = [x for x in events() if x["id"] == "w1"][0]
    moved_day = pg.evaluate(f"new Date({w['s']}).getDate() !== new Date({now}).getDate()")
    check("week drag moves the event to another day column", moved_day, w)
    check("week drag changes the time of day", pg.evaluate(f"new Date({w['s']}).getHours()*60+new Date({w['s']}).getMinutes()") != 10 * 60, w)
    check("week drag snaps to 15 minutes", pg.evaluate(f"new Date({w['s']}).getMinutes() % 15") == 0)
    check("week drag keeps the duration", w["e"] - w["s"] == 3600000)
    # repeating event: only the dragged occurrence moves
    start = pg.evaluate("(()=>{const d=new Date(); d.setDate(1); d.setHours(8,0,0,0); return d.getTime()})()")
    pg.evaluate(f"CAL.put({{id:'r1',t:'Daily rep',s:{start},e:{start}+1800000,r:{{f:'d',n:1}},u:3}}); CAL.flush();")
    reopen("month")
    before = len(events())
    src = pg.locator(".kc-cell:not(.out)").nth(9).locator(".kc-chip", has_text="Daily rep").first
    drag(src, pg.locator(".kc-cell:not(.out)").nth(12))
    evs = events(); rep = [x for x in evs if x["id"] == "r1"][0]
    check("repeating event keeps its rule", rep.get("r") and rep["r"]["f"] == "d")
    check("dragged day is skipped in the series", len(rep.get("ex", [])) == 1, rep.get("ex"))
    singles = [x for x in evs if x["t"] == "Daily rep" and "r" not in x]
    check("one single copy was made at the new place", len(singles) == 1 and pg.evaluate(f"new Date({singles[0]['s']}).getDate()") == 13, singles)
    check("copy keeps its time and length", len(singles) == 1 and singles[0]["e"] - singles[0]["s"] == 1800000)
    pg.wait_for_timeout(900)
    saved = json.load(open(os.path.join(cfg, "calendar.json")))
    check("moves are saved", any(x["id"] == "t1" and x["s"] == e["s"] for x in saved["events"]) and len(saved["events"]) >= 4, len(saved["events"]))
    # all-day events move by whole days
    d0 = pg.evaluate("(()=>{const d=new Date(); d.setDate(8); d.setHours(0,0,0,0); return d.getTime()})()")
    pg.evaluate(f"CAL.put({{id:'a1',t:'Trip',s:{d0},e:{d0}+2*86400000,ad:true,u:4}}); CAL.flush();")
    reopen("month")
    first = pg.locator(".kc-cell:not(.out)").nth(7).locator(".kc-chip", has_text="Trip").first
    drag(first, pg.locator(".kc-cell:not(.out)").nth(21))
    a = [x for x in events() if x["id"] == "a1"][0]
    check("all-day event moves to the chosen day and keeps two days", pg.evaluate(f"new Date({a['s']}).getDate()") == 22 and abs((a["e"] - a["s"]) / 86400000 - 2) < 0.05, a)
    check("no page errors", not errs, errs)
    browser.close(); g.stop()
print(f"\n{PASS} passed, {FAIL} failed"); sys.exit(1 if FAIL else 0)
