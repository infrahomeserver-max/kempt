"""A file deleted or renamed while Kempt runs is not brought back as a tab when a window reconnects, and nothing is blank."""
import os, sys, tempfile
sys.path.insert(0, os.path.dirname(__file__))
from ui_common import *
d = tempfile.mkdtemp(); a = os.path.join(d, "old.log"); b = os.path.join(d, "new.log")
open(a, "w").write("".join(f"line {i}\n" for i in range(5000))); open(b, "w").write("hello\nworld\n")
ok = True
def chk(n, c):
    global ok; print(("  PASS  " if c else "  FAIL  ") + n); ok &= bool(c)
with sync_playwright() as pw:
    br = pw.chromium.launch(); g = Gui([a]); pg = br.new_page(); pg.goto(g.url)
    for _ in range(100):
        if pg.evaluate("() => Tabs.list.length"): break
        pg.wait_for_timeout(200)
    pg.wait_for_timeout(500)
    r = pg.evaluate("() => api('lines', { id: Tabs.list[0].info.id, from: 0, count: 2 }).then(r => r.lines.length)")
    chk("lines readable", r == 2)
    os.rename(a, os.path.join(d, "renamed.log"))
    r = pg.evaluate("() => api('lines', { id: Tabs.list[0].info.id, from: 0, count: 2 }).then(r => r.lines.length).catch(e => -1)")
    chk("still readable after a rename", r == 2)
    pg.close(); pg = br.new_page(); pg.goto(g.url); pg.wait_for_timeout(4000)
    chk("renamed file is not restored as a tab", pg.evaluate("() => Tabs.list.length") == 0)
    br.close(); g.stop()
sys.exit(0 if ok else 1)
