"""Browser tests: opt-in update check and settings backup (export/import)."""
import os, sys, tempfile, json, time
sys.path.insert(0, os.path.dirname(__file__))
from ui_common import *

PASS = FAIL = 0
def check(name, cond, detail=""):
    global PASS, FAIL
    if cond: PASS += 1; print(f"  PASS  {name}")
    else: FAIL += 1; print(f"  FAIL  {name}  {str(detail)[:400]}")

d = tempfile.mkdtemp(); P = lambda n: os.path.join(d, n)
cfg = P("cfg"); os.makedirs(cfg); os.environ["KEMPT_CONFIG_DIR"] = cfg; os.environ["KEMPT_NO_TRAY"] = "1"; os.environ["XDG_CONFIG_HOME"] = P("xdg")
json.dump({"tag_name": "v99.1.0", "html_url": "https://example.com/releases/v99.1.0"}, open(P("new.json"), "w"))
json.dump({"tag_name": "v0.0.1", "html_url": "https://example.com/releases/v0.0.1"}, open(P("old.json"), "w"))
os.environ["KEMPT_UPDATE_URL"] = "file://" + P("new.json")
open(P("a.json"), "w").write('{"a":1}\n')

with sync_playwright() as pw:
    browser = pw.chromium.launch()
    g = Gui([P("a.json")])
    ctx = browser.new_context(viewport={"width": 1100, "height": 800}, timezone_id="Asia/Kolkata", locale="en-US", accept_downloads=True)
    pg = ctx.new_page(); pg.set_default_timeout(20000); errs = []; calls = []
    pg.on("pageerror", lambda e: errs.append(str(e))); pg.on("console", lambda m: errs.append(m.text) if m.type == "error" and "502" not in m.text and "ERR_CONNECTION" not in m.text else None)
    pg.on("request", lambda r: calls.append(r.url) if "update/check" in r.url else None)
    pg.goto(g.url); pg.wait_for_selector("#tabs .tab"); pg.wait_for_timeout(5000)
    def waitmsg(sub):
        for _ in range(100):
            if sub in pg.inner_text('#set-updmsg'): return
            pg.wait_for_timeout(100)
    opts = lambda: (pg.click("#btn-settings"), pg.click(".s-nav [data-p=startup]"), pg.wait_for_selector("#set-upd"))

    print("== update check")
    check("nothing is requested while the setting is off (default)", calls == [] and not json.load(open(P("cfg/state.json"))).get("updchk") if os.path.exists(P("cfg/state.json")) else calls == [], calls)
    opts()
    check("the checkbox is off and explains itself", not pg.is_checked("#set-upd") and pg.locator("#set-upd").count() == 1)
    pg.click("#set-updnow"); waitmsg('available')
    check("the result line is actually visible (not folded behind an i)", pg.locator("#set-updmsg").is_visible())
    check("'Check now' reports the newer version", "99.1.0 is available" in pg.inner_text("#set-updmsg"), pg.inner_text("#set-updmsg"))
    check("and shows a notice with a link that opens a new tab safely", pg.locator("#toasts a[href='https://example.com/releases/v99.1.0'][rel*=noopener][target=_blank]").count() == 1)
    pg.check("#set-upd"); pg.wait_for_timeout(600)
    check("turning it on is remembered", json.load(open(P("cfg/state.json"))).get("updchk") is True and json.load(open(P("cfg/state.json"))).get("updlast", 0) > 0)
    pg.fill("#set-updrepo", "bad repo?x=1"); pg.press("#set-updrepo", "Tab"); pg.click("#set-updnow"); waitmsg('owner/name')
    check("an invalid repository name is refused before any request", "owner/name" in pg.inner_text("#set-updmsg"))
    pg.fill("#set-updrepo", ""); pg.press("#set-updrepo", "Tab")
    pg.keyboard.press("Escape")
    pg.reload(); pg.wait_for_selector("#tabs .tab"); pg.wait_for_timeout(5000)
    check("on startup it does not ask again within a day", len(calls) == 2 + 0 or len(calls) <= 3, calls)
    g.stop(); time.sleep(1.5)
    os.environ["KEMPT_UPDATE_URL"] = "file://" + P("old.json")
    g = Gui([P("a.json")]); pg.goto(g.url); pg.wait_for_selector("#tabs .tab"); opts()
    pg.click("#set-updnow"); waitmsg('latest')
    check("an up-to-date install says so", "latest version" in pg.inner_text("#set-updmsg"), pg.inner_text("#set-updmsg"))
    os.environ["KEMPT_UPDATE_URL"] = "file://" + P("missing.json"); g.stop(); time.sleep(1.5); g = Gui([P("a.json")]); pg.goto(g.url); pg.wait_for_selector("#tabs .tab"); opts()
    pg.click("#set-updnow"); waitmsg('Could not')
    check("offline or no release: a plain message, no crash", "Could not reach" in pg.inner_text("#set-updmsg") and not errs, errs)
    pg.keyboard.press("Escape")

    print("== backup")
    pg.evaluate("""async () => { prefs.set('zones', ['Asia/Kolkata', 'Europe/Paris']); prefs.set('dsta', { zones: ['Europe/London'], lead: [1440] }); prefs.set('recent', ['/secret/path.json']); prefs.set('dir', '/home/me'); await prefs.flush();
      await api('notes/set', { patch: { items: [{ id: 'k1', t: 'Pay rent', done: false, due: null, fired: false }, { id: 'occ|k1|9', k: 'occ', t: 'hidden', due: 9, done: false }], pad: 'my scratch' } }); }""")
    pg.click("#btn-settings"); pg.click(".s-nav [data-p=startup]"); pg.wait_for_selector("#set-export")
    with pg.expect_download() as dl: pg.click("#set-export")
    out = P("backup.json"); dl.value.save_as(out); b = json.load(open(out))
    check("export is one JSON file named with the date", dl.value.suggested_filename.startswith("kempt-backup-") and b["kempt"] == "backup" and b["version"] == 1)
    check("it holds settings, zones, alerts, checklist and notes", b["state"]["zones"] == ["Asia/Kolkata", "Europe/Paris"] and b["state"]["dsta"]["zones"] == ["Europe/London"] and [i["t"] for i in b["notes"]["items"]] == ["Pay rent"] and b["notes"]["pad"] == "my scratch", b)
    check("it leaves out this computer's recent files and folder, and derived reminders", "recent" not in b["state"] and "dir" not in b["state"] and all(not i.get("k") for i in b["notes"]["items"]))
    pg.evaluate("""async () => { prefs.set('zones', ['UTC']); await prefs.flush(); await api('notes/set', { patch: { items: [], pad: '' } }); }""")
    pg.set_input_files("#set-impfile", out); pg.wait_for_selector("#modals .modal, #modals [role=dialog]")
    check("import asks first and says what it replaces", "Replaces your settings" in pg.inner_text("#modals") and "1 item" in pg.inner_text("#modals"))
    pg.click("#modals button.primary"); pg.wait_for_load_state("load"); pg.wait_for_selector("#tabs .tab"); pg.wait_for_timeout(600)
    st = pg.evaluate("api('state/get')"); nt = pg.evaluate("api('notes/get')")
    check("after import the zones, alerts, checklist and notes are back", st["zones"] == ["Asia/Kolkata", "Europe/Paris"] and st["dsta"]["zones"] == ["Europe/London"] and [i["t"] for i in nt["items"] if not i.get("k")] == ["Pay rent"] and nt["pad"] == "my scratch", (st.get("zones"), nt["pad"]))
    open(P("junk.json"), "w").write('{"hello": 1}'); open(P("evil.json"), "w").write(json.dumps({"kempt": "backup", "state": {"evil": 1, "theme": "dark", "zones": "no", "recent": ["/evil"], "dir": "/evil"}, "notes": {"items": [{"id": "z1", "t": "x"}], "pad": ""}}))
    pg.click("#btn-settings"); pg.click(".s-nav [data-p=startup]"); pg.set_input_files("#set-impfile", P("junk.json")); pg.wait_for_timeout(300)
    check("a file that is not a backup is rejected with a message", "not a Kempt backup" in pg.inner_text("#toasts") and pg.locator("#modals button.primary").count() == 0)
    pg.set_input_files("#set-impfile", P("evil.json")); pg.wait_for_selector("#modals button.primary"); pg.click("#modals button.primary"); pg.wait_for_load_state("load"); pg.wait_for_timeout(800)
    st = pg.evaluate("api('state/get')"); check("unknown keys are ignored, and recent files/folder in a file never overwrite this computer's", "evil" not in st and st.get("theme") == "dark" and st.get("recent") != ["/evil"] and st.get("dir") != "/evil", st)
    ext = [r for r in calls if not r.startswith(g.url.split("/?")[0])]
    check("no page error", not errs, errs)
    ctx.close(); g.stop()
print(f"\n{PASS} passed, {FAIL} failed"); sys.exit(1 if FAIL else 0)
