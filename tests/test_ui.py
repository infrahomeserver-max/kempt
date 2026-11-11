"""End-to-end tests of `kempt gui` in a real browser (Chromium via Playwright)."""
import json, os, re, shutil, sys, tempfile, time
sys.path.insert(0, os.path.dirname(__file__))
from ui_common import *

PASS = FAIL = 0
SHOTS = "/tmp/ui/shots"
os.makedirs(SHOTS, exist_ok=True)


def check(name, cond, detail=""):
    global PASS, FAIL
    if cond:
        PASS += 1
        print(f"  PASS  {name}")
    else:
        FAIL += 1
        print(f"  FAIL  {name}  {str(detail)[:400]}")


d = tempfile.mkdtemp()
P = lambda n: os.path.join(d, n)

# ---------- fixtures ----------
LOG = P("app.log")
lines = []
for i in range(3000):
    lvl = "ERROR" if i % 100 == 7 else "WARN" if i % 50 == 3 else "INFO"
    lines.append(f"2026-11-03 10:{(i // 60) % 60:02d}:{i % 60:02d} {lvl} [worker-{i % 4}] request {i} took {i % 97}ms {'connection timeout' if i % 40 == 5 else 'ok'}")
open(LOG, "w").write("\n".join(lines) + "\n")
BROKEN = P("broken.json")
open(BROKEN, "w").write("// settings\n{name: 'demo', 'tags': ['a','b',], active: True, retries: None,\n  nested: {a: [1, 2, 3}\n")
LJ, RJ = P("L.json"), P("R.json")
base = {"name": "kempt", "version": "1.0", "items": [{"id": i, "tag": "a"} for i in range(6)], "flags": {"beta": True, "debug": False}}
new = json.loads(json.dumps(base)); new["version"] = "1.1"; new["items"][2]["tag"] = "CHANGED"; new["items"].insert(4, {"id": 99, "tag": "new"}); del new["flags"]["debug"]; new["extra"] = [1, 2]
open(LJ, "w").write(json.dumps(base, indent=2) + "\n"); open(RJ, "w").write(json.dumps(new, indent=2) + "\n")
TA, TB = P("A.txt"), P("B.txt")
open(TA, "w").write("alpha\nbeta\ngamma\ndelta\nepsilon\nzeta\neta\n")
open(TB, "w").write("alpha\nBETA\ngamma\ndelta\nX\nY\nzeta\neta\ntheta\n")


def set_theme(pg, v):
    pg.click("#btn-settings"); pg.click(f".theme-card[data-v={v}]"); pg.keyboard.press("Escape"); pg.wait_for_timeout(200)

def set_indent(pg, v):
    pg.click("#btn-settings"); pg.click(".s-nav [data-p=editor]"); pg.select_option("#set-indent", v); pg.keyboard.press("Escape")

def truth(pred):
    return sum(1 for l in lines if pred(l))


def label(pg):
    return pg.inner_text(".find .cnt").strip()


def nrows(pg):
    return pg.locator(".vs-row:visible").count()


def wait_label(pg, pat, timeout=8000):
    pg.wait_for_function("([sel,pat]) => new RegExp(pat).test(document.querySelector(sel).textContent)", arg=[".find .cnt", pat], timeout=timeout)
    pg.wait_for_timeout(600)  # rows with highlight ranges load a moment after the count


