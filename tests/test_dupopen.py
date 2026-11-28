"""Handing over a file that is already open switches to its tab instead of adding a second one."""
import os, sys, subprocess, tempfile
sys.path.insert(0, os.path.dirname(__file__))
from ui_common import *
d = tempfile.mkdtemp(); a = os.path.join(d, "a.log"); b = os.path.join(d, "b.log")
open(a, "w").write("one\ntwo\n"); open(b, "w").write("three\n")
ok = True
def chk(n, c):
    global ok; print(("  PASS  " if c else "  FAIL  ") + n); ok &= bool(c)
with sync_playwright() as pw:
    br = pw.chromium.launch(); g = Gui([a]); pg = br.new_page(); pg.goto(g.url)
    for _ in range(100):
        if pg.evaluate("() => Tabs.list.length"): break
        pg.wait_for_timeout(200)
    pg.wait_for_timeout(800)
    env = dict(os.environ); env["KEMPT_CONFIG_DIR"] = [x for x in open("/proc/%d/environ" % g.p.pid).read().split("\0") if x.startswith("KEMPT_CONFIG_DIR=")][0].split("=", 1)[1]
    for f in (b, a, a, b):
        subprocess.run([BIN, "gui", f], env=env, capture_output=True, timeout=30); pg.wait_for_timeout(600)
    names = pg.evaluate("() => Tabs.list.map(t => t.info && t.info.name)")
    chk("each file appears once: %s" % names, sorted(names) == ["a.log", "b.log"])
    # many launches at once for one file (a cold browser makes them all land together): still one tab
    ps = [subprocess.Popen([BIN, "gui", a], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL) for _ in range(8)]
    for p in ps: p.wait(timeout=40)
    pg.wait_for_timeout(1500)
    names = pg.evaluate("() => Tabs.list.map(t => t.info && t.info.name)")
    chk("eight simultaneous launches give one tab: %s" % names, sorted(names) == ["a.log", "b.log"])
    br.close(); g.stop()
sys.exit(0 if ok else 1)
