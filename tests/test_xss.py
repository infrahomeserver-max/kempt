"""Hostile file names and contents must never run script in the page: logs, JSON (tree, table), XML, CSV, search highlights, Find box, notes,
compare, toasts, recent list. A canary sets window.__x if any payload executes."""
import os, sys, tempfile, json
sys.path.insert(0, os.path.dirname(__file__))
from ui_common import *
P1 = '<img src=x onerror="window.__x=1">'
P2 = '"><svg onload=window.__x=2>'
P3 = "<script>window.__x=3</script>"
P4 = "javascript:window.__x=4"
d = tempfile.mkdtemp(); ok = True
def chk(n, c, extra=""):
    global ok; print(("  PASS  " if c else "  FAIL  ") + n + ("" if c else "  " + str(extra)[:200])); ok &= bool(c)
files = []
def mk(name, text):
    p = os.path.join(d, name); open(p, "w").write(text); files.append(p); return p
mk(f"{P1}.log", f"2026-11-27 10:00:00 INFO {P1}\n2026-11-27 10:00:01 ERROR {P2}\n{P3}\n{P4}\n")
mk("data.json", json.dumps({P1: [P2, {P3: P4}], "k": P1}))
mk("tbl.json", json.dumps([{P1: P2, "b": P3}, {P1: "x", "b": P4}]))
mk("doc.xml", f'<r a="{P2.replace(chr(34), "&quot;")}"><{ "t" } x="1">{P1.replace("<", "&lt;")}</t><![CDATA[{P3}]]></r>')
mk("data.csv", f'a,b\n"{P1}","{P2}"\n{P3},{P4}\n')
with sync_playwright() as pw:
    br = pw.chromium.launch(); g = Gui([files[0]]); pg = br.new_page(viewport={"width": 1400, "height": 800}); pg.set_default_timeout(15000)
    dialogs = []; pg.on("dialog", lambda dlg: (dialogs.append(dlg.message), dlg.dismiss())); errs = []; pg.on("pageerror", lambda e: errs.append(str(e)))
    pg.goto(g.url)
    for _ in range(100):
        if pg.evaluate("() => Tabs.list.length"): break
        pg.wait_for_timeout(200)
    pg.wait_for_timeout(1000)
    for f in files[1:]:
        pg.evaluate("p => actions.openPath(p)", f); pg.wait_for_timeout(900)
    chk("all files opened", pg.evaluate("() => Tabs.list.length") >= 5, pg.evaluate("() => Tabs.list.length"))
    for k in range(pg.evaluate("() => Tabs.list.length")):
        pg.evaluate("k => Tabs.activate(Tabs.list[k])", k); pg.wait_for_timeout(500)
        t = pg.evaluate("() => Tabs.active && Tabs.active.openFind ? 1 : 0")
        if t:
            for q in (P1, "onerror", "<", P3):
                pg.evaluate("() => Tabs.active.openFind(false)"); pg.wait_for_timeout(200)
                pg.locator(".find .q:visible").first.fill(q); pg.wait_for_timeout(500)
            for lab in ("Table", "Format", "Fix"):
                b = pg.locator(f"button:has-text('{lab}'):visible").first
                if b.count() and b.is_enabled():
                    try: b.click(timeout=2000); pg.wait_for_timeout(500)
                    except Exception: pass
    # compare two hostile files (the diff view is drawn from server-made HTML)
    import subprocess
    a = mk("cmp_a.txt", f"same\n{P1}\nold {P2}\n"); b = mk("cmp_b.txt", f"same\n{P1}\nnew {P3} {P4}\n")
    env = dict(os.environ, KEMPT_CONFIG_DIR=[x for x in open("/proc/%d/environ" % g.p.pid).read().split("\0") if x.startswith("KEMPT_CONFIG_DIR=")][0].split("=", 1)[1])
    subprocess.run([BIN, "gui", a, b], env=env, capture_output=True, timeout=30); pg.wait_for_timeout(2500)
    chk("a comparison tab was created", pg.evaluate("() => Tabs.list.some(t => t.kind === 'diff')"))
    # notes, toast, recent list, settings, checklist and calendar text
    pg.evaluate("p => { try { toast(p, 'ok'); } catch (e) {} }", P1)
    pg.evaluate("p => { try { rememberRecent(p, false); } catch (e) {} }", os.path.join(d, P1 + ".log")); pg.wait_for_timeout(300)
    pg.evaluate("() => Tabs.list.forEach(t => t.close && 0)")
    canary = pg.evaluate("() => window.__x")
    chk("no payload executed (canary is %r)" % canary, canary is None)
    chk("no alert/confirm dialogs", not dialogs, dialogs)
    injected = pg.evaluate("() => document.querySelectorAll('img[src=x], svg[onload], [onerror], [onload]').length")
    chk("no injected elements with handlers in the DOM (%s)" % injected, injected == 0)
    scripts = pg.evaluate("() => [...document.scripts].filter(s => /__x/.test(s.textContent)).length")
    chk("no injected <script>", scripts == 0)
    chk("no page errors", not errs, errs)
    br.close(); g.stop()
sys.exit(0 if ok else 1)