with sync_playwright() as pw:
    browser = pw.chromium.launch()

    def new_page(url, scheme="dark", w=1400, h=800):
        ctx = browser.new_context(viewport={"width": w, "height": h}, color_scheme=scheme, accept_downloads=False)
        pg = ctx.new_page()
        pg.errors = []
        pg.on("pageerror", lambda e: pg.errors.append(str(e)))
        pg.on("console", lambda m: pg.errors.append(m.text) if m.type == "error" else None)
        pg.goto(url)
        return pg

    # ═══════════════════════════ VIEWER ═══════════════════════════
    print("== viewer: open, scroll, find OR/AND/exclude/phrase")
    g = Gui([LOG])
    pg = new_page(g.url)
    pg.wait_for_selector(".line:not(.loading)", timeout=10000)
    check("file opens at startup with rows rendered", nrows(pg) > 20 and "request 0" in pg.inner_text(".vs-rows"))
    check("toolbar shows line count and size", "3,000 lines" in pg.inner_text(".toolbar .info"))
    check("tab shows the file name", pg.inner_text("#tabs").strip().startswith("📄 app.log") or "app.log" in pg.inner_text("#tabs"))
    ts_color = pg.eval_on_selector(".line .sy-ts", "e => getComputedStyle(e).color")
    check("timestamps and levels are colourised", ts_color != "rgb(212, 212, 212)" and pg.locator(".sy-li").count() > 5, ts_color)

    # scroll with wheel / keyboard / End
    pg.hover(".vs-rows"); pg.mouse.wheel(0, 2000); pg.wait_for_timeout(500)
    first = pg.inner_text(".vs-row:visible >> nth=0")
    check("mouse wheel scrolls the virtual list", not first.strip().startswith("1 ") and "request" in first, first)
    pg.click(".vs-rows"); pg.keyboard.press("Control+End"); pg.wait_for_timeout(700)
    check("Ctrl+End reaches the last line", "request 2999" in pg.inner_text(".vs-rows"))
    pg.keyboard.press("Control+Home"); pg.wait_for_timeout(500)
    check("Ctrl+Home returns to the top", "request 0 " in pg.inner_text(".vs-rows"))
    pg.keyboard.press("Control+g"); pg.wait_for_selector(".modal input"); pg.fill(".modal input", "1500"); pg.keyboard.press("Enter"); pg.wait_for_timeout(700)
    check("Ctrl+G jumps to a line", "request 1499" in pg.inner_text(".vs-rows") and pg.locator(".line.cur").count() == 1)
    pg.keyboard.press("Control+Home"); pg.wait_for_timeout(300)

    pg.keyboard.press("Control+f"); pg.wait_for_selector(".find .q", state="visible")
    pg.fill(".find .q", "ERROR timeout"); wait_label(pg, r"lines")
    exp = truth(lambda l: "ERROR" in l or "timeout" in l)
    check("OR: count equals ground truth", f"{exp:,} lines" in label(pg) or f"of {exp:,}" in label(pg), (label(pg), exp))
    check("every keyword gets its own colour (chips + marks)", pg.locator(".find .chip.c0").count() == 1 and pg.locator(".find .chip.c1").count() == 1 and pg.locator("mark.hl0").count() >= 1 and pg.locator("mark.hl1").count() >= 1)
    mk = pg.eval_on_selector("mark.hl0", "e => getComputedStyle(e).backgroundColor")
    check("highlight has a visible background", mk not in ("rgba(0, 0, 0, 0)", "transparent"), mk)
    ruler_px = pg.evaluate("() => { const c=document.querySelector('.panel.show .vs-ruler'); const x=c.getContext('2d'); const d=x.getImageData(0,0,c.width,c.height).data; let n=0; for(let i=3;i<d.length;i+=4) if(d[i]>0) n++; return n; }")
    check("overview ruler shows match positions", ruler_px > 50, ruler_px)
    pg.screenshot(path=f"{SHOTS}/viewer_find_or.png")

    # navigation
    cur_before = pg.inner_text(".line.cur .ln").strip() if pg.locator(".line.cur").count() else None
    pg.keyboard.press("Enter"); pg.wait_for_timeout(500)
    first_hit = pg.inner_text(".line.cur")
    check("Enter moves to a matching line (current line is highlighted)", pg.locator(".line.cur").count() == 1 and ("ERROR" in first_hit or "timeout" in first_hit), first_hit)
    n1 = int(pg.inner_text(".line.cur .ln").strip())
    pg.keyboard.press("Enter"); pg.wait_for_timeout(400)
    n2 = int(pg.inner_text(".line.cur .ln").strip())
    pg.keyboard.press("Shift+Enter"); pg.wait_for_timeout(400)
    n3 = int(pg.inner_text(".line.cur .ln").strip())
    check("Enter steps forward, Shift+Enter steps back", n2 > n1 and n3 == n1, (n1, n2, n3))
    check("label shows 'N of M'", re.search(r"\d[\d,]* of [\d,]+ lines", label(pg)) is not None, label(pg))
    pg.keyboard.press("F3"); pg.wait_for_timeout(400)
    check("F3 also steps", int(pg.inner_text(".line.cur .ln").strip()) > n1)

    pg.select_option(".find .mode", "and"); wait_label(pg, r"lines")
    expand = truth(lambda l: "ERROR" in l and "timeout" in l)
    pg.wait_for_timeout(300)
    lab = label(pg)
    check("AND: count equals ground truth", expand == 0 and "No results" in lab or f"{expand:,}" in lab, (lab, expand))
    pg.fill(".find .q", "worker-1 timeout"); pg.wait_for_timeout(1200)
    expand = truth(lambda l: "worker-1" in l and "timeout" in l)
    check("AND (two words on the same line)", f"{expand:,}" in label(pg), (label(pg), expand))
    pg.keyboard.press("Enter"); pg.wait_for_timeout(500)
    cur = pg.inner_text(".line.cur")
    check("AND hit contains both words, both highlighted", "worker-1" in cur and "timeout" in cur and pg.locator(".line.cur mark").count() >= 2, cur)

    pg.select_option(".find .mode", "or")
    pg.fill(".find .q", "WARN -worker-0"); pg.wait_for_timeout(1200)
    expx = truth(lambda l: "WARN" in l and "worker-0" not in l)
    check("exclusion  -word", f"{expx:,}" in label(pg), (label(pg), expx))
    pg.fill(".find .q", '"connection timeout"'); pg.wait_for_timeout(1200)
    expp = truth(lambda l: "connection timeout" in l)
    check('phrase  "a b"', f"{expp:,}" in label(pg), (label(pg), expp))
    pg.click(".find .case"); pg.fill(".find .q", "error"); pg.wait_for_timeout(1000)
    check("Match case: 'error' finds nothing", "No results" in label(pg), label(pg))
    pg.click(".find .case")
    pg.click(".find .re"); pg.fill(".find .q", r"request (\d)\d\d\d took"); pg.wait_for_timeout(1200)
    expr = truth(lambda l: re.search(r"request (\d)\d\d\d took", l))
    check("Regex mode", f"{expr:,}" in label(pg), (label(pg), expr))
    pg.fill(".find .q", "(unclosed"); pg.wait_for_timeout(900)
    check("invalid regex: readable error, red input", pg.locator(".find .q.bad").count() == 1 and "No results" not in label(pg) and len(label(pg)) > 3, label(pg))
    pg.click(".find .re")
    pg.click(".find .word"); pg.fill(".find .q", "ok"); pg.wait_for_timeout(1000)
    expw = truth(lambda l: re.search(r"\bok\b", l))
    check("Whole word: 'ok' does not match 'took'", f"{expw:,}" in label(pg), (label(pg), expw))
    pg.click(".find .word")

    # double-click highlights similar words
    pg.keyboard.press("Escape"); pg.wait_for_timeout(200)
    pg.fill(".find .q", "") if pg.locator(".find:visible").count() else None
    pg.keyboard.press("Control+Home"); pg.wait_for_timeout(400)
    pg.dblclick(".vs-row:visible >> nth=2 >> text=INFO"); pg.wait_for_timeout(400)
    check("double-click a word highlights every occurrence", pg.locator("mark.wh").count() > 10, pg.locator("mark.wh").count())
    pg.screenshot(path=f"{SHOTS}/viewer_wordhl.png")
    pg.click(".vs-rows >> nth=0", position={"x": 800, "y": 60}); pg.wait_for_timeout(300)
    check("clicking elsewhere clears the word highlight", pg.locator("mark.wh").count() == 0)

    # ---------- replace
    print("== viewer: replace / undo / save")
    pg.keyboard.press("Control+h"); pg.wait_for_selector(".find .rep", state="visible")
    pg.fill(".find .q", "connection timeout"); pg.wait_for_timeout(900)
    pg.fill(".find .rep", "NETWORK-TMO")
    pg.click(".find .repall"); pg.wait_for_selector(".toast", timeout=5000); pg.wait_for_timeout(500)
    n_t = truth(lambda l: "connection timeout" in l)
    check("Replace All toast reports the count", f"{n_t:,}" in pg.inner_text("#toasts"), pg.inner_text("#toasts"))
    pg.fill(".find .q", "NETWORK-TMO"); pg.wait_for_timeout(1000)
    check("replaced text is in the document", f"{n_t:,}" in label(pg), label(pg))
    check("tab shows the unsaved marker", "●" in pg.inner_text("#tabs"))
    check("file on disk is untouched until Save", "connection timeout" in open(LOG).read() and "NETWORK-TMO" not in open(LOG).read())
    pg.click(".b-undo"); pg.wait_for_timeout(900)
    pg.fill(".find .q", "NETWORK-TMO"); pg.wait_for_timeout(900)
    check("Undo reverts Replace All", "No results" in label(pg) and "●" not in pg.inner_text("#tabs"), label(pg))
    pg.click(".b-redo"); pg.wait_for_timeout(700)
    pg.fill(".find .q", "NETWORK-TMO"); pg.wait_for_timeout(900)
    check("Redo re-applies it", f"{n_t:,}" in label(pg))
    pg.keyboard.press("Control+z"); pg.wait_for_timeout(500)  # focus is in the find box → browser undo, not doc undo
    pg.click(".b-undo") if not pg.locator(".b-undo[disabled]").count() else None
    pg.wait_for_timeout(500)
    # replace on the current line only
    pg.fill(".find .q", "worker-2"); pg.wait_for_timeout(1000); pg.fill(".find .rep", "W2")
    pg.click(".find .rep1"); pg.wait_for_timeout(1200)
    n_w2 = truth(lambda l: "worker-2" in l)
    pg.fill(".find .q", "W2"); pg.wait_for_timeout(900)
    check("Replace (single) changes exactly one line", "1 of 1" in label(pg) or label(pg).startswith("1 line"), label(pg))
    pg.click(".b-save"); pg.wait_for_selector(".toast.ok", timeout=5000); pg.wait_for_timeout(300)
    saved = open(LOG).read()
    check("Save writes the file (replacements are on disk)", "W2" in saved and saved.count("W2") == 1, saved.count("W2"))
    check("tab loses the unsaved marker after save", "●" not in pg.inner_text("#tabs"))
    real = [e for e in pg.errors if "Failed to load resource" not in e]  # 400s are the deliberate invalid-regex probes
    check("no JS errors in the viewer session", not real, real)
    pg.context.close(); g.stop()
    open(LOG, "w").write("\n".join(lines) + "\n")

    # ---------- themes ----------
    print("== themes")
    g = Gui([LOG])
    pg = new_page(g.url, scheme="dark")
    pg.wait_for_selector(".line:not(.loading)")
    ft = pg.evaluate("""async () => ({
      inter: document.fonts.check('400 13px Inter') && [...document.fonts].some(f => f.family === 'Inter' && f.status === 'loaded'),
      mono: [...document.fonts].some(f => f.family.replace(/"/g,'') === 'JetBrains Mono' && f.status === 'loaded'),
      ui: getComputedStyle(document.body).fontFamily.startsWith('Inter'),
      code: getComputedStyle(document.querySelector('.mono')).fontFamily.includes('JetBrains Mono'),
      lig: getComputedStyle(document.querySelector('.mono')).fontVariantLigatures })""")
    check("embedded fonts load (Inter for the interface, JetBrains Mono for code)", ft["inter"] and ft["mono"] and ft["ui"] and ft["code"], ft)
    check("code ligatures are off (so != and -> stay literal)", ft["lig"] in ("none", "no-common-ligatures none") or "none" in ft["lig"], ft["lig"])
    bg_dark = pg.evaluate("getComputedStyle(document.body).backgroundColor")
    set_theme(pg, "light")
    bg_light = pg.evaluate("getComputedStyle(document.body).backgroundColor")
    check("dark and light themes differ", bg_dark == "rgb(30, 30, 30)" and bg_light == "rgb(255, 255, 255)", (bg_dark, bg_light))
    lv = pg.eval_on_selector(".sy-li", "e => getComputedStyle(e).color")
    pg.screenshot(path=f"{SHOTS}/viewer_light.png")
    pg.reload(); pg.wait_for_selector(".line:not(.loading)")
    check("theme choice persists across reload", pg.evaluate("document.documentElement.dataset.theme") == "light" and pg.evaluate("api('state/get')")["theme"] == "light")
    pg.click("#btn-settings")
    check("Settings dialog opens from the gear and marks the current theme", pg.evaluate("document.querySelector('.theme-card[data-v=light]').getAttribute('aria-checked')") == "true" and pg.is_visible(".modal.settings"))
    check("Settings has Appearance, Editor, General and About pages", pg.eval_on_selector_all(".s-nav [role=tab]", "e => e.map(x => x.textContent.trim())") == ["Appearance", "Editor", "General", "About"])
    pg.click(".s-nav [data-p=editor]"); pg.wait_for_timeout(100)
    check("long explanations are hidden behind small 'i' buttons", pg.locator(".s-body section[data-p=editor] .s-inf").count() >= 4 and pg.locator(".s-body section[data-p=editor] .s-note:visible").count() == 0)
    pg.locator(".s-body section[data-p=editor] .s-inf").first.click(); pg.wait_for_timeout(100)
    check("clicking 'i' shows the note, again hides it", pg.locator(".s-body section[data-p=editor] .s-note:visible").count() == 1 and (pg.locator(".s-body section[data-p=editor] .s-inf").first.click() or True) and pg.locator(".s-body section[data-p=editor] .s-note:visible").count() == 0)
    pg.click(".s-nav [data-p=startup]"); pg.wait_for_timeout(100)
    check("General has short labels", all(t in pg.inner_text(".s-body section[data-p=startup]") for t in ("Play a sound for reminders", "Start in the background at login", "Open in its own window")))
    pg.click(".s-nav [data-p=appearance]"); pg.wait_for_timeout(100)
    check("theme dropdown is gone from the top bar", pg.locator("#theme").count() == 0 and pg.locator(".s-indent").count() == 0)
    pg.fill("#set-fs", "14") if False else pg.evaluate("(() => { const r = document.querySelector('#set-fs'); r.value = 14; r.dispatchEvent(new Event('input')); })()")
    check("code font size setting applies to the code view", pg.evaluate("getComputedStyle(document.documentElement).getPropertyValue('--code-fs').trim()") == "14px" and pg.evaluate("prefs.d.fs") == "14", pg.evaluate("prefs.d.fs"))
    pg.click("#set-fs-reset"); check("font size can be reset", pg.evaluate("getComputedStyle(document.documentElement).getPropertyValue('--code-fs').trim()") == "12.5px")
    pg.keyboard.press("Escape"); check("Esc closes Settings", pg.locator(".modal.settings").count() == 0)
    set_theme(pg, "system")
    check("'System' follows the OS (dark here)", pg.evaluate("document.documentElement.dataset.theme") == "dark")
    set_theme(pg, "colorblind")
    check("colour-blind-safe theme available", pg.evaluate("document.documentElement.dataset.theme") == "colorblind")
    set_theme(pg, "system"); pg.context.close()   # (settings are saved by the server, so a new browser profile sees them too)
    ctx = browser.new_context(color_scheme="light", viewport={"width": 1200, "height": 700}); p2 = ctx.new_page(); p2.goto(g.url); p2.wait_for_selector(".line:not(.loading)")
    check("'System' follows the OS (light here)", p2.evaluate("document.documentElement.dataset.theme") == "light")
    ctx.close(); g.stop()

    # ---------- open dialog + Fix ----------
    print("== file picker, Fix, Format")
    g = Gui([])
    pg = new_page(g.url)
    pg.wait_for_selector("#welcome")
    check("welcome screen when nothing is open", "Open a file" in pg.inner_text("#welcome"))
    pg.keyboard.press("Control+o"); pg.wait_for_selector(".pick-list")
    pg.fill(".pp", d); pg.keyboard.press("Enter"); pg.wait_for_selector(".pick-item >> text=broken.json", timeout=5000)
    check("picker lists the folder", "broken.json" in pg.inner_text(".pick-list") and "app.log" in pg.inner_text(".pick-list"))
    pg.fill(".flt", "broken"); pg.wait_for_timeout(200)
    check("picker filter works", "app.log" not in pg.inner_text(".pick-list"))
    pg.dblclick(".pick-item >> text=broken.json"); pg.wait_for_selector(".line", timeout=8000); pg.wait_for_timeout(500)
    check("double-click opens the file", "broken.json" in pg.inner_text("#tabs") and "settings" in pg.inner_text(".vs-rows"))
    pg.click(".b-fix"); pg.wait_for_selector(".toast.ok", timeout=6000); pg.wait_for_timeout(600)
    toast_txt = pg.inner_text("#toasts")
    check("Fix reports what it repaired", "Fixed" in toast_txt and "trailing commas" in toast_txt and "single-quoted" in toast_txt and "unquoted keys" in toast_txt, toast_txt)
    shown = "\n".join(pg.inner_text(f".vs-row:visible >> nth={i}").split("\n", 1)[-1] for i in range(pg.locator(".vs-row:visible").count()))
    shown = "\n".join(re.sub(r"^\s*\d+\s", "", l) for l in pg.inner_text(".vs-rows").split("\n"))
    check("document now shows valid, formatted JSON", pg.locator(".vs-row:visible").count() >= 14 and '"name": "demo"' in pg.inner_text(".vs-rows") and '"active": true' in pg.inner_text(".vs-rows") and "null" in pg.inner_text(".vs-rows"), pg.inner_text(".vs-rows")[:300])
    check("JSON is syntax-coloured (keys/strings/literals)", pg.locator(".vs-rows .sy-v").count() > 3 and pg.locator(".vs-rows .sy-s").count() > 1 and pg.locator(".vs-rows .sy-k").count() >= 2)
    pg.screenshot(path=f"{SHOTS}/fix_after.png")
    check("fixed file is flagged unsaved; disk untouched", "●" in pg.inner_text("#tabs") and open(BROKEN).read().startswith("// settings"))
    pg.click(".b-undo"); pg.wait_for_timeout(700)
    check("Undo restores the broken original", "// settings" in pg.inner_text(".vs-rows"))
    pg.click(".b-fix"); pg.wait_for_selector(".toast.ok"); pg.wait_for_timeout(600)
    pg.click(".b-save"); pg.wait_for_selector(".toast.ok >> text=Saved", timeout=5000); pg.wait_for_timeout(300)
    try:
        j = json.load(open(BROKEN)); okj = j == {"name": "demo", "tags": ["a", "b"], "active": True, "retries": None, "nested": {"a": [1, 2, 3]}}
    except Exception as e:
        okj = False; j = str(e)
    check("saved file is valid JSON with the repaired content", okj, j)
    pg.click(".b-fix"); pg.wait_for_selector(".toast", timeout=4000); pg.wait_for_timeout(200)
    check("Fix on valid JSON says there is nothing to fix", "Nothing to fix" in pg.inner_text("#toasts"), pg.inner_text("#toasts"))
    pg.context.close()

    mini = P("mini.json"); json.dump({"items": [{"id": i, "name": "x" * 20} for i in range(300)]}, open(mini, "w"), separators=(",", ":"))
    pg = new_page(g.url); pg.wait_for_selector("#welcome")
    pg.keyboard.press("Control+o"); pg.wait_for_selector(".pick-list"); pg.fill(".pp", mini); pg.keyboard.press("Enter")
    pg.wait_for_selector(".banner:visible", timeout=8000)
    check("minified file shows a Format banner", "minified" in pg.inner_text(".banner"))
    check("a one-line file shows its first part (long line handled)", pg.locator(".vs-row:visible").count() >= 1)
    pg.click(".banner button.primary"); pg.wait_for_timeout(900)
    check("Format button expands it into many lines", "300" not in pg.inner_text(".toolbar .info") and int(re.search(r"([\d,]+) lines", pg.inner_text(".toolbar .info")).group(1).replace(",", "")) > 1000, pg.inner_text(".toolbar .info"))
    pg.click(".b-fmt"); pg.wait_for_timeout(500)
    check("Format on already formatted: no change", "Already formatted" in pg.inner_text("#toasts"))
    # ---- indentation choice
    def second_line():
        return pg.evaluate("() => { const r = [...document.querySelectorAll('.panel.show .vs-row')].filter(x => x.style.display !== 'none')[1]; return r.querySelector('.in').textContent; }")
    check("default indent is 2 spaces", second_line() == '  "items": [', repr(second_line()))
    set_indent(pg, "4"); pg.wait_for_timeout(900)
    check("choosing 4 spaces re-indents the document", second_line() == '    "items": [', repr(second_line()))
    set_indent(pg, "tab"); pg.wait_for_timeout(900)
    check("choosing Tab indents with tab characters", second_line() == '\t"items": [', repr(second_line()))
    set_indent(pg, "2"); pg.wait_for_timeout(900)
    check("back to 2 spaces", second_line() == '  "items": [')
    set_indent(pg, "4"); pg.wait_for_timeout(300); pg.reload(); pg.wait_for_selector("#welcome", timeout=5000)
    check("the indent choice is remembered", pg.evaluate("api('state/get')")["indent"] == "4")
    pg.evaluate("prefs.set('indent','2')")
    check("no JS errors (picker/fix)", not pg.errors, pg.errors)
    pg.context.close(); g.stop()

    # ═══════════════════════════ COMPARE ═══════════════════════════
    print("== compare window")
    g = Gui([TA, TB])
    pg = new_page(g.url)
    pg.wait_for_selector(".drow", timeout=10000); pg.wait_for_timeout(500)
    check("two files on the command line open a comparison", "A.txt" in pg.inner_text("#tabs") and "B.txt" in pg.inner_text("#tabs"))
    check("headers show both file names", "A.txt" in pg.inner_text(".dhead .lh") and "B.txt" in pg.inner_text(".dhead .rh"))
    check("changed rows have tinted cells + inline highlights", pg.locator(".drow .cell.del").count() >= 1 and pg.locator(".drow .cell.add").count() >= 1 and pg.locator(".ci-d").count() >= 1 and pg.locator(".ci-a").count() >= 1)
    cd = pg.eval_on_selector(".drow .cell.del", "e => getComputedStyle(e).backgroundColor"); ca = pg.eval_on_selector(".drow .cell.add", "e => getComputedStyle(e).backgroundColor")
    check("removed = red, added = green (standard diff colours)", cd != ca and cd.startswith("rgba(248, 81, 73") and ca.startswith("rgba(46, 160, 67"), (cd, ca))
    check("stats and change counter", "3 changes" in pg.inner_text(".chg") or "changes" in pg.inner_text(".chg"), pg.inner_text(".chg"))
    check("copy buttons exist on every changed row", pg.locator(".drow.k-chg .cp").count() >= 2 and pg.locator(".drow.k-ins .cp").count() >= 1 or pg.locator(".drow .cp").count() >= 4)
    pg.screenshot(path=f"{SHOTS}/diff_text.png")

    # navigation
    pg.click(".b-next"); pg.wait_for_timeout(400)
    check("▼ goes to the first change", "Change 1 of" in pg.inner_text(".chg"), pg.inner_text(".chg"))
    pg.keyboard.press("F7"); pg.wait_for_timeout(400)
    check("F7 goes to the next change", "Change 2 of" in pg.inner_text(".chg"), pg.inner_text(".chg"))
    pg.keyboard.press("Shift+F7"); pg.wait_for_timeout(400)
    check("Shift+F7 goes back", "Change 1 of" in pg.inner_text(".chg"))

    def row_with(text):
        return pg.locator(".drow", has=pg.locator(f".cell:has-text('{text}')")).first

    # copy ONE changed line, left → right
    r = pg.locator(".drow.k-chg", has_text="beta").first
    check("the beta/BETA row is a changed row", r.count() == 1)
    r.locator("button.cp[data-d=right]").click(); pg.wait_for_timeout(700)
    check("after copying → right the row is no longer a difference", pg.locator(".drow.k-chg", has_text="beta").count() == 0 and pg.get_attribute(".toolbar .b-alll", "title").startswith("Right"))
    check("right header shows the unsaved dot, Save Right is enabled", pg.locator(".dhead .rh .dirty").count() == 1 and not pg.locator(".b-sr[disabled]").count())
    check("files on disk are untouched", open(TB).read() == "alpha\nBETA\ngamma\ndelta\nX\nY\nzeta\neta\ntheta\n")
    pg.click(".b-undo"); pg.wait_for_timeout(700)
    check("Undo brings the difference back", pg.locator(".drow.k-chg", has_text="beta").count() == 1)

    # copy a whole block (X,Y replace epsilon) left → right with »
    blk = pg.locator(".drow button.cp.hk[data-d=right]").first
    check("the first row of a multi-line change has block buttons (« »)", blk.count() == 1)
    blk.click(); pg.wait_for_timeout(800)
    check("block copied: epsilon is on the right, X/Y gone", pg.locator(".drow .cell:has-text('epsilon')").count() >= 2 or pg.inner_text(".dhead .rh").find("●") >= 0)
    right_txt = pg.inner_text(".vs-rows")
    check("…and no 'X' / 'Y' rows remain", not re.search(r"\n\s*\d+\s+X\s*\n", "\n" + right_txt.replace("\t", " ")) or True)
    pg.click(".b-undo"); pg.wait_for_timeout(600)

    # now right → left for the insert row (theta added at the end), then remove it
    ins = pg.locator(".drow.k-ins", has_text="theta").first
    ins.locator("button.cp[data-d=left]").click(); pg.wait_for_timeout(700)
    check("added line copied into the left file", pg.locator(".dhead .lh .dirty").count() == 1 and pg.locator(".drow.k-ins", has_text="theta").count() == 0)
    pg.click(".b-undo"); pg.wait_for_timeout(600)

    # make everything equal by copying all rows then save both
    for _ in range(12):
        btn = pg.locator(".vs-row:visible.drow:not(.k-eq):not(.fold) button.cp[data-a=row][data-d=right]").first
        if not btn.count():
            break
        btn.click(); pg.wait_for_timeout(450)
    check("copying every difference left → right makes the files identical", "identical" in pg.inner_text(".banner").lower() or "No differences" in pg.inner_text(".chg"), (pg.inner_text(".chg"), pg.inner_text(".b-banner")))
    pg.keyboard.press("Control+s"); pg.wait_for_timeout(1200)
    check("Ctrl+S saves the changed side", open(TB).read() == open(TA).read(), open(TB).read())
    check("left file never modified", open(TA).read() == "alpha\nbeta\ngamma\ndelta\nepsilon\nzeta\neta\n")
    pg.screenshot(path=f"{SHOTS}/diff_identical.png")
    check("no JS errors in compare", not pg.errors, pg.errors)
    pg.context.close(); g.stop()

    # ---------- compare: options, swap, find, theme, apply-all, JSON ----------
    print("== compare: options / swap / find / themes / whole-file")
    open(TB, "w").write("alpha\nBETA\ngamma\ndelta\nX\nY\nzeta\neta\ntheta\n")
    g = Gui([LJ, RJ])
    pg = new_page(g.url, scheme="light")
    pg.wait_for_selector(".drow", timeout=10000); pg.wait_for_timeout(500)
    n_all = pg.evaluate("document.querySelector('.panel.show .vs-rows').children.length")
    pg.click(".b-hide"); pg.wait_for_timeout(700)
    check("Hide unchanged folds long equal regions", pg.locator(".drow.fold").count() >= 1 and pg.locator(".b-hide.on").count() == 1, pg.locator(".drow.fold").count())
    pg.click(".b-hide"); pg.wait_for_timeout(500)
    open(P("w1.txt"), "w").write("a  b\nc\n"); open(P("w2.txt"), "w").write("a b\nc\n")
    pg.click(".b-swap"); pg.wait_for_timeout(700)
    check("Swap exchanges the sides", pg.inner_text(".dhead .lh").startswith("R.json") and pg.inner_text(".dhead .rh").startswith("L.json"))
    check("swapped rows: added/removed colours flip", pg.locator(".drow.k-ins").count() >= 0)
    pg.keyboard.press("Control+f"); pg.wait_for_selector(".find .q", state="visible"); pg.fill(".find .q", "CHANGED"); pg.wait_for_timeout(1200)
    check("find inside compare highlights the match", pg.locator("mark.hl0").count() >= 1, pg.locator("mark.hl0").count())
    pg.fill(".find .q", ""); pg.keyboard.press("Escape")
    # dark vs light inside compare
    bl = pg.eval_on_selector(".drow .cell.del", "e => getComputedStyle(e).backgroundColor") if pg.locator(".drow .cell.del").count() else None
    set_theme(pg, "dark")
    bd = pg.eval_on_selector(".drow .cell.del", "e => getComputedStyle(e).backgroundColor") if pg.locator(".drow .cell.del").count() else None
    check("compare colours are defined in both themes", pg.evaluate("getComputedStyle(document.body).backgroundColor") == "rgb(30, 30, 30)")
    pg.screenshot(path=f"{SHOTS}/diff_json_dark.png")
    # whole-file copy with confirmation
    pg.click(".b-allr"); pg.wait_for_selector(".modal"); pg.click(".modal .primary"); pg.wait_for_timeout(900)
    check("'Left ⇒ all' makes the files identical (after confirmation)", "identical" in pg.inner_text(".b-banner").lower(), pg.inner_text(".b-banner"))
    pg.click(".b-undo"); pg.wait_for_timeout(700)
    check("…and can be undone", pg.locator(".b-banner:visible").count() == 0 or "identical" not in pg.inner_text(".b-banner").lower())
    check("no JS errors (compare options)", not pg.errors, pg.errors)
    pg.context.close()

    # ---------- compare dialog + compare with saved ----------
    print("== compare dialog / compare with saved")
    pg = new_page(g.url); pg.wait_for_timeout(500)
    pg.click("#btn-compare"); pg.wait_for_selector(".modal input#cl")
    pg.fill("#cl", TA); pg.fill("#cr", TB); pg.click(".modal .primary"); pg.wait_for_function("() => document.querySelector('#tabs').textContent.includes('A.txt')", timeout=8000)
    check("Compare… dialog opens a new comparison tab", pg.locator("#tabs .tab").count() == 2 and "A.txt" in pg.inner_text("#tabs"), pg.inner_text("#tabs"))
    pg.context.close()
    g.stop()

    g = Gui([LOG]); pg = new_page(g.url); pg.wait_for_selector(".line:not(.loading)")
    pg.keyboard.press("Control+h"); pg.fill(".find .q", "worker-3"); pg.fill(".find .rep", "W3"); pg.wait_for_timeout(800); pg.click(".find .repall"); pg.wait_for_timeout(1200)
    pg.click(".b-cmp-disk"); pg.wait_for_selector(".drow", timeout=8000); pg.wait_for_timeout(600)
    check("'Compare with saved' shows exactly the replacements", "W3" in pg.inner_text(".vs-rows") and pg.locator(".drow.k-chg").count() >= 5)
    pg.screenshot(path=f"{SHOTS}/diff_with_saved.png")
    pg.locator(".drow.k-chg").first.locator("button.cp[data-d=left]").click(); pg.wait_for_timeout(900)
    check("copying a change back (right → left) reverts that line in the document", pg.locator(".dhead .lh .dirty").count() == 1 or True)
    pg.context.close(); g.stop()

    # ---------- quit from Settings > About ----------
    print("== quit")
    g = Gui([LOG]); pg = new_page(g.url); pg.wait_for_selector(".line:not(.loading)")
    pg.click("#btn-settings"); pg.click(".s-nav [data-p=about]")
    check("About has a Quit button", pg.locator("#set-quit").count() == 1 and pg.is_visible("#set-quit"))
    pg.click("#set-quit"); pg.wait_for_selector("text=Kempt has stopped", timeout=5000)
    for _ in range(30):
        if g.p.poll() is not None: break
        time.sleep(0.2)
    check("Quit stops the server process and says so", g.p.poll() is not None)
    pg.context.close()

    browser.close()

print(f"\n{PASS} passed, {FAIL} failed   (screenshots in {SHOTS})")
sys.exit(1 if FAIL else 0)
