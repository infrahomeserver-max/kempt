"""Log tools in the Find box: level chips, go to time, extract a time range, saved searches."""
import os, sys, tempfile
sys.path.insert(0, os.path.dirname(__file__))
from ui_common import *
PASS = FAIL = 0
def check(name, cond, detail=""):
    global PASS, FAIL
    if cond: PASS += 1; print(f"  PASS  {name}")
    else: FAIL += 1; print(f"  FAIL  {name}  {str(detail)[:300]}")
def poll(f, t=12):
    end = time.time() + t
    while time.time() < end:
        try:
            if f(): return True
        except Exception: pass
        time.sleep(0.2)
    return False
d = tempfile.mkdtemp(); path = os.path.join(d, "app.log")
lv = ["INFO", "INFO", "WARN", "INFO", "ERROR"]
with open(path, "w") as f:
    for m in range(600):
        f.write(f"2026-11-27 {10 + m // 120:02d}:{(m // 2) % 60:02d}:{(m % 2) * 30:02d}.000 {lv[m % 5]} [w{m % 3}] request {m}\n")
        if m % 50 == 7: f.write("    at com.example.Foo.bar(Foo.java:12)\n    at com.example.Main.run(Main.java:9)\n")
plain = os.path.join(d, "plain.txt"); open(plain, "w").write("hello\nworld\n")
with sync_playwright() as pw:
    br = pw.chromium.launch(); g = Gui([path]); errs = []
    pg = br.new_page(viewport={"width": 1400, "height": 860}); pg.set_default_timeout(15000)
    pg.on("pageerror", lambda e: errs.append(str(e)))
    pg.goto(g.url); poll(lambda: pg.evaluate("() => Tabs.list.length") >= 1); pg.wait_for_timeout(800)
    pg.evaluate("() => Tabs.activate(Tabs.list.find(t => t.info && t.info.name === 'app.log'))"); pg.wait_for_timeout(500)
    pg.evaluate("() => Tabs.active.openFind(false)"); pg.wait_for_timeout(300)
    check("log button is there", pg.locator(".find .lgb:visible").count() >= 1)
    pg.locator(".find .lgb:visible").first.click(); pg.wait_for_timeout(600)
    msg = pg.locator(".find .lgm:visible").first.inner_text()
    check("shows the span of the log", "2026-11-27 10:00:00" in msg and "→" in msg, msg)
    # level chip
    pg.locator(".find .lv button:visible").nth(0).click()
    check("error chip searches errors", poll(lambda: "120" in pg.locator(".find .cnt:visible").first.inner_text()), pg.locator(".find .cnt:visible").first.inner_text())
    pg.locator(".find .lv button:visible").nth(1).click()
    check("error+warn together", poll(lambda: "240" in pg.locator(".find .cnt:visible").first.inner_text()), pg.locator(".find .cnt:visible").first.inner_text())
    pg.locator(".find .lv button:visible").nth(0).click(); pg.locator(".find .lv button:visible").nth(1).click(); pg.wait_for_timeout(400)
    check("chips off clears the search", pg.locator(".find .q:visible").first.input_value() == "")
    # go to time
    gt = pg.locator(".find .gt:visible").first; gt.fill("10:30"); gt.press("Enter"); pg.wait_for_timeout(500)
    msg = pg.locator(".find .lgm:visible").first.inner_text()
    check("go to time lands on 10:30", "10:30:00" in msg and "Line" in msg, msg)
    check("status bar moved", pg.evaluate("() => Tabs.active.curLine") > 40, pg.evaluate("() => Tabs.active.curLine"))
    gt.fill("nonsense"); gt.press("Enter"); pg.wait_for_timeout(400)
    check("bad time says so", "Could not read" in pg.locator(".find .lgm:visible").first.inner_text())
    # extract
    pg.locator(".find .rf:visible").first.fill("10:10"); pg.locator(".find .rt:visible").first.fill("10:19"); pg.locator(".find .rgb:visible").first.click()
    check("range opens a new tab", poll(lambda: pg.evaluate("() => Tabs.list.some(t => /10\\.10-10\\.19/.test((t.info && t.info.name) || ''))")))
    check("range has 10 minutes of lines", poll(lambda: pg.evaluate("() => { const t = Tabs.active; return t && t.info && t.info.lines >= 20 && t.info.lines < 26; }")), pg.evaluate("() => Tabs.active.info.lines"))
    # saved searches
    pg.evaluate("() => Tabs.activate(Tabs.list.find(t => t.info && t.info.name === 'app.log'))"); pg.wait_for_timeout(400)
    if not pg.locator(".find .lgb:visible").count(): pg.evaluate("() => Tabs.active.openFind(false)")
    q = pg.locator(".find .q:visible").first; q.fill("request 5"); pg.wait_for_timeout(500)
    pg.locator(".find .svb:visible").first.click(); pg.wait_for_timeout(300)
    pg.locator("input[type=text]:visible").last.fill("fives"); pg.keyboard.press("Enter"); pg.wait_for_timeout(500)
    check("saved search is listed", "fives" in pg.locator(".find .sv:visible").first.inner_text())
    check("saved search is stored", poll(lambda: "fives" in str(pg.evaluate("() => prefs.get('saved', [])"))))
    q.fill(""); pg.wait_for_timeout(300)
    pg.locator(".find .sv:visible").first.select_option("fives"); pg.wait_for_timeout(400)
    check("saved search restores the text", q.input_value() == "request 5")
    pg.locator(".find .svd:visible").first.click(); pg.wait_for_timeout(300)
    check("saved search can be deleted", "fives" not in pg.locator(".find .sv:visible").first.inner_text())
    # a file without timestamps
    pg.evaluate("p => actions.openPath(p)", plain); pg.wait_for_timeout(1000)
    pg.evaluate("() => Tabs.active.openFind(false)"); pg.wait_for_timeout(300)
    lb = pg.locator(".find .lgb:visible").first
    if not pg.locator(".find .lg:visible").count(): lb.click()
    pg.wait_for_timeout(500)
    check("plain text explains there are no timestamps", "No timestamps" in pg.locator(".find .lgm:visible").first.inner_text())
    check("no page errors", not errs, errs)
    br.close(); g.stop()
print(f"{PASS} passed, {FAIL} failed"); sys.exit(1 if FAIL else 0)
