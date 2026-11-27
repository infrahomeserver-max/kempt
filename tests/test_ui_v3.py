"""Tools menu, date format and picker, timezone search, file icons, Table/Fix relevance, wrap pane, selection count, Office/image/PDF opening."""
import os, sys, tempfile, zipfile, base64
sys.path.insert(0, os.path.dirname(__file__)); os.environ["UI_EDITABLE"] = "1"
from ui_common import *

fails = 0
def check(n, ok, extra=""):
    global fails
    print(("PASS " if ok else "FAIL ") + n + ("" if ok else f"  {extra}"))
    fails += 0 if ok else 1

d = tempfile.mkdtemp()
open(os.path.join(d, "a.csv"), "w").write("a,b\n1,2\n")
open(os.path.join(d, "t.txt"), "w").write("x\ty\n1\t2\n3\t4\n")
open(os.path.join(d, "n.txt"), "w").write("just some prose here\nand more\n")
open(os.path.join(d, "p.png"), "wb").write(base64.b64decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=="))
with zipfile.ZipFile(os.path.join(d, "d.docx"), "w") as z:
    z.writestr("word/document.xml", "<w:document><w:body><w:p><w:r><w:t>Hello docx</w:t></w:r></w:p></w:body></w:document>")
with sync_playwright() as pw:
    br = pw.chromium.launch(); g = Gui([os.path.join(d, "n.txt")]); pg = br.new_page(viewport={"width": 1400, "height": 800}); errs = []
    pg.on("pageerror", lambda e: errs.append(str(e))); pg.goto(g.url); pg.wait_for_selector(".panel.show"); pg.wait_for_timeout(800)
    check("one Tools dropdown, no topbar Compare", pg.locator("#btn-tools").count() == 1 and pg.locator("#btn-compare").count() == 0)
    pg.evaluate("document.getElementById('btn-tools').click()"); pg.wait_for_timeout(150)
    check("Tools menu lists clock, calendar, paste & compare, checklist", pg.locator(".tmenu button").count() == 4)
    check("Table disabled for prose", pg.evaluate("document.querySelector('.b-tbl').disabled"))
    check("Fix disabled for prose", pg.evaluate("document.querySelector('.b-fix').disabled"))
    pg.evaluate(f"actions.openPath({os.path.join(d,'t.txt')!r})"); pg.wait_for_timeout(900)
    check("Table enabled for tab-delimited txt", not pg.evaluate("document.querySelector('.panel.show .b-tbl').disabled"))
    check("single Compare button", pg.locator(".panel.show .b-cmp").count() == 1)
    check("Changes label is short", "Changes vs saved" not in pg.inner_text(".panel.show .toolbar"))
    check("wrap button present", pg.locator(".panel.show .b-wrap").count() == 1)
    pg.evaluate("document.querySelector('.panel.show .b-wrap').click()"); pg.wait_for_timeout(200)
    check("wrap pane opens", pg.locator(".panel.show .wrapp").count() == 1)
    check("default date format is YYYY/MM/DD", pg.evaluate("DF.fmt(2026,12,31)") == "2026/12/31", pg.evaluate("DF.fmt(2026,12,31)"))
    for q, want in [("cst", "Chicago"), ("pst", "Los_Angeles"), ("central", "Chicago"), ("cst6cdt", "Chicago")]:
        got = pg.evaluate(f"ZP.list({q!r}, new Set()).map(z => typeof z === 'string' ? z : z.id || z.z || '').join(' ')")
        check(f"zone search '{q}' finds {want}", want in got, got[:100])
    check("folder and file icon colours differ", pg.evaluate("FI.ic('x',16,true) !== FI.ic('x.json',16,false) && FI.ic('x.json',16,false) !== FI.ic('x.py',16,false)"))
    pg.evaluate("document.getElementById('btn-notes').click()"); pg.wait_for_timeout(600)
    check("sort select exists, no calendar button", pg.locator(".nt .so").count() == 1 and pg.locator(".nt .opc").count() == 0)
    pg.locator(".nt .clr, .nt button:has-text('Clear done')").first.click(); pg.wait_for_timeout(300)
    check("clear done with nothing shows a message", "Nothing to clear" in pg.inner_text("body"))
    pg.evaluate("document.getElementById('btn-clock').click()"); pg.wait_for_timeout(900)
    check("DST tab has no forced zones", "None of your zones use daylight saving" in pg.inner_text(".wc") or pg.locator(".wc .dstc .x").count() == 0)
    pg.evaluate(f"actions.openPath({os.path.join(d,'d.docx')!r})"); pg.wait_for_timeout(700)
    check("docx opens as text", "Hello docx" in pg.inner_text(".panel.show"))
    pg.evaluate(f"actions.openPath({os.path.join(d,'p.png')!r})"); pg.wait_for_timeout(700)
    check("png opens in a picture tab", pg.evaluate("Tabs.active.kind") == "media" and pg.locator(".panel.show img").count() == 1)
    pg.evaluate("document.getElementById('btn-help').click()") if pg.locator("#btn-help").count() else None
    pg.keyboard.press("Escape"); pg.wait_for_timeout(200); pg.click("#btn-settings"); pg.wait_for_timeout(200)
    pg.click(".sw1[title=Purple]"); pg.select_option("#set-mono", "sys"); pg.click(".s-nav [data-p=tools]"); pg.select_option("#set-c12", "1"); pg.select_option("#set-hf", "8"); pg.select_option("#set-ht", "18")
    check("accent colour applies", pg.evaluate("getComputedStyle(document.documentElement).getPropertyValue('--accent').trim()") == "#8b5cf6")
    check("clock format and working hours are saved", pg.evaluate("prefs.get('clock12') === true && FIND.cfg().from === 8 && FIND.cfg().to === 18"))
    pg.click("button[data-p=about]"); check("About shows the version", "version" in pg.inner_text("#s-ver"))
    pg.click("button[data-p=startup]"); pg.click("#set-reset"); pg.wait_for_selector(".modal .primary, .modal .danger"); pg.keyboard.press("Enter"); pg.wait_for_timeout(1500)
    check("Reset preferences restores the defaults", pg.evaluate("prefs.get('accent', '')") == "" and pg.evaluate("prefs.get('clock12', false)") is False)
    check("no page errors", not errs, errs)
    br.close(); g.stop()
sys.exit(1 if fails else 0)
