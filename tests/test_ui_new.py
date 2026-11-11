"""Browser tests: New file without saving, folder icons in the file picker, compact tab bar, Meeting finder blocks."""
import os, sys, tempfile
sys.path.insert(0, os.path.dirname(__file__))
from ui_common import *

PASS = FAIL = 0
def check(name, cond, detail=""):
    global PASS, FAIL
    if cond: PASS += 1; print(f"  PASS  {name}")
    else: FAIL += 1; print(f"  FAIL  {name}  {str(detail)[:400]}")

d = tempfile.mkdtemp(); os.mkdir(os.path.join(d, "sub")); open(os.path.join(d, "a.txt"), "w").write("x\n")
os.environ["UI_EDITABLE"] = "1"
with sync_playwright() as pw:
    browser = pw.chromium.launch(); g = Gui([]); errs = []
    pg = browser.new_page(viewport={"width": 1300, "height": 800}); pg.set_default_timeout(20000)
    pg.on("pageerror", lambda e: errs.append(str(e)))
    pg.goto(g.url); pg.wait_for_timeout(1500)
    check("tab bar has no vertical scroll", pg.evaluate("(()=>{const t=document.querySelector('#tabs');return getComputedStyle(t).overflowY==='hidden'})()"))
    # file picker: folders and files look different
    pg.click("#w-open"); pg.wait_for_timeout(500)
    pg.fill(".pick-path .pp", d); pg.press(".pick-path .pp", "Enter"); pg.wait_for_timeout(600)
    col = pg.evaluate("""(()=>{const c=s=>{const e=document.querySelector(s);return e?getComputedStyle(e).color:null};
      return [c('.pick-item.dir .pi .ic'), c('.pick-item:not(.dir) .pi .ic')]})()""")
    check("folder icon and file icon have different colours", col[0] and col[1] and col[0] != col[1], col)
    check("folders are marked and listed first", pg.locator(".pick-item").first.get_attribute("class").find("dir") >= 0)
    pg.keyboard.press("Escape"); pg.wait_for_timeout(300)
    # new file kept in memory
    pg.click("#btn-new"); pg.wait_for_timeout(400)
    pg.fill("#nf-name", "idea.md")
    pg.get_by_role("button", name="Start without saving").click(); pg.wait_for_timeout(900)
    check("new file opens as a tab with the unsaved dot", "idea.md" in pg.inner_text("#tabs") and pg.locator("#tabs .dot").count() == 1, pg.inner_text("#tabs"))
    check("nothing was written to disk", not any(n.startswith("idea") for n in os.listdir(os.path.expanduser("~"))) and not os.path.exists(os.path.join(os.getcwd(), "idea.md")))
    # meeting finder
    pg.keyboard.press("Escape"); pg.keyboard.press("Alt+k"); pg.wait_for_timeout(1200)
    check("meeting finder: rarely used options are folded away", pg.locator(".mmore").count() == 1 and not pg.locator(".mmore").evaluate("e=>e.open"))
    rows = pg.locator(".mres .mr")
    if rows.count():
        txt = rows.first.inner_text()
        check("meeting finder: a block shows a time range", "–" in txt, txt)
        check("meeting finder: at most 8 blocks", rows.count() <= 8, rows.count())
    else:
        check("meeting finder: shows a message when nothing fits", pg.locator(".mres .none").count() == 1)
    pg.screenshot(path="/tmp/ui_new_meeting.png")
    check("no page errors", not errs, errs)
    browser.close(); g.stop()
print(f"\n{PASS} passed, {FAIL} failed"); sys.exit(1 if FAIL else 0)
