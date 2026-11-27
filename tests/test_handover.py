"""Second `kempt gui <file>` and drag & drop open new tabs inside the open window."""
import os, sys, tempfile, subprocess, json
sys.path.insert(0, os.path.dirname(__file__))
from ui_common import *

PASS = FAIL = 0
def check(name, cond, detail=""):
    global PASS, FAIL
    if cond: PASS += 1; print(f"  PASS  {name}")
    else: FAIL += 1; print(f"  FAIL  {name}  {str(detail)[:300]}")

os.environ["UI_EDITABLE"] = "1"
cfg = tempfile.mkdtemp(); os.environ["KEMPT_CONFIG_DIR"] = cfg
d = tempfile.mkdtemp()
a, b, c = (os.path.join(d, n) for n in ("a.json", "b.json", "c.txt"))
open(a, "w").write('{"x":1}'); open(b, "w").write('{"x":2}'); open(c, "w").write("hello drop")
with sync_playwright() as pw:
    browser = pw.chromium.launch(); g = Gui([a]); errs = []
    pg = browser.new_page(viewport={"width": 1300, "height": 800}); pg.set_default_timeout(20000)
    pg.on("pageerror", lambda e: errs.append(str(e)))
    pg.goto(g.url); pg.wait_for_timeout(2000)
    check("starts with one tab", pg.locator("#tabs .tab").count() == 1, pg.locator("#tabs .tab").count())
    # second launch hands over
    env = dict(os.environ, KEMPT_CONFIG_DIR=cfg)
    r = subprocess.run([BIN, "gui", b], env=env, capture_output=True, text=True, timeout=20)
    check("second launch says added", "already open" in r.stdout, r.stdout + r.stderr)
    pg.wait_for_timeout(1500)
    check("second file is a new tab in the same window", pg.locator("#tabs .tab").count() == 2, pg.locator("#tabs .tab").count())
    # drop a file (no path available -> upload copy)
    pg.evaluate("""() => {
      const dt = new DataTransfer(); dt.items.add(new File(['hello drop'], 'c.txt', {type:'text/plain'}));
      document.body.dispatchEvent(new DragEvent('drop', {dataTransfer: dt, bubbles: true, cancelable: true}));
    }""")
    pg.wait_for_timeout(1500)
    check("dropped file opens as a tab", pg.locator("#tabs .tab").count() == 3, pg.locator("#tabs .tab").count())
    check("dropped tab is c.txt", "c.txt" in pg.locator("#tabs .tab").nth(2).inner_text())
    # background: the tab title is marked and a notification is sent; coming back restores the title
    pg.evaluate("""() => { window.__n = []; window.Notification = class { constructor(t, o) { window.__n.push([t, o.body]); } close() {} static get permission() { return 'granted'; } };
      Object.defineProperty(document, 'hidden', { configurable: true, get: () => true }); attention('c.txt'); }""")
    check("title marked while hidden", pg.title().startswith("\u25cf Opened c.txt"), pg.title())
    check("notification sent", pg.evaluate("window.__n.length") == 1, pg.evaluate("window.__n"))
    pg.evaluate("() => { Object.defineProperty(document, 'hidden', { configurable: true, get: () => false }); document.dispatchEvent(new Event('visibilitychange')); }")
    check("title restored", not pg.title().startswith("\u25cf") and pg.title().endswith("Kempt"), pg.title())
    # two windows: the file goes to the one that was in front last, not to both
    pg2 = br_ctx = browser.new_page(viewport={"width": 1300, "height": 800}); pg2.set_default_timeout(20000)
    pg2.goto(g.url); pg2.wait_for_timeout(2500)
    n1, n2 = pg.locator("#tabs .tab").count(), pg2.locator("#tabs .tab").count()
    pg2.evaluate("touch()"); pg2.wait_for_timeout(300)
    r = subprocess.run([BIN, "gui", c], env=env, capture_output=True, text=True, timeout=20)
    pg.wait_for_timeout(1500); pg2.wait_for_timeout(500)
    check("file went to the window in front only", pg2.locator("#tabs .tab").count() == n2 + 1 and pg.locator("#tabs .tab").count() == n1, (n1, n2, pg.locator("#tabs .tab").count(), pg2.locator("#tabs .tab").count(), r.stdout, r.stderr))
    pg2.close()
    # page gone -> next launch must not claim hand-over
    pg.close(); pg.context.close() if False else None
    check("no page errors", not errs, errs)
    g.stop()
print(f"\n{PASS} passed, {FAIL} failed"); sys.exit(1 if FAIL else 0)
