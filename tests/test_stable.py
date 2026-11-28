"""The address (port and token) survives a restart, so an old link from the browser history still opens."""
import os, sys, subprocess, tempfile, time, re, urllib.request
sys.path.insert(0, os.path.dirname(__file__))
PASS = FAIL = 0
def check(name, cond, detail=""):
    global PASS, FAIL
    if cond: PASS += 1; print(f"  PASS  {name}")
    else: FAIL += 1; print(f"  FAIL  {name}  {str(detail)[:300]}")
BIN = os.environ.get("KEMPT_BIN") or os.path.join(os.path.dirname(__file__), "..", "target", "release", "kempt")
def start(env):
    p = subprocess.Popen([BIN, "gui", "--no-open"], stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, env=env)
    url = None; end = time.time() + 15
    while time.time() < end:
        line = p.stdout.readline()
        m = re.search(r"http://127\.0\.0\.1:\d+/\?t=[0-9a-f]+", line)
        if m: url = m.group(0); break
    return p, url
def stop(p):
    p.terminate()
    try: p.wait(5)
    except Exception: p.kill()
def get(url):
    try: return urllib.request.urlopen(url, timeout=5).status
    except urllib.error.HTTPError as e: return e.code
    except Exception: return None
cfg = tempfile.mkdtemp(); env = dict(os.environ, KEMPT_CONFIG_DIR=cfg, KEMPT_NO_TRAY="1", XDG_CONFIG_HOME=cfg, APPDATA=cfg, HOME=cfg)
p1, u1 = start(env); check("first run prints an address", bool(u1), u1)
check("it answers", get(u1) == 200, get(u1)); stop(p1); time.sleep(0.5)
check("after it stops the old link is refused", get(u1) is None)
p2, u2 = start(env); check("second run uses the same address", u1 == u2, f"{u1} vs {u2}")
check("the old link opens again", get(u2) == 200)
stop(p2); time.sleep(0.5)
# switched off: a new address every time
import json
sd = [os.path.join(r, f) for r, _, fs in os.walk(cfg) for f in fs if f == "state.json"]
d = os.path.dirname(sd[0]) if sd else cfg
json.dump({"stableAddr": False}, open(os.path.join(d, "state.json"), "w"))
p3, u3 = start(env); stop(p3); time.sleep(0.5)
p4, u4 = start(env); stop(p4)
check("with the setting off the address changes", u3 != u4, f"{u3} vs {u4}")
# the saved port is taken by something else: Kempt still starts, on another port
json.dump({}, open(os.path.join(d, "state.json"), "w"))
p5, u5 = start(env); port5 = int(re.search(r":(\d+)/", u5).group(1)); stop(p5); time.sleep(0.5)
import socket
blocker = socket.socket(); blocker.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1); blocker.bind(("127.0.0.1", port5)); blocker.listen(1)
p6, u6 = start(env)
check("a taken port falls back to another one", bool(u6) and int(re.search(r":(\d+)/", u6).group(1)) != port5, u6)
check("and it answers there", get(u6) == 200)
stop(p6); blocker.close()
print(f"\n{PASS} passed, {FAIL} failed"); sys.exit(1 if FAIL else 0)
