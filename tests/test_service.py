"""Background service: single instance, reminders announced by the program itself (no browser), start at login, stop."""
import os, sys, time, json, stat, subprocess, tempfile, urllib.request

BIN = os.environ.get("KEMPT_BIN") or os.path.join(os.path.dirname(__file__), "..", "target", "release", "kempt")
PASS = FAIL = 0
def check(name, cond, detail=""):
    global PASS, FAIL
    if cond: PASS += 1; print(f"  PASS  {name}")
    else: FAIL += 1; print(f"  FAIL  {name}  {str(detail)[:400]}")

d = tempfile.mkdtemp(); cfg = os.path.join(d, "cfg"); xdg = os.path.join(d, "xdg"); os.makedirs(cfg)
nlog, slog = os.path.join(d, "notify.log"), os.path.join(d, "sound.log")
def stub(name, log):
    p = os.path.join(d, name); open(p, "w").write(f'#!/bin/sh\necho "$@" >> {log}\n'); os.chmod(p, 0o755); return p
env = dict(os.environ, KEMPT_CONFIG_DIR=cfg, XDG_CONFIG_HOME=xdg, KEMPT_NO_TRAY="1", KEMPT_NOTIFY_CMD=stub("n.sh", nlog), KEMPT_SOUND_CMD=stub("s.sh", slog))
now = lambda: int(time.time() * 1000)
NF = os.path.join(cfg, "notes.json")
def notes(): return json.load(open(NF))
def lines(p): return open(p).read().strip().splitlines() if os.path.exists(p) else []
def wait_for(cond, t=8):
    end = time.time() + t
    while time.time() < end:
        if cond(): return True
        time.sleep(0.1)
    return False
def inst(): return json.load(open(os.path.join(cfg, "instance.json")))
def api(name, body):
    i = inst(); r = urllib.request.Request(f"http://127.0.0.1:{i['port']}/api/{name}", json.dumps(body).encode(), {"X-Token": i["token"], "Content-Type": "application/json"})
    return json.load(urllib.request.urlopen(r, timeout=5))

# one reminder already overdue at start, one due in 2 s, one far away
json.dump({"items": [{"id": "a", "t": "Missed while closed", "done": False, "due": now() - 60000, "fired": False},
                     {"id": "b", "t": "Call Ann", "done": False, "due": now() + 3000, "fired": False},
                     {"id": "c", "t": "Next week", "done": False, "due": now() + 7 * 86400000, "fired": False}], "pad": ""}, open(NF, "w"))

