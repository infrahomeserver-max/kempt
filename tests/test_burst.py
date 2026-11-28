"""Real `kempt gui <file>` launch bursts against a simulated browser: a fake xdg-open records each tab Kempt asks for and the test opens it
(slowly, visible or hidden) in Chromium. Whatever happens, one live Kempt tab must end up holding the file once."""
import os, sys, subprocess, tempfile, time, stat, json
sys.path.insert(0, os.path.dirname(__file__))
from ui_common import *
d = tempfile.mkdtemp(); a = os.path.join(d, "a.log"); open(a, "w").write("one\ntwo\n")
cfg = tempfile.mkdtemp(); bindir = tempfile.mkdtemp(); opens = os.path.join(d, "opens")
open(opens, "w").close()
x = os.path.join(bindir, "xdg-open"); open(x, "w").write('#!/bin/sh\necho "$1" >> %s\n' % opens); os.chmod(x, 0o755)
env = dict(os.environ, KEMPT_CONFIG_DIR=cfg, PATH=bindir + ":" + os.environ["PATH"])
ok = True
def chk(n, c):
    global ok; print(("  PASS  " if c else "  FAIL  ") + n); ok &= bool(c)
seen = 0
def pump(br, pages, delay_ms, hidden_new, procs, limit=40):
    """open the tabs Kempt asked for (after a delay) until the launches have all finished"""
    global seen
    t0 = time.time(); pending = []
    while time.time() - t0 < limit:
        lines = open(opens).read().split()
        for u in lines[seen:]: pending.append((time.time() + delay_ms / 1000, u))
        seen = len(lines)
        for item in list(pending):
            if time.time() >= item[0]:
                pending.remove(item)
                pg = br.new_page()
                if hidden_new: pg.add_init_script("Object.defineProperty(document,'hidden',{get:()=>true,configurable:true});Object.defineProperty(document,'visibilityState',{get:()=>'hidden'})")
                pg.goto(item[1]); pages.append(pg)
        if all(p.poll() is not None for p in procs) and not pending: break
        time.sleep(0.1)
    time.sleep(2.5)
def live(pages):
    out = []
    for p in pages:
        try:
            if p.is_closed(): continue
            if "moved to a newer tab" in p.inner_text("body"): continue
            out.append(p)
        except Exception: pass
    return out
def names(p):
    try: return p.evaluate("() => Tabs.list.map(t => t.info && t.info.name)")
    except Exception: return None
def burst(n): return [subprocess.Popen([BIN, "gui", a], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL) for _ in range(n)]
srv = subprocess.Popen([BIN, "gui", "--no-open", "--background"], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
time.sleep(2)
with sync_playwright() as pw:
    br = pw.chromium.launch(); pages = []
    # 1. cold start, 8 double-clicks at once, the browser is slow to load the tab
    pump(br, pages, 1500, False, burst(8))
    lv = live(pages)
    chk("cold start: asked for exactly one browser tab (%d)" % len(open(opens).read().split()), len(open(opens).read().split()) == 1)
    chk("cold start: one live tab holding the file once: %s" % [names(p) for p in lv], len(lv) == 1 and names(lv[0]) == ["a.log"])
    # 2. the Kempt tab is hidden behind other tabs; a burst of double-clicks
    for p in lv: p.evaluate("() => { Object.defineProperty(document,'hidden',{get:()=>true,configurable:true}); document.dispatchEvent(new Event('visibilitychange')); }")
    time.sleep(1)
    before = len(open(opens).read().split())
    pump(br, pages, 1500, False, burst(6))
    lv = live(pages)
    chk("hidden tab: exactly one new browser tab (%d)" % (len(open(opens).read().split()) - before), len(open(opens).read().split()) - before == 1)
    chk("hidden tab: one live tab holding the file once: %s" % [names(p) for p in lv], len(lv) == 1 and names(lv[0]) == ["a.log"])
    # 3. the new tab itself loads in the background (window on another desktop): still no second tab
    for p in lv: p.evaluate("() => { Object.defineProperty(document,'hidden',{get:()=>true,configurable:true}); document.dispatchEvent(new Event('visibilitychange')); }")
    time.sleep(1)
    before = len(open(opens).read().split())
    pump(br, pages, 1500, True, burst(6))
    lv = live(pages)
    chk("new tab in background: exactly one browser tab (%d)" % (len(open(opens).read().split()) - before), len(open(opens).read().split()) - before == 1)
    chk("new tab in background: one live tab holding the file once: %s" % [names(p) for p in lv], len(lv) == 1 and names(lv[0]) == ["a.log"])
    # 4. visible tab: no new browser tab at all
    for p in lv: p.evaluate("() => { Object.defineProperty(document,'hidden',{get:()=>false,configurable:true}); document.dispatchEvent(new Event('visibilitychange')); }")
    time.sleep(1)
    before = len(open(opens).read().split())
    pump(br, pages, 0, False, burst(6))
    chk("visible tab: no new browser tab", len(open(opens).read().split()) == before)
    chk("visible tab: still one tab with the file once: %s" % [names(p) for p in live(pages)], [names(p) for p in live(pages)] == [["a.log"]])
    br.close()
subprocess.run([BIN, "stop"], env=env, capture_output=True); srv.terminate()
print(open(os.path.join(cfg, "launch.log")).read()[-1500:])
sys.exit(0 if ok else 1)
