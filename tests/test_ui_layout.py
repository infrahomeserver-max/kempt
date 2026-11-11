"""Layout integrity: sizes, alignment, ordering and grouping of the toolbars, no overflow, panels fit, at several window sizes and in all themes."""
import os, sys, tempfile
sys.path.insert(0, os.path.dirname(__file__))
os.environ["UI_EDITABLE"] = "1"
from ui_common import *

PASS = FAIL = 0
def check(name, cond, detail=""):
    global PASS, FAIL
    if cond: PASS += 1; print(f"  PASS  {name}")
    else: FAIL += 1; print(f"  FAIL  {name}  {str(detail)[:500]}")

d = tempfile.mkdtemp(); P = lambda n: os.path.join(d, n)
open(P("a.py"), "w").write("import os\ndef f(a):\n    return a\n")
open(P("t.csv"), "w").write("id,name,city\n" + "".join(f"{i},Name {i},City {i%5}\n" for i in range(200)))
open(P("l.json"), "w").write('{"a":1,"b":[1,2]}\n'); open(P("l2.json"), "w").write('{"a":2,"b":[1,3]}\n')
open(P("n.md"), "w").write("# T\n\ntext `code`\n")
open(P("e.json"), "w").write('{"a":{"b":[1,2,{"c":3}]}}\n')

MEASURE = """() => {
  const vis = e => e.offsetParent !== null && getComputedStyle(e).visibility !== 'hidden';
  const bars = ['#topbar .topright', '.panel.show .toolbar'].map(s => document.querySelector(s)).filter(Boolean);
  const out = {};
  for (const b of bars) {
    const btns = [...b.querySelectorAll('button')].filter(vis).map(x => { const r = x.getBoundingClientRect(); return { c: x.className.split(' ')[0], x: r.left, y: r.top, w: r.width, h: r.height, r: r.right, b: r.bottom, ic: !!x.querySelector('svg') }; });
    out[b.className.split(' ')[0]] = { btns, w: b.clientWidth, sw: b.scrollWidth, h: b.getBoundingClientRect().height, top: b.getBoundingClientRect().top };
  }
  const r = document.documentElement;
  return { bars: out, page: { sw: r.scrollWidth, cw: r.clientWidth, sh: r.scrollHeight, ch: r.clientHeight }, status: (document.querySelector('#statusbar, .statusbar') || {}).offsetHeight || 0 };
}"""
ORDER = {
  "file": ["b-save", "b-saveas", "b-copy", "|", "b-undo", "b-redo", "|", "b-find", "|", "b-fmt", "b-fix", "|", "b-cmp-disk", "b-cmp", "|", "b-view", "b-tbl"],
  "diff": ["b-prev", "b-next", "|", "tgl", "tgl", "tgl", "tgl", "|", "b-find", "b-swap", "|", "b-allr", "b-alll", "b-undo", "|", "b-sl", "b-sr", "b-cl", "b-cr"],
}
ORDER_JS = """() => { const b = document.querySelector('.panel.show .toolbar'); return [...b.children].filter(c => !c.hidden && !c.classList.contains('sp') && !c.classList.contains('info') && !c.classList.contains('chg')).map(c => c.classList.contains('sep') ? '|' : c.className.split(' ')[0]); }"""

def subseq(want, got):
    it = iter(got); return all(w in it for w in want)

