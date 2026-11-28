"""A hidden Kempt tab steps aside when a launch has to open a fresh tab, and its files move to the new one."""
import os, sys, tempfile
sys.path.insert(0, os.path.dirname(__file__))
from ui_common import *
d = tempfile.mkdtemp(); a = os.path.join(d, "a.log")
open(a, "w").write("one\ntwo\n")
ok = True
def chk(n, c):
    global ok; print(("  PASS  " if c else "  FAIL  ") + n); ok &= bool(c)
with sync_playwright() as pw:
    br = pw.chromium.launch(); g = Gui([a]); pa = br.new_page(); pa.goto(g.url)
    for _ in range(100):
        if pa.evaluate("() => Tabs.list.length"): break
        pa.wait_for_timeout(200)
    pa.evaluate("() => { Object.defineProperty(document, 'hidden', {get: () => true}); document.dispatchEvent(new Event('visibilitychange')); }")
    pa.wait_for_timeout(500)
    r = pa.evaluate("() => api('gui/open', {files: [%s]})" % __import__("json").dumps(a)); pa.evaluate("() => api('gui/retire', {})")
    chk("nobody visible: caller must open a tab (%s)" % r, r.get("delivered") is False)
    pb = br.new_page(); pb.goto(g.url)
    pb.wait_for_timeout(5000)
    chk("old tab stepped aside", "moved to a newer tab" in pa.inner_text("body"))
    names = pb.evaluate("() => Tabs.list.map(t => t.info && t.info.name)")
    chk("new tab holds the file once: %s" % names, names == ["a.log"])
    # a tab the browser froze (cannot answer) while the new tab opened still steps aside when it wakes up
    pc = br.new_page(); pc.goto(g.url); pc.wait_for_timeout(1500)
    pc.route("**/api/inbox", lambda r: r.abort()); pc.wait_for_timeout(500)
    pa.evaluate("() => api('gui/retire', {})")
    pd = br.new_page(); pd.goto(g.url); pd.wait_for_timeout(1500)
    pc.unroute("**/api/inbox"); pc.wait_for_timeout(5000)
    chk("frozen tab steps aside when it wakes", "moved to a newer tab" in pc.inner_text("body"))
    chk("the newest tab is untouched", "moved to a newer tab" not in pd.inner_text("body"))
    br.close(); g.stop()
sys.exit(0 if ok else 1)
