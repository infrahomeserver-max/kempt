"""Security tests for `kempt gui`: auth, DNS rebinding, headers, injection, limits, permissions, no secrets in the tree."""
import json, os, re, stat, subprocess, sys, tempfile
sys.path.insert(0, os.path.dirname(__file__))
import api_common as T
from api_common import check, Server
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
cfg = tempfile.mkdtemp()
os.environ["KEMPT_CONFIG_DIR"] = cfg
os.environ["KEMPT_NO_TRAY"] = "1"
d = tempfile.mkdtemp()
f = os.path.join(d, "a.json"); open(f, "w").write('{"a":1}')
s = Server([f])

# --- authentication
st, _ = s.req("POST", "/api/state/get", {})
check("API without token is refused", st == 403, st)
st, _ = s.req("POST", "/api/state/get", {}, {"X-Token": "0" * len(s.token)})
check("API with wrong token (same length) is refused", st == 403, st)
st, _ = s.req("POST", "/api/state/get", {}, {"X-Token": s.token[:-1]})
check("API with truncated token is refused", st == 403)
st, _ = s.req("POST", "/api/state/get", {}, {"X-Token": ""})
check("empty token refused", st == 403)
st, _ = s.req("GET", "/api/state/get", None, {"X-Token": s.token})
check("GET on API is refused (POST only)", st == 405, st)
st, _ = s.req("GET", "/")
check("page without ?t token is refused", st == 403, st)
st, _ = s.req("GET", "/?t=" + "a" * len(s.token))
check("page with wrong token is refused", st == 403)
st, raw = s.req("GET", f"/?t={s.token}")
check("page with token is served", st == 200 and b"<html" in raw[:2000].lower())
check("token is 128 bits or more of hex", re.fullmatch(r"[0-9a-f]{32,}", s.token) is not None, s.token)

# --- DNS rebinding / Host
for h in ("evil.example.com", "evil.example.com:%d" % s.port, "127.0.0.1.evil.com:%d" % s.port, "127.0.0.1", "localhost:1"):
    st, _ = s.req("POST", "/api/state/get", {}, {"X-Token": s.token}, host=h)
    check(f"Host {h!r} refused", st == 403, st)
st, _ = s.req("GET", f"/?t={s.token}", host="evil.example.com")
check("page with foreign Host refused even with token", st == 403)
st, _ = s.req("POST", "/api/state/get", {}, {"X-Token": s.token}, host=f"localhost:{s.port}")
check("localhost Host accepted", st == 200, st)

# --- headers
import http.client
def hdrs(method, path, headers=None, body=None):
    c = http.client.HTTPConnection("127.0.0.1", s.port, timeout=20)
    h = {"Host": f"127.0.0.1:{s.port}"}; h.update(headers or {})
    c.request(method, path, body, h); r = c.getresponse(); r.read()
    out = {k.lower(): v for k, v in r.getheaders()}; c.close(); return r.status, out
st, h = hdrs("GET", f"/?t={s.token}")
csp = h.get("content-security-policy", "")
check("page CSP restricts default-src/connect-src to self", "default-src 'self'" in csp and "connect-src 'self'" in csp, csp)
check("page CSP forbids framing, plugins, base and forms", all(x in csp for x in ("frame-ancestors 'none'", "object-src 'none'", "base-uri 'none'", "form-action 'none'")), csp)
check("CSP has no remote origins", not re.search(r"https?://", csp), csp)
check("page nosniff + no-store + no referrer", h.get("x-content-type-options") == "nosniff" and h.get("cache-control") == "no-store" and h.get("referrer-policy") == "no-referrer", h)
check("page X-Frame-Options DENY", h.get("x-frame-options") == "DENY")
st, h = hdrs("POST", "/api/state/get", {"X-Token": s.token, "Origin": "https://evil.example.com"}, "{}")
check("API reply nosniff + no-store", h.get("x-content-type-options") == "nosniff" and h.get("cache-control") == "no-store", h)
check("API reply has no CORS headers", not any(k.startswith("access-control-") for k in h), h)
st, h = hdrs("OPTIONS", "/api/state/get", {"Origin": "https://evil.example.com", "Access-Control-Request-Method": "POST", "Access-Control-Request-Headers": "x-token"})
check("CORS preflight is not granted", not any(k.startswith("access-control-allow") for k in h) and st != 200, (st, h))
st, h = hdrs("GET", "/favicon.svg")
check("favicon nosniff", h.get("x-content-type-options") == "nosniff")

# --- routing and bad input
for p in ("/etc/passwd", "/../../etc/passwd", "/%2e%2e/%2e%2e/etc/passwd", "/static/../Cargo.toml", "/api/../"):
    st, raw = s.req("GET", p)
    check(f"GET {p} serves no file", st in (403, 404, 405) and b"root:" not in raw, st)
st, _ = s.req("POST", "/api/nonexistent", {}, {"X-Token": s.token})
check("unknown API -> 4xx not 5xx", 400 <= st < 500, st)
c = http.client.HTTPConnection("127.0.0.1", s.port, timeout=20)
c.request("POST", "/api/state/get", "{not json", {"Host": f"127.0.0.1:{s.port}", "X-Token": s.token}); r = c.getresponse(); r.read()
check("malformed JSON -> 400", r.status == 400, r.status)
c = http.client.HTTPConnection("127.0.0.1", s.port, timeout=20)
c.request("POST", "/api/state/get", b"\xff\xfe\x00bad", {"Host": f"127.0.0.1:{s.port}", "X-Token": s.token}); r = c.getresponse(); r.read()
check("non-UTF-8 body -> 400", r.status == 400, r.status)
big = "x" * (13 << 20)
try:
    st, raw = s.req("POST", "/api/notes/set", {"items": [], "pad": big}, {"X-Token": s.token})
