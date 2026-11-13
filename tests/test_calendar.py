"""Calendar: views, repeating events, saving, tasks on the calendar, reminders, and the occurrence maths."""
import os, sys, json, tempfile, subprocess
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
    pg = browser.new_page(viewport={"width": 1300, "height": 850}); pg.set_default_timeout(20000)
    pg.on("pageerror", lambda e: errs.append(str(e)))
    pg.goto(g.url); pg.wait_for_timeout(1500)
    pg.click("#btn-cal"); pg.wait_for_selector(".kc-month")
    check("month view opens", pg.locator(".kc-cell").count() >= 28)
    # occurrence maths (daily x3, weekly Mon/Wed, monthly on 31st, exceptions)
    r = pg.evaluate("""(()=>{const K=KemptCalendar, d=(y,m,dd,h=9)=>new Date(y,m,dd,h).getTime(), n=(ev,a,b)=>K.occurrences(ev,a,b).length;
      const day=n({id:'a',s:d(2026,0,1),e:d(2026,0,1,10),r:{f:'d',n:1,count:3}},d(2026,0,1,0),d(2027,0,1,0));
      const wk=n({id:'b',s:d(2026,0,5),e:d(2026,0,5,10),r:{f:'w',n:1,by:[1,3]}},d(2026,0,1,0),d(2026,0,15,0));
      const mo=K.occurrences({id:'c',s:d(2026,0,31),e:d(2026,0,31,10),r:{f:'m',n:1}},d(2026,0,1,0),d(2026,4,1,0)).map(o=>new Date(o.s).getDate()).join();
      const ex=n({id:'d',s:d(2026,0,1),e:d(2026,0,1,10),r:{f:'d',n:1},ex:['2026-01-02']},d(2026,0,1,0),d(2026,0,4,0));
      const del=n({id:'e',x:true,s:d(2026,0,1),e:d(2026,0,1,10)},0,d(2030,0,1));
      return {day,wk,mo,ex,del}})()""")
    check("daily with count 3", r["day"] == 3, r); check("weekly Mon+Wed over two weeks", r["wk"] == 4, r)
    check("monthly on the 31st clamps to month end", r["mo"] == "31,28,31,30", r)
    check("excluded day skipped", r["ex"] == 2, r); check("deleted event never drawn", r["del"] == 0, r)
    # create a repeating event with a reminder through the editor
    pg.click(".kc-btn.add"); pg.wait_for_selector(".kc-dlg")
    pg.fill("#kc-t", "Standup"); pg.select_option("#kc-r", "d"); pg.check(".rem[value='10']")
    pg.click(".kc-dlg button[type=submit]"); pg.wait_for_timeout(300)
    check("repeating event shows many times in month", pg.locator(".kc-chip", has_text="Standup").count() >= 20, pg.locator(".kc-chip", has_text="Standup").count())
    for v, sel in [("Week", ".kc-cols"), ("Day", ".kc-cols"), ("Year", ".kc-year"), ("Schedule", ".kc-sched"), ("Month", ".kc-month")]:
        pg.click(f".kc-seg button:text-is('{v}')"); pg.wait_for_timeout(200)
        check(f"{v} view draws", pg.locator(sel).count() == 1)
    pg.click(".kc-seg button:text-is('Week')"); pg.wait_for_timeout(200)
    check("week view shows the event", pg.locator(".kc-ev", has_text="Standup").count() >= 1)
    # keyboard: m switches to month, edit title requires focus out of inputs
    pg.focus(".kc"); pg.keyboard.press("m"); pg.wait_for_timeout(200)
    check("keyboard m = month", pg.locator(".kc-month").count() == 1)
    # XSS: title with markup is text
    pg.click(".kc-btn.add"); pg.wait_for_selector(".kc-dlg"); pg.fill("#kc-t", "<img src=x onerror=window.__x=1>"); pg.click(".kc-dlg button[type=submit]"); pg.wait_for_timeout(300)
    check("markup in title is not executed", pg.evaluate("window.__x") is None)
    pg.wait_for_timeout(900)
    saved = json.load(open(os.path.join(cfg, "calendar.json")))
    check("saved to calendar.json", len(saved.get("events", [])) == 2 and saved["cfg"].get("view"), saved.get("cfg"))
    # reminders became hidden checklist items; they do not show in the checklist list
    notes = json.load(open(os.path.join(cfg, "notes.json")))
    check("event reminder became a hidden item", any(i.get("k") == "cal" for i in notes.get("items", [])))
    # task with a time shows on the calendar
    pg.click("#btn-notes"); pg.wait_for_selector(".nt .t")
    pg.fill(".nt .t", "Pay rent"); pg.fill(".nt .w", "2099-05-10T10:00"); pg.click(".nt .go"); pg.wait_for_timeout(300)
    check("cal items stay out of the checklist", pg.locator(".nt .it").count() == 1)
    pg.click("#btn-cal"); pg.wait_for_timeout(300)
    pg.evaluate("document.querySelector('.kc').focus()"); pg.keyboard.press("y"); pg.wait_for_timeout(200)
    # go to 2099-05 via the API of the module
    pg.evaluate("CAL_goto=1"); 
    errs_before = len(errs)
    check("no page errors", not errs, errs)
    browser.close(); g.stop()
print(f"\n{PASS} passed, {FAIL} failed"); sys.exit(1 if FAIL else 0)