print("== starts quietly in the background")
p = subprocess.Popen([BIN, "gui", "--background"], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
check("instance file appears", wait_for(lambda: os.path.exists(os.path.join(cfg, "instance.json"))))
check("it answers /ping", urllib.request.urlopen(f"http://127.0.0.1:{inst()['port']}/ping", timeout=3).read() == b"kempt")
check("instance file is private (0600)", stat.S_IMODE(os.stat(os.path.join(cfg, "instance.json")).st_mode) == 0o600)
time.sleep(0.5)
check("nothing printed to the terminal in background mode", p.poll() is None)

print("== reminders without any browser")
check("the overdue one is announced at start", wait_for(lambda: len(lines(nlog)) >= 1), lines(nlog))
check("text of the notification", "Kempt reminder Missed while closed" in lines(nlog)[0], lines(nlog))
check("a sound is played (alert.wav)", wait_for(lambda: len(lines(slog)) >= 1) and lines(slog)[0].endswith("alert.wav"), lines(slog))
check("alert.wav is a real RIFF file", open(os.path.join(cfg, "alert.wav"), "rb").read(4) == b"RIFF")
check("the item is flagged as announced", wait_for(lambda: notes()["items"][0].get("n") is True), notes()["items"])
check("the timed one fires on time (within ~1.5 s of its due time)", wait_for(lambda: len(lines(nlog)) >= 2, 8), lines(nlog))
n = notes(); due_b = next(i for i in n["items"] if i["id"] == "b")["due"]
check("and was announced no earlier than due", due_b <= now() + 100)
check("the far-away one is left alone", "n" not in next(i for i in n["items"] if i["id"] == "c"))
time.sleep(2.2)
check("no duplicate announcements", len(lines(nlog)) == 2, lines(nlog))
check("notes/get reports what the program can do", api("notes/get", {})["caps"] == {"os": True, "sound": True})

print("== editing from the page")
items = notes()["items"]
for i in items:
    if i["id"] == "c": i["due"] = now() + 2500
api("notes/set", {"patch": {"items": items}})
check("changing a due time wakes the thread and re-arms", wait_for(lambda: len(lines(nlog)) >= 3, 8), lines(nlog))
items = notes()["items"]; b = next(i for i in items if i["id"] == "b")
check("a page that echoes stale data cannot re-trigger an announced item", b.get("n") is True)
for i in items:
    if i["id"] == "b": i["n"] = False
api("notes/set", {"patch": {"items": items}}); time.sleep(0.6)
check("the page cannot clear the flag without changing the time", next(i for i in notes()["items"] if i["id"] == "b").get("n") is True and len(lines(nlog)) == 3)
items = notes()["items"]
for i in items:
    if i["id"] == "b": i["due"] = now() + 2000; i.pop("n", None)
api("notes/set", {"patch": {"items": items}})
check("snoozing (new due time) announces again", wait_for(lambda: len(lines(nlog)) >= 4, 8), lines(nlog))
items = notes()["items"]
for i in items:
    if i["id"] == "c": i["id"] = "c"; i["due"] = now() + 1500; i["done"] = True; i.pop("n", None)
api("notes/set", {"patch": {"items": items}}); time.sleep(2.5)
check("ticked items are never announced", len(lines(nlog)) == 4)
items = [{"id": "x1", "t": "One", "done": False, "due": now() + 1200}, {"id": "x2", "t": "Two", "done": False, "due": now() + 1200}]
api("notes/set", {"patch": {"items": items}})
check("several at once give one notification", wait_for(lambda: len(lines(nlog)) >= 5) and "2 reminders" in lines(nlog)[4], lines(nlog)[4:])
json.dump({"sound": False}, open(os.path.join(cfg, "state.json"), "w")); sc = len(lines(slog))
api("notes/set", {"patch": {"items": [{"id": "q", "t": "Quiet", "done": False, "due": now() + 1000}]}})
check("the sound setting is respected", wait_for(lambda: len(lines(nlog)) >= 6) and len(lines(slog)) == sc, (lines(nlog), lines(slog)))

print("== one instance only")
q = subprocess.run([BIN, "gui", "--background"], env=env, capture_output=True, text=True, timeout=10)
check("a second background start exits quietly", q.returncode == 0 and q.stdout == "", (q.returncode, q.stdout))
f = os.path.join(d, "x.txt"); open(f, "w").write("hi\n")
q = subprocess.run([BIN, "gui", f, "--no-open"], env=env, capture_output=True, text=True, timeout=10)
check("kempt gui with a file reuses the running one", "already running" in q.stdout, q.stdout)
check("the original is still the only server", p.poll() is None)
q = subprocess.run([BIN, "gui", "--view", "notes", "--no-open"], env=env, capture_output=True, text=True, timeout=10)
check("--view notes opens the checklist in the running copy", "&view=notes" in q.stdout, q.stdout)
alog = os.path.join(d, "chromium.log"); os.makedirs(os.path.join(d, "bin"), exist_ok=True)
open(os.path.join(d, "bin", "google-chrome"), "w").write(f'#!/bin/sh\necho "$@" >> {alog}\n'); os.chmod(os.path.join(d, "bin", "google-chrome"), 0o755)
json.dump({"sound": False, "appwin": True}, open(os.path.join(cfg, "state.json"), "w"))
subprocess.run([BIN, "gui", "--view", "clock"], env=dict(env, PATH=os.path.join(d, "bin") + ":" + os.environ["PATH"]), capture_output=True, text=True, timeout=10)
check("own-window mode launches a Chromium app window with Kempt's window class", wait_for(lambda: os.path.exists(alog)) and "--app=http://127.0.0.1:" in lines(alog)[0] and "view=clock" in lines(alog)[0] and "--class=Kempt" in lines(alog)[0], lines(alog))
flog = os.path.join(d, "firefox.log")
open(os.path.join(d, "bin", "firefox"), "w").write(f'#!/bin/sh\necho "$@" >> {flog}\n'); os.chmod(os.path.join(d, "bin", "firefox"), 0o755)
json.dump({"sound": False, "browser": "firefox"}, open(os.path.join(cfg, "state.json"), "w"))
subprocess.run([BIN, "gui", "--view", "notes"], env=dict(env, PATH=os.path.join(d, "bin") + ":" + os.environ["PATH"]), capture_output=True, text=True, timeout=10)
check("the browser chosen in Settings is the one that opens", wait_for(lambda: os.path.exists(flog)) and "http://127.0.0.1:" in lines(flog)[0] and "view=notes" in lines(flog)[0], lines(flog))
found = api("service/browsers", {})["installed"]
check("the API lists installed browsers (only known ids)", isinstance(found, list) and set(found) <= {"chrome", "edge", "brave", "chromium", "vivaldi", "firefox", "safari"}, found)
desk = open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "packaging", "kempt.desktop")).read()
check("launcher groups the window under Kempt's icon and has quick actions", "StartupWMClass=Kempt" in desk and "kempt gui --view notes" in desk and "kempt gui --view clock" in desk)