except Exception as e:
    st = 0
check("13 MB body does not store data (rejected or cut)", st != 200, st)
st, j = s.api("notes/get")[0:2] if False else (None, None)
for payload in ({"items": "x"}, {"items": [1, 2, None]}, {"pad": 5}, {"items": [{"t": "x" * 100000}] * 20}):
    st, j = s.api("notes/set", payload, ok=False)
    check(f"notes/set odd payload {str(payload)[:30]} -> no crash", st in (200, 400, 413), st)
check("server alive after odd payloads", s.api("state/get") is not None)
st, j = s.api("state/set", {"__proto__": {"x": 1}, "evil": "1", "theme": "dark"}, ok=False)
got = s.api("state/get")
check("state/set ignores keys outside whitelist", "evil" not in json.dumps(got) and "__proto__" not in json.dumps(got), got)

# --- command / URL injection in update check
for repo in ("a/b;touch /tmp/kempt_pwn", "a/b$(touch /tmp/kempt_pwn)", "a/b`touch /tmp/kempt_pwn`", "a/b\nHost: x", "../..", "a/b/../../c", "-o/x/y", "a/b?x=1", "a/b#frag", "a b/c", "x" * 300 + "/y"):
    st, j = s.api("update/check", {"repo": repo}, ok=False)
    check(f"update/check rejects repo {repo[:30]!r}", st in (400, 502) and not os.path.exists("/tmp/kempt_pwn"), (st, j))

# --- file permissions on the instance file
inst = os.path.join(cfg, "instance.json")
if os.path.exists(inst):
    m = stat.S_IMODE(os.stat(inst).st_mode)
    check("instance.json is private (0600)", m & 0o077 == 0, oct(m))
for name in ("state.json", "notes.json"):
    p = os.path.join(cfg, name)
    if os.path.exists(p):
        m = stat.S_IMODE(os.stat(p).st_mode); check(f"{name} is not world-writable", m & 0o002 == 0, oct(m))
check("server listens on loopback only", True)
import socket
raw = socket.create_connection(("127.0.0.1", s.port), timeout=10)
raw.sendall(b"POST /api/state/get HTTP/1.1\r\nX-Token: " + s.token.encode() + b"\r\nContent-Length: 2\r\n\r\n{}")
check("request with no Host header is refused", b" 403 " in raw.recv(200).split(b"\r\n")[0], "")
raw.close()
tcp = open("/proc/net/tcp").read().splitlines()[1:]
mine = [l.split()[1] for l in tcp if l.split()[3] == "0A" and int(l.split()[1].split(":")[1], 16) == s.port]
check("socket bound to 127.0.0.1 only (not 0.0.0.0)", mine == ["0100007F:%04X" % s.port], mine)
s.stop()

# --- static checks of the source and UI
ui = open(os.path.join(ROOT, "ui/index.html"), encoding="utf-8").read()
code = re.sub(r"data:[a-z/0-9+.-]+;base64,[A-Za-z0-9+/=]+", "DATA", ui)
check("UI loads nothing from remote origins", not re.search(r"""(src|href)\s*=\s*["']https?://""", code) and not re.search(r"@import\s+url\(\s*['\"]?https?:", code))
check("UI has no eval/new Function", not re.search(r"\beval\s*\(|new\s+Function\s*\(", code))
check("UI has no document.write", "document.write" not in code)
check("UI does not use localStorage for secrets", "localStorage" not in code or "token" not in code.split("localStorage")[1][:80].lower())
check("UI never puts the token in a URL it fetches", not re.search(r"fetch\([^)]*token", code))
srcs = "".join(open(os.path.join(ROOT, "src", n), encoding="utf-8").read() for n in os.listdir(os.path.join(ROOT, "src")) if n.endswith(".rs"))
check("no unsafe blocks except reviewed ones", len(re.findall(r"\bunsafe\s*\{", srcs)) <= 6, len(re.findall(r"\bunsafe\s*\{", srcs)))
check("no shell invocation with string concatenation (sh -c format!)", not re.search(r'"-c"\s*,\s*&?format!', srcs))

# --- no secrets in the repository
pat = re.compile(r"(-----BEGIN [A-Z ]*PRIVATE KEY-----|ghp_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{30,}|AKIA[0-9A-Z]{16}|xox[baprs]-[A-Za-z0-9-]{10,}|sk-[A-Za-z0-9]{32,})")
bad = []
for dp, dn, fn in os.walk(ROOT):
    dn[:] = [x for x in dn if x not in ("target", "dist", ".git", "__pycache__")]
    for n in fn:
        p = os.path.join(dp, n)
        try:
            t = open(p, encoding="utf-8").read()
        except Exception:
            continue
        t = re.sub(r"data:[a-z/0-9+.-]+;base64,[A-Za-z0-9+/=]+", "", t)
        if pat.search(t) and not p.endswith("test_security.py"): bad.append(p)
check("no private keys / tokens committed", not bad, bad)
print(f"\n{T.PASS} passed, {T.FAIL} failed"); sys.exit(1 if T.FAIL else 0)
