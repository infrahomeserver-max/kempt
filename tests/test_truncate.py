"""A file that is cut shorter (or regenerated under the same name) while it is open must not crash Kempt."""
import os, sys, tempfile
sys.path.insert(0, os.path.dirname(__file__))
from ui_common import *
d = tempfile.mkdtemp(); path = os.path.join(d, "same.log")
open(path, "w").write("".join(f"2026-11-27 10:00:00 INFO line {i}\n" for i in range(300000)))
ok = True
with sync_playwright() as pw:
    br = pw.chromium.launch(); g = Gui([path]); pg = br.new_page(); pg.goto(g.url)
    for _ in range(100):
        if pg.evaluate("() => Tabs.list.length"): break
        pg.wait_for_timeout(200)
    pg.wait_for_timeout(800)
    os.truncate(path, 500)
    r = pg.evaluate("() => api('lines', { id: Tabs.list[0].info.id, from: 250000, count: 5 }).then(() => 'ok').catch(e => 'err')")
    alive = g.p.poll() is None
    print("  PASS  server survives a truncated file" if alive else "  FAIL  server died"); ok &= alive
    pg.goto(g.url); pg.wait_for_timeout(800)
    alive = g.p.poll() is None
    print("  PASS  still answering afterwards" if alive else "  FAIL  not answering"); ok &= alive
    br.close(); g.stop()
sys.exit(0 if ok else 1)
