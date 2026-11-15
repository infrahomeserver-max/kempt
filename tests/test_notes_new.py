"""Checklist: grouped by day, stars, search, calendar side panel; several notes with titles."""
import os, sys
sys.path.insert(0, os.path.dirname(__file__))
from ui_common import *

PASS = FAIL = 0
def check(name, cond, detail=""):
    global PASS, FAIL
    if cond: PASS += 1; print(f"  PASS  {name}")
    else: FAIL += 1; print(f"  FAIL  {name}  {str(detail)[:300]}")

os.environ["UI_EDITABLE"] = "1"
with sync_playwright() as pw:
    br = pw.chromium.launch(); g = Gui([]); errs = []
    pg = br.new_page(viewport={"width": 1700, "height": 950}); pg.set_default_timeout(20000)
    pg.on("pageerror", lambda e: errs.append(str(e)))
    pg.goto(g.url); pg.wait_for_timeout(800)
    pg.evaluate("""(()=>{const n=Date.now(),H=3600e3;Notes.items=[{id:'a',t:'Pay rent',due:n-30*H,c:'p',fired:true},{id:'b',t:'Report',due:n+1*H,c:'w'},{id:'c',t:'Dentist',due:n+26*H,c:'p'},{id:'d',t:'Plan',c:'w'},{id:'f',t:'Old',done:true,c:'w'}];Notes.docs=[];CAL.events=[];CAL.loaded=true;})()""")
    pg.click("#btn-notes"); pg.wait_for_selector(".nt .it"); pg.wait_for_timeout(500)
    heads = pg.evaluate("[...document.querySelectorAll('.nt .gh span')].map(e=>e.textContent)")
    check("items grouped under headings", heads[0] == "Overdue" and "No date" in heads and heads[-1] == "Done", heads)
    pg.click(".nt .it[data-id=d] .star"); pg.wait_for_timeout(200)
    check("star toggles", pg.evaluate("Notes.items.find(i=>i.id==='d').s") == 1)
    pg.fill(".nt .fq", "dent"); pg.wait_for_timeout(200)
    check("search filters", pg.locator(".nt .it").count() == 1)
    pg.fill(".nt .fq", "")
    pg.click(".nt .mg button.today"); pg.wait_for_timeout(200)
    check("clicking a day filters the list", not pg.locator(".nt .dayf").is_hidden() and pg.locator(".nt .it").count() <= 2)
    pg.click(".nt .dayf button"); pg.wait_for_timeout(200)
    check("show all days", pg.locator(".nt .dayf").is_hidden())
    check("agenda lists upcoming tasks", pg.locator(".nt .ae.task").count() >= 1)
    # quick chip fills the time
    pg.click(".nt .qk button[data-q=tom]"); check("quick chip sets the time", pg.input_value(".nt .w") != "")
    # multiple notes
    pg.click(".nt .nn"); pg.fill(".nt .ntt", "First"); pg.fill(".nt .pad", "alpha body")
    pg.click(".nt .nn"); pg.fill(".nt .ntt", "Second"); pg.fill(".nt .pad", "beta body"); pg.wait_for_timeout(300)
    check("two notes listed", pg.locator(".nt .ni").count() == 2)
    pg.click(".nt .ni:has-text('First')"); check("switching shows its text", pg.input_value(".nt .pad") == "alpha body")
    pg.fill(".nt .nsq", "beta"); check("note search", pg.locator(".nt .ni").count() == 1); pg.fill(".nt .nsq", "")
    pg.evaluate("Notes.flush()"); pg.wait_for_timeout(500)
    pg.reload(); pg.wait_for_timeout(800); pg.click("#btn-notes"); pg.wait_for_selector(".nt .ni")
    check("notes persist", pg.locator(".nt .ni").count() == 2)
    pg.once("dialog", lambda d: d.accept()); pg.click(".nt .nd"); pg.wait_for_timeout(300)
    check("note deleted", pg.locator(".nt .ni").count() == 1)
    check("no page errors", not errs, errs)
    g.stop(); br.close()
print(f"\n{PASS} passed, {FAIL} failed"); sys.exit(1 if FAIL else 0)
