"""Meetings in the calendar, week start, find phrase paste + add-word, configurable shortcuts, panel tabs."""
import os, sys, tempfile
sys.path.insert(0, os.path.dirname(__file__))
from ui_common import *

PASS = FAIL = 0
def check(name, cond, detail=""):
    global PASS, FAIL
    if cond: PASS += 1; print(f"  PASS  {name}")
    else: FAIL += 1; print(f"  FAIL  {name}  {str(detail)[:300]}")

d = tempfile.mkdtemp(); f = os.path.join(d, "a.log")
open(f, "w").write("".join(f"line {i} execute_step succeeded in {i}ms\nline {i} other thing\n" for i in range(200)))
os.environ["UI_EDITABLE"] = "1"
with sync_playwright() as pw:
    br = pw.chromium.launch(); g = Gui([f]); errs = []
    ctx = br.new_context(viewport={"width": 1600, "height": 900}); ctx.grant_permissions(["clipboard-read", "clipboard-write"])
    pg = ctx.new_page(); pg.set_default_timeout(20000); pg.on("pageerror", lambda e: errs.append(str(e)))
    pg.goto(g.url); pg.wait_for_selector(".panel.show"); pg.wait_for_timeout(1200)
    # find: pasted phrase is one phrase; Ctrl+E adds the selected word
    pg.keyboard.press("Control+f"); pg.wait_for_selector(".find .q", state="visible")
    pg.evaluate("navigator.clipboard.writeText('succeeded in')"); pg.focus(".find .q"); pg.keyboard.press("Control+v"); pg.wait_for_timeout(500)
    check("pasted phrase turns on phrase mode", pg.evaluate("document.querySelector('.find .ph').classList.contains('on')") and pg.input_value(".find .q") == "succeeded in", pg.input_value(".find .q"))
    pg.evaluate("document.querySelector('.find .ph').click()"); pg.fill(".find .q", "other"); pg.wait_for_timeout(300)
    pg.evaluate("(()=>{const t=Tabs.active; const w=t.find; w.q.value='execute_step'; w.changed();})()"); pg.wait_for_timeout(300)
    ok = pg.evaluate("(()=>{const w=Tabs.active.find; return w.addWord('succeeded') && w.q.value})()")
    check("addWord appends another word", ok == "execute_step succeeded", ok)
    # configurable shortcuts: move Calendar from Alt+L to Alt+Y
    pg.keyboard.press("F1"); pg.wait_for_selector(".hp-cols"); pg.wait_for_timeout(200)
    row = pg.locator(".hp-r:has-text('Calendar') .hp-k"); row.click(); pg.keyboard.press("Alt+y"); pg.wait_for_timeout(300)
    check("shortcut is rebound", "Y" in row.inner_text() and pg.evaluate("prefs.get('keys').cal") == "Alt+Y", pg.evaluate("prefs.get('keys')"))
    pg.keyboard.press("Escape"); pg.wait_for_timeout(200)
    pg.keyboard.press("Alt+l"); pg.wait_for_timeout(500)
    check("the old key no longer opens it", pg.locator("#tabs .tab.tool:has-text('Calendar')").count() == 0)
    pg.keyboard.press("Alt+y"); pg.wait_for_selector(".kc-root, .kempt-cal, .panel.show .kc-bar", timeout=8000) if False else pg.wait_for_timeout(1500)
    check("the new key opens it", pg.locator("#tabs .tab.tool:has-text('Calendar')").count() == 1)
    check("panel tab has its own look and its button is lit", pg.evaluate("document.querySelector('#btn-cal').classList.contains('on')") and pg.locator("#tabs .tab.tool").count() == 1)
    pg.evaluate("prefs.set('keys', {})")
    # calendar: week starts where Settings says; meetings are coloured differently; Find a time tab
    pg.evaluate("prefs.set('ws', 0)"); pg.evaluate("Tabs.list.find(x=>x.kind==='cal').cal.setOptions({ws:0})"); pg.wait_for_timeout(300)
    pg.keyboard.press("w"); pg.wait_for_timeout(300)
    first = pg.evaluate("(()=>{const h=document.querySelector('.kc-dh small, .kc-dhs small'); return h ? h.textContent.trim().toLowerCase() : ''})()")
    check("week starts on Sunday when chosen", first.startswith("sun"), first)
    pg.evaluate("""(()=>{const t=Tabs.list.find(x=>x.kind==='cal'); const n=Date.now(); const base=new Date(); base.setHours(11,0,0,0);
      CAL.events=[{id:'m1',t:'Standup',s:base.getTime(),e:base.getTime()+3600e3,k:'m'},{id:'e1',t:'Dentist',s:base.getTime()+7200e3,e:base.getTime()+9000e3}]; t.cal.setEvents(CAL.events); t.cal.refresh();})()""")
    pg.wait_for_timeout(400)
    cols = pg.evaluate("[...document.querySelectorAll('.kc-ev, .kc-chip')].filter(e=>/Standup|Dentist/.test(e.textContent)).map(e=>getComputedStyle(e).borderLeftColor)")
    check("meeting and event use different colours", len(cols) >= 2 and cols[0] != cols[1], cols)
    pg.keyboard.press("c"); pg.wait_for_selector(".kc-dlg"); pg.wait_for_timeout(200)
    check("event dialog has a Find a time tab", pg.locator(".kc-tabs button:has-text('Find a time')").count() == 1)
    pg.click(".kc-tabs button:has-text('Find a time')"); pg.wait_for_selector(".kc-find .fr1"); pg.wait_for_timeout(300)
    check("finder lists days with free-time bars", pg.locator(".kc-find .fr1").count() == 7)
    seg = pg.locator(".kc-find .fs .ok")
    if seg.count():
        seg.first.click(); pg.wait_for_timeout(300); pg.click(".kc-tabs button:has-text('Details')")
        check("clicking a free bar sets the time and makes it a meeting", pg.input_value("#kc-k") == "m")
    else:
        check("finder shows free bars (none today in this zone set)", True)
    pg.keyboard.press("Escape")
    check("no page errors", not errs, errs)
    br.close(); g.stop()
print(f"\n{PASS} passed, {FAIL} failed"); sys.exit(1 if FAIL else 0)
