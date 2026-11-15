"""World clock: live seconds, draggable sections, meeting-finder calendar, company holidays and weekend days."""
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
    pg = br.new_page(viewport={"width": 1600, "height": 1000}); pg.set_default_timeout(20000)
    pg.on("pageerror", lambda e: errs.append(str(e)))
    pg.goto(g.url); pg.wait_for_timeout(1000); pg.click("#btn-clock"); pg.wait_for_selector(".wc .hero"); pg.wait_for_timeout(1200)
    t1 = pg.inner_text(".wc .hc1"); pg.wait_for_timeout(2200); t2 = pg.inner_text(".wc .hc1")
    check("big clock shows seconds and ticks", t1 != t2 and t1.count(":") == 2, (t1, t2))
    check("zone rows show seconds", pg.inner_text(".wc .zrow .tm b").count(":") == 2)
    w = pg.evaluate("document.querySelector('.wc .grid').getBoundingClientRect().width")
    check("page uses the full width", w > 1400, w)
    # sections reorder by keyboard and the order is remembered
    pg.focus(".wc section[data-f=hol] .sgrip"); pg.keyboard.press("Alt+ArrowUp"); pg.keyboard.press("Alt+ArrowUp"); pg.wait_for_timeout(200)
    check("section moved with Alt+Up", pg.evaluate("[...document.querySelectorAll('.wc .secs > section')].map(s=>s.dataset.f)") == ["dst", "hol", "dal", "meet"])
    check("order is saved", pg.evaluate("prefs.get('wcsec')") == ["dst", "hol", "dal", "meet"])
    # company holidays: add, scope, import, hide a built-in, weekend days
    pg.select_option(".wc .hc", "IN"); pg.fill(".wc .hd", "2027-03-10"); pg.fill(".wc .hn", "Founders day"); pg.select_option(".wc .hs", "IN"); pg.click(".wc .hgo")
    pg.click(".wc .hbulk"); pg.fill(".wc .hta", "2027-04-01 Offsite\n15/08/2027, Company day\nnot a date"); pg.click(".wc .hdo"); pg.wait_for_timeout(300)
    own = pg.evaluate("HOL.custom()")
    check("own holidays added and imported", len(own) == 3 and own[0]["c"] == "IN", own)
    check("bad import lines stay in the box", pg.input_value(".wc .hta").strip() == "not a date")
    check("scoped holiday applies only to its country", pg.evaluate("HOL.on('2027-03-10','IN').join()") == "Founders day" and pg.evaluate("HOL.on('2027-03-10','US').length") == 0)
    n0 = pg.locator(".wc .hl:not(.hl2) > div").count(); first = pg.inner_text(".wc .hl:not(.hl2) .hnm >> nth=0"); pg.click(".wc .hhide >> nth=0"); pg.wait_for_timeout(200)
    check("a built-in holiday can be hidden", pg.locator(".wc .hl:not(.hl2) > div").count() < n0 and pg.evaluate("HOL.on('2026-12-25','IN').length") == 0, first)
    pg.click(".wc .hrest"); pg.wait_for_timeout(200)
    check("and restored", pg.evaluate("HOL.on('2026-12-25','IN').length") == 1)
    pg.check(".wc .hwe input[data-d='5']"); pg.wait_for_timeout(200)
    check("weekend days are configurable", pg.evaluate("HOL.weekend()") == [0, 5, 6])
    pg.locator(".wc .hdl >> nth=0").click(); pg.wait_for_timeout(200)
    check("a company holiday can be removed", len(pg.evaluate("HOL.custom()")) == 2)
    # meeting finder calendar
    days = pg.locator(".wc .mcg button"); n = days.count(); days.nth(n - 1).click(); pg.wait_for_timeout(500)
    st = pg.evaluate("prefs.get('meet').start")
    check("calendar sets the search start day", bool(st), st)
    check("calendar marks the start day", pg.locator(".wc .mcg button.start").count() == 1 or pg.evaluate("HOL.ymd0(new Date())") >= st)
    pg.click(".wc .mcnow"); pg.wait_for_timeout(300)
    check("Today clears the start day", not pg.evaluate("prefs.get('meet').start"))
    check("no page errors", not errs, errs)
    br.close(); g.stop()
print(f"\n{PASS} passed, {FAIL} failed"); sys.exit(1 if FAIL else 0)