with sync_playwright() as pw:
    browser = pw.chromium.launch()
    for W, H in ((800, 600), (1000, 700), (1300, 800), (1700, 900)):
        for scheme in ("dark", "light"):
            for label, files in (("code", [P("a.py")]), ("table", [P("t.csv")]), ("compare", [P("l.json"), P("l2.json")]), ("markdown", [P("n.md")])):
                g = Gui(files); ctx = browser.new_context(viewport={"width": W, "height": H}, color_scheme=scheme); pg = ctx.new_page()
                errs = []; pg.on("pageerror", lambda e: errs.append(str(e)))
                pg.goto(g.url); pg.wait_for_timeout(900)
                m = pg.evaluate(MEASURE); tag = f"{W}x{H} {scheme} {label}"
                check(f"{tag}: page has no scrollbars (fits the window)", m["page"]["sw"] <= m["page"]["cw"] and m["page"]["sh"] <= m["page"]["ch"], m["page"])
                for name, bar in m["bars"].items():
                    bt = bar["btns"]
                    if not bt: continue
                    check(f"{tag}: {name} does not overflow", bar["sw"] <= bar["w"] + 1, (bar["sw"], bar["w"]))
                    check(f"{tag}: {name} buttons are on one line", len({round(b['y']) for b in bt}) == 1, sorted({round(b['y']) for b in bt}))
                    check(f"{tag}: {name} buttons all have the same height", len({round(b['h']) for b in bt}) == 1, sorted({round(b['h']) for b in bt}))
                    check(f"{tag}: {name} widths come from the 34 / 42 px set", all(round(b["w"]) in (34, 42) or False for b in bt), sorted({round(b['w']) for b in bt}))
                    check(f"{tag}: {name} buttons never overlap", all(a["r"] <= b["x"] + 0.5 for a, b in zip(sorted(bt, key=lambda t: t["x"]), sorted(bt, key=lambda t: t["x"])[1:])))
                    check(f"{tag}: {name} buttons centred vertically in the bar", all(abs((b["y"] + b["h"] / 2) - (bar["top"] + bar["h"] / 2)) <= 1.5 for b in bt), [(b["c"], round(b["y"] + b["h"] / 2 - bar["top"] - bar["h"] / 2, 1)) for b in bt][:4])
                    check(f"{tag}: {name} every button has an icon", all(b["ic"] for b in bt), [b["c"] for b in bt if not b["ic"]])
                if scheme == "dark" and W == 1300:
                    got = pg.evaluate(ORDER_JS)
                    want = ORDER["diff" if label == "compare" else "file"]
                    check(f"{tag}: icon groups are in the agreed order", subseq([w for w in want if w not in ("b-view",) or label == "markdown"], got), got)
                    check(f"{tag}: no empty / doubled separators", "| |" not in " ".join(got) and got[0] != "|" and got[-1] != "|", got)
                check(f"{tag}: no JS errors", not errs, errs)
                pg.context.close(); g.stop()

    print("== panels, dialogs, find box")
    for W, H in ((800, 600), (1300, 800)):
        g = Gui([P("a.py")]); ctx = browser.new_context(viewport={"width": W, "height": H}); pg = ctx.new_page(); pg.goto(g.url); pg.wait_for_timeout(900)
        inside = lambda sel: pg.eval_on_selector(sel, "e => { const r = e.getBoundingClientRect(); return r.left >= -0.5 && r.top >= -0.5 && r.right <= innerWidth + 0.5 && r.bottom <= innerHeight + 0.5; }")
        pg.keyboard.press("Control+h"); pg.wait_for_selector(".panel.show .find:not([hidden])")
        check(f"{W}: find box is inside the window", inside(".panel.show .find"))
        w1 = pg.eval_on_selector(".panel.show .find", "e => e.getBoundingClientRect().width")
        for ch in "alpha beta -gamma": pg.keyboard.type(ch)
        pg.wait_for_timeout(500)
        w2 = pg.eval_on_selector(".panel.show .find", "e => e.getBoundingClientRect().width"); h2 = pg.eval_on_selector(".panel.show .find", "e => e.getBoundingClientRect().height")
        check(f"{W}: find box keeps its size while keywords are typed", abs(w1 - w2) < 1, (w1, w2))
        check(f"{W}: keyword chips stay inside the window", pg.locator(".panel.show .find .chips .chip").count() == 3 and inside(".panel.show .find .chips"))
        pg.keyboard.press("Escape")
        for btn, sel in (("#btn-new", ".modal"), ("#btn-open", ".modal"), ("#btn-settings", ".modal")):
            pg.click(btn); pg.wait_for_selector(sel)
            box = pg.eval_on_selector(sel, "e => { const r = e.getBoundingClientRect(); return [r.left, r.top, r.right, r.bottom, innerWidth, innerHeight]; }")
            check(f"{W}: {btn} dialog fits in the window", box[0] >= 0 and box[1] >= 0 and box[2] <= box[4] and box[3] <= box[5], box)
            check(f"{W}: {btn} dialog is horizontally centred", abs((box[0] + box[2]) / 2 - box[4] / 2) <= 2, box)
            pg.keyboard.press("Escape"); pg.wait_for_timeout(200)
        pg.context.close(); g.stop()

    print("== themes keep the same layout")
    sizes = {}
    for scheme in ("dark", "light"):
        g = Gui([P("a.py")]); ctx = browser.new_context(viewport={"width": 1300, "height": 800}, color_scheme=scheme); pg = ctx.new_page(); pg.goto(g.url); pg.wait_for_timeout(800)
        sizes[scheme] = pg.evaluate("[...document.querySelectorAll('#topbar button, .panel.show .toolbar button')].filter(e => e.offsetParent).map(e => { const r = e.getBoundingClientRect(); return [Math.round(r.left), Math.round(r.top), Math.round(r.width), Math.round(r.height)]; })")
        pg.context.close(); g.stop()
    check("dark and light themes place every button identically", sizes["dark"] == sizes["light"])
print(f"\n{PASS} passed, {FAIL} failed"); sys.exit(1 if FAIL else 0)
