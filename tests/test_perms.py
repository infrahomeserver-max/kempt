"""Files Kempt writes (token, settings, launch log) are private to the user; a dropped file's folder too."""
import os, re, stat, subprocess, sys, tempfile, time, http.client, json
BIN = os.environ.get("KEMPT_BIN") or os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "target", "release", "kempt")
if os.name != "posix": print("skipped: POSIX permissions only"); sys.exit(0)
ok = True
def chk(n, c, extra=""):
    global ok; print(("  PASS  " if c else "  FAIL  ") + n + ("" if c else "  " + str(extra))); ok &= bool(c)
cfg = tempfile.mkdtemp(); d = tempfile.mkdtemp(); env = dict(os.environ, KEMPT_CONFIG_DIR=cfg)
f = os.path.join(d, "a.log"); open(f, "w").write("x\n")
os.umask(0o022)   # a typical default: new files would be world-readable unless Kempt restricts them
p = subprocess.Popen([BIN, "gui", "--no-open", f], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
line = ""
while "http://" not in line: line = p.stdout.readline()
port, tok = re.search(r"127.0.0.1:(\d+)/\?t=([0-9a-f]+)", line).groups()
def api(name, body):
    c = http.client.HTTPConnection("127.0.0.1", int(port), timeout=20); c.request("POST", "/api/" + name, json.dumps(body), {"Host": f"127.0.0.1:{port}", "X-Token": tok}); r = c.getresponse(); b = r.read(); c.close(); return r.status, b
api("state/set", {"theme": "dark"}); api("notes/set", {"items": []})
subprocess.run([BIN, "gui", f], env=env, capture_output=True, timeout=30)   # writes launch.log (the window never answers; it waits then gives up)
mode = lambda n: stat.S_IMODE(os.stat(os.path.join(cfg, n)).st_mode)
for n in sorted(os.listdir(cfg)):
    fp = os.path.join(cfg, n)
    if os.path.isfile(fp) and n in ("address.json", "instance.json", "launch.log", "state.json", "notes.json"):
        chk("%s is 0600 (is %o)" % (n, mode(n)), mode(n) & 0o077 == 0, oct(mode(n)))
# a dragged-in file's copy
c = http.client.HTTPConnection("127.0.0.1", int(port), timeout=20)
c.request("POST", "/api/upload?name=drop.txt&size=3", b"abc", {"Host": f"127.0.0.1:{port}", "X-Token": tok, "Content-Length": "3"}); r = c.getresponse(); r.read()
drops = [os.path.join(dp) for dp, dn, fn in os.walk(os.environ.get("TMPDIR", "/tmp")) if os.path.basename(dp) == "kempt-drops" or dp.endswith("/drops")]
for dd in drops[:3]:
    chk("drop folder %s is not readable by others (%o)" % (dd, stat.S_IMODE(os.stat(dd).st_mode)), stat.S_IMODE(os.stat(dd).st_mode) & 0o007 == 0)
p.terminate(); p.wait(timeout=10)
sys.exit(0 if ok else 1)
