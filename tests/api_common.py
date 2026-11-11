"""HTTP API tests for `kempt gui` (backend). Starts the real server."""
import http.client, json, os, re, subprocess, sys, tempfile, time

BIN = os.environ.get("KEMPT_BIN") or os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "target", "release", "kempt" + (".exe" if os.name == "nt" else ""))
PASS = FAIL = 0


def check(name, cond, detail=""):
    global PASS, FAIL
    if cond:
        PASS += 1
        print(f"  PASS  {name}")
    else:
        FAIL += 1
        print(f"  FAIL  {name}  {str(detail)[:300]}")


class Server:
    def __init__(self, files=()):
        self.p = subprocess.Popen([BIN, "gui", "--no-open", *files], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        line = ""
        while "http://" not in line:
            line = self.p.stdout.readline()
            if not line and self.p.poll() is not None:
                raise RuntimeError(self.p.stderr.read())
        m = re.search(r"http://127.0.0.1:(\d+)/\?t=([0-9a-f]+)", line)
        self.port, self.token = int(m.group(1)), m.group(2)

    def req(self, method, path, body=None, headers=None, host=None):
        c = http.client.HTTPConnection("127.0.0.1", self.port, timeout=60)
        h = {"Host": host or f"127.0.0.1:{self.port}"}
        h.update(headers or {})
        data = None
        if body is not None:
            data = json.dumps(body)
            h["Content-Type"] = "application/json"
        c.request(method, path, data, h)
        r = c.getresponse()
        raw = r.read()
        c.close()
        return r.status, raw

    def api(self, name, body=None, ok=True):
        st, raw = self.req("POST", "/api/" + name, body or {}, {"X-Token": self.token})
        j = json.loads(raw)
        if ok and st != 200:
            raise AssertionError(f"{name}: {st} {j}")
        return j if ok else (st, j)

    def stop(self):
        self.p.terminate()
        self.p.wait(timeout=5)