items = notes()["items"] + [{"id": "dst|Europe/London|1|1440", "k": "dst", "t": "London: clocks go forward 1 h in 1 day", "done": False, "due": now() + 1500, "fired": False}]
api("notes/set", {"patch": {"items": items}})
check("a clock-change alert is announced like any reminder", wait_for(lambda: any("London: clocks go" in l for l in lines(nlog)), 8), lines(nlog))
check("and keeps its kind marker", any(i.get("k") == "dst" and i.get("n") is True for i in notes()["items"]))
print("== start at login")
r = lambda a: subprocess.run([BIN, "autostart", a], env=env, capture_output=True, text=True, timeout=10)
check("status starts off", "off" in r("status").stdout)
o = r("on"); desk = os.path.join(xdg, "autostart", "kempt.desktop")
check("on writes an autostart entry", os.path.exists(desk) and "gui --background" in open(desk).read() and "Exec=" in open(desk).read(), o.stderr)
check("status says on", "on" in r("status").stdout)
check("the web API agrees", api("service/autostart", {})["on"] is True)
api("service/autostart", {"on": False})
check("turning it off through the API removes the entry", not os.path.exists(desk))
r("on"); r("off"); check("off removes it", not os.path.exists(desk))

print("== stop")
s = subprocess.run([BIN, "stop"], env=env, capture_output=True, text=True, timeout=10)
check("kempt stop stops it", "stopped" in s.stdout and wait_for(lambda: p.poll() is not None, 5), s.stdout)
check("the instance file is removed", not os.path.exists(os.path.join(cfg, "instance.json")))
s = subprocess.run([BIN, "stop"], env=env, capture_output=True, text=True, timeout=10)
check("stop when nothing runs is harmless", "not running" in s.stdout)

print("== detached start")
q = subprocess.run([BIN, "gui", "--background", "--detach"], env=env, capture_output=True, text=True, timeout=10)
check("--detach returns at once", q.returncode == 0)
check("and leaves a running copy", wait_for(lambda: os.path.exists(os.path.join(cfg, "instance.json"))))
subprocess.run([BIN, "stop"], env=env, capture_output=True, timeout=10)
print(f"\n{PASS} passed, {FAIL} failed"); sys.exit(1 if FAIL else 0)
