"""GUI performance on huge files, measured in a real browser."""
import os, sys, time, threading
sys.path.insert(0, os.path.dirname(__file__))
from ui_common import *

B = "/tmp/big"


def rss(pid):
    d = {}
    for l in open(f"/proc/{pid}/status"):
        k = l.split(":")[0]
        if k in ("RssAnon", "RssFile", "VmHWM"):
            d[k] = int(l.split()[1]) / 1024
    return d


class Peak:
    """sample the server's anonymous memory in the background"""
    def __init__(self, pid):
        self.pid, self.max, self.stop = pid, 0, False
        threading.Thread(target=self.run, daemon=True).start()
    def run(self):
        while not self.stop:
            try:
                self.max = max(self.max, rss(self.pid)["RssAnon"])
            except Exception:
                pass
            time.sleep(0.02)


def T(label, t0, extra=""):
    print(f"  {label:62s} {1000 * (time.time() - t0):8.0f} ms  {extra}", flush=True)


def wait_text(pg, sel, pat, timeout=120000):
    pg.wait_for_function("([s,p]) => new RegExp(p).test(document.querySelector(s).textContent)", arg=[sel, pat], timeout=timeout)


with sync_playwright() as pw:
    browser = pw.chromium.launch()

    print("== GUI on big.log (325 MB, 2.3M lines)")
    g = Gui([f"{B}/big.log"]); pk = Peak(g.p.pid)
    ctx = browser.new_context(viewport={"width": 1400, "height": 800}, color_scheme="dark"); pg = ctx.new_page()
    t0 = time.time(); pg.goto(g.url); pg.wait_for_selector(".line:not(.loading)", timeout=60000); T("page load -> first rows visible", t0)
    print("      ", pg.inner_text(".toolbar .info"))
    pg.click(".vs-rows")
    t0 = time.time(); pg.keyboard.press("Control+End"); pg.wait_for_function("() => document.querySelector('.vs-rows').textContent.includes('request 2299999')", timeout=30000); T("Ctrl+End -> last lines rendered", t0)
    t0 = time.time(); pg.keyboard.press("Control+Home"); pg.wait_for_function("() => document.querySelector('.vs-rows').textContent.includes('request 1 ')", timeout=30000); T("Ctrl+Home -> first lines rendered", t0)
    # drag the scrollbar thumb to the middle
    bar = pg.locator(".panel.show .vs-bar").bounding_box()
    t0 = time.time(); pg.mouse.click(bar["x"] + 8, bar["y"] + bar["height"] / 2); pg.wait_for_function("() => (() => { const n = parseInt(document.querySelector('.vs-row:not([style*=none]) .ln').textContent); return n > 1000000 && n < 1300000; })()", timeout=30000); T("click scrollbar track at 50% -> rows at ~1.15M", t0)
    # smooth wheel scrolling cost
    pg.hover(".vs-rows"); t0 = time.time()
    for _ in range(40):
        pg.mouse.wheel(0, 400)
    pg.wait_for_timeout(200); T("40 wheel events (16,000 px)", t0)

    pg.keyboard.press("Control+f"); pg.wait_for_selector(".find .q", state="visible")
    t0 = time.time(); pg.fill(".find .q", "ERROR timeout"); wait_text(pg, ".find .cnt", r"of \d|lines", 60000)
    wait_text(pg, ".find .cnt", r"^[\d,]+ lines|^\d[\d,]* of [\d,]+ lines", 60000); T("OR search 'ERROR timeout' over 2.3M lines -> first jump + count", t0, pg.inner_text(".find .cnt"))
    t0 = time.time(); pg.keyboard.press("Enter"); pg.wait_for_timeout(50); pg.wait_for_selector(".line.cur", timeout=20000); T("Enter -> next match", t0)
    t0 = time.time(); pg.select_option(".find .mode", "and"); pg.fill(".find .q", "worker-3 ERROR"); pg.wait_for_function("() => document.querySelector('.find .cnt').textContent.match(/lines|No results/)", timeout=60000); pg.wait_for_timeout(300); T("AND search 'worker-3 ERROR'", t0, pg.inner_text(".find .cnt"))
    pg.select_option(".find .mode", "or")
    t0 = time.time(); pg.click(".find .re"); pg.fill(".find .q", r"req=[0-9a-f-]+-0{6}1[0-9a-f]{5}$"); pg.wait_for_function("() => document.querySelector('.find .cnt').textContent.match(/lines|No results/)", timeout=60000); pg.wait_for_timeout(300); T("regex search with anchor", t0, pg.inner_text(".find .cnt"))
    pg.click(".find .re")

    # replace all on ~575k lines
    pg.keyboard.press("Control+h"); pg.fill(".find .q", "worker-3"); pg.fill(".find .rep", "W3"); pg.wait_for_timeout(1500)
    t0 = time.time(); pg.click(".find .repall"); pg.wait_for_selector(".toast", timeout=120000); T("Replace All 'worker-3' -> 'W3' (575k lines, new working copy)", t0, pg.inner_text("#toasts").split("\n")[0][:70])
    pg.wait_for_timeout(500)
    t0 = time.time(); pg.click(".b-undo"); pg.wait_for_function("() => !document.querySelector('.tab .dot')", timeout=30000); T("Undo (instant: previous copy)", t0)
    pg.click(".b-redo"); pg.wait_for_timeout(500)
    print(f"      server memory so far: anon peak {pk.max:.0f} MB, current {rss(g.p.pid)}")
    t0 = time.time(); pg.click(".b-save"); pg.wait_for_selector(".toast.ok >> text=Saved", timeout=120000); T("Save 325 MB atomically", t0)
    os.system(f"cp {B}/big.log {B}/big_restore.log; true")  # keep a pristine copy name for later
    pk.stop = True
    ctx.close(); g.stop()
    # restore the original log (we saved the replaced one over it)
    os.system(f"cp {B}/big_01.log {B}/big_01.log.bak; sed -i 's/\\[W3\\]/[worker-3]/g' {B}/big.log")

    print("\n== GUI: Fix / Format on big.json (250 MB, one line)")
    g = Gui([f"{B}/big.json"]); pk = Peak(g.p.pid)
    ctx = browser.new_context(viewport={"width": 1400, "height": 800}, color_scheme="dark"); pg = ctx.new_page()
    t0 = time.time(); pg.goto(g.url); pg.wait_for_selector(".line:not(.loading)", timeout=60000); T("open 250 MB one-line JSON -> first screen", t0)
    pg.wait_for_selector(".banner:visible"); print("      banner:", pg.inner_text(".banner")[:80])
    t0 = time.time(); pg.click(".b-fmt"); pg.wait_for_selector(".toast.ok", timeout=120000); pg.wait_for_function("() => parseInt(document.querySelector('.toolbar .info').textContent.match(/([\\d,]+) lines/)[1].replace(/,/g,'')) > 1000000", timeout=60000); T("Format -> millions of lines", t0, pg.inner_text(".toolbar .info"))
    t0 = time.time(); pg.click(".b-undo"); pg.wait_for_timeout(300); T("Undo", t0)
    t0 = time.time(); pg.click(".b-fix"); pg.wait_for_selector(".toast >> text=/Fix|Nothing|Looks/", timeout=120000); T("Fix (repair + format) on 250 MB", t0, pg.inner_text("#toasts").split("\n")[0][:60])
    pg.click(".vs-rows"); pg.keyboard.press("Control+End"); pg.wait_for_timeout(600)
    print(f"      server anon peak {pk.max:.0f} MB")
    pk.stop = True; ctx.close(); g.stop()

    print("\n== GUI compare on big logs (2 x 325 MB, 0.1% differences)")
    g = Gui([f"{B}/big.log", f"{B}/big_01.log"]); pk = Peak(g.p.pid)
    ctx = browser.new_context(viewport={"width": 1400, "height": 800}, color_scheme="dark"); pg = ctx.new_page()
    t0 = time.time(); pg.goto(g.url); pg.wait_for_selector(".drow", timeout=120000); T("open compare -> first rows", t0, pg.inner_text(".chg"))
    pg.wait_for_function("() => document.querySelector('.chg').textContent.match(/changes/)", timeout=30000)
    print("      ", pg.inner_text(".chg"), "|", pg.inner_text(".toolbar .info"))
    t0 = time.time(); pg.click(".b-next"); pg.wait_for_function("() => document.querySelector('.chg').textContent.includes('Change 1 of')", timeout=30000); T("▼ next change", t0)
    for i in range(3):
        t0 = time.time(); pg.keyboard.press("F7"); pg.wait_for_function(f"() => document.querySelector('.chg').textContent.includes('Change {i + 2} of')", timeout=30000); T(f"F7 -> change {i + 2}", t0)
    t0 = time.time(); pg.click(".b-hide"); pg.wait_for_selector(".drow.fold", timeout=60000); T("'Hide unchanged' (re-diff 2.3M lines)", t0, pg.inner_text(".chg"))
    t0 = time.time(); pg.click(".b-hide"); pg.wait_for_timeout(300); pg.wait_for_selector(".drow", timeout=60000); T("'Show all'", t0)
    pg.click(".b-next"); pg.wait_for_timeout(500)
    btn = pg.locator(".vs-row:visible.drow:not(.k-eq) button.cp[data-d=right]").first
    t0 = time.time(); btn.click(); pg.wait_for_function("() => document.querySelector('.dhead .rh .dirty')", timeout=120000); T("copy one line left→right (new 325 MB working copy + re-diff)", t0)
    t0 = time.time(); pg.click(".b-undo"); pg.wait_for_function("() => !document.querySelector('.dhead .rh .dirty')", timeout=60000); T("undo the copy (re-diff)", t0)
    pg.keyboard.press("Control+f"); pg.fill(".find .q", "HANDLED"); pg.wait_for_timeout(1500)
    t0 = time.time(); pg.keyboard.press("Enter"); pg.wait_for_timeout(100); T("find 'HANDLED' in compare -> next row", t0, pg.inner_text(".find .cnt"))
    print(f"      server anon peak {pk.max:.0f} MB; file-backed {rss(g.p.pid)['RssFile']:.0f} MB")
    pk.stop = True; ctx.close(); g.stop()
    browser.close()
