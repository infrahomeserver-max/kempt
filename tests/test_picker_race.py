"""The open dialog keeps the path you typed even when the first folder listing arrives late (slow disk, big home folder)."""
import os, sys, tempfile
sys.path.insert(0, os.path.dirname(__file__))
from ui_common import *
d = tempfile.mkdtemp(); open(os.path.join(d, "broken.json"), "w").write("{")
ok = True
def chk(n, c):
    global ok; print(("  PASS  " if c else "  FAIL  ") + n); ok &= bool(c)
with sync_playwright() as pw:
    br = pw.chromium.launch(); g = Gui([]); pg = br.new_page(); n = [0]
    def slow(route):
        n[0] += 1
        if n[0] == 1: pg.wait_for_timeout(1500)   # the first listing (the starting folder) is slow
        route.continue_()
    pg.goto(g.url); pg.wait_for_selector("#welcome")
    pg.route("**/api/fs", slow)
    pg.keyboard.press("Control+o"); pg.wait_for_selector(".pick-list")
    pg.fill(".pp", d); pg.keyboard.press("Enter")
    try:
        pg.wait_for_selector(".pick-item >> text=broken.json", timeout=8000); found = True
    except Exception: found = False
    chk("typed folder is listed although the first listing was late", found)
    pg.wait_for_timeout(3000)   # the late first listing has arrived by now
    chk("typed folder is still listed after the late listing", "broken.json" in pg.inner_text(".pick-list"))
    chk("path box still shows the typed folder", pg.input_value(".pp").rstrip("/") == d.rstrip("/"))
    pg.unroute_all(behavior='ignoreErrors'); br.close(); g.stop()
sys.exit(0 if ok else 1)
