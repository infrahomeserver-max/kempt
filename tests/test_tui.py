import json, os, sys, tempfile, time
sys.path.insert(0, os.path.dirname(__file__))
from tui_harness import Session

PASS = FAIL = 0


def check(name, cond, detail=""):
    global PASS, FAIL
    if cond:
        PASS += 1
        print(f"  PASS  {name}")
    else:
        FAIL += 1
        print(f"  FAIL  {name}  {detail}")


d = tempfile.mkdtemp()

# ---------- sample log: 3000 lines, mixed levels, some JSON payloads ----------
log = os.path.join(d, "app.log")
with open(log, "w") as f:
    for i in range(3000):
        lvl = "ERROR" if i % 100 == 7 else "WARN" if i % 50 == 3 else "INFO"
        ts = f"2026-11-03 10:{(i // 60) % 60:02d}:{i % 60:02d}"
        if i == 10:
            f.write(f"{ts} INFO {json.dumps({'event': 'login', 'user': {'id': 42, 'name': 'ann'}, 'tags': ['a', 'b']})}\n")
        else:
            f.write(f"{ts} {lvl} request {i} handled in {i % 97}ms\n")

print("== VIEWER")
s = Session(["view", log])
s.pump(1.0)
check("opens and shows first line with line number", "2026-11-03 10:00:00 INFO request 0 handled" in s.text() and s.line(0).lstrip().startswith("1 "), s.line(0))
check("status bar shows file name and position", "app.log" in s.line(29) and "/3000" in s.line(29), s.line(29))
# timestamp coloured (truecolor ts_fg = 110,170,220 -> hex 6eaadc)
check("timestamp is colour-highlighted", s.fg_at(0, 6) == "6eaadc", s.fg_at(0, 6))

# search
s.send("/ERROR\r", 0.8)
check("search jumps to first ERROR (line 8)", s.find("request 7 handled") is not None and "8 " in s.text(), s.text()[:200])
check("search shows match count in status", "/ERROR/" in s.line(29) and "match" in s.line(29), s.line(29))
r, c = s.find("ERROR")
check("search match highlighted (magenta bg)", s.bg_at(r, c) == "be3cbe", s.bg_at(r, c))
s.send("n", 0.3)
check("n goes to next ERROR (line 108)", "request 107 handled" in s.text(), s.text()[:300])
s.send("N", 0.3)
check("N goes back to previous ERROR", "request 7 handled" in s.text())

# filter
s.send("&WARN\r", 1.0)
t = s.text()
check("filter shows only WARN lines", "WARN" in t and "INFO" not in "\n".join(s.screen.display[:29]), t[:300])
check("filter status shows line count", "filter /WARN/ 60 lines" in s.line(29), s.line(29))
s.send("&", 0.4)      # prompt is pre-filled with the current pattern
s.send("\x15", 0.2)   # Ctrl-U clears the prompt text
s.send("\r", 0.6)
check("empty filter clears it", "filter" not in s.line(29) or "filter cleared" in s.line(29), s.line(29))

# quick level filter
s.send("e", 1.0)
check("'e' filters to errors only", "ERROR" in s.text() and "INFO request 1 " not in s.text() and "30 lines" in s.line(29), s.line(29))
s.send("e", 0.6)
check("'e' again clears the filter", "30 lines" not in s.line(29), s.line(29))

# goto line, end, home
s.send(":2500\r", 0.6)
check("goto line 2500", "request 2499 handled" in s.text(), s.text()[:200])
s.send("G", 0.4)
check("G goes to last line", "request 2999 handled" in s.text())
s.send("g", 0.4)
check("g goes to first line", "request 0 handled" in s.text())

# timestamp jump
s.send("t2026-11-03 10:30\r", 0.8)
check("timestamp jump lands on 10:30:00", "2026-11-03 10:30:00" in s.text(), s.text()[:300])

# detail pane pretty-prints the JSON line
s.send(":11\r", 0.5)
s.send("\r", 0.6)
t = s.text()
check("Enter opens detail pane with pretty JSON", '"event": "login"' in t and '"id": 42' in t and "detail" in t, t[-900:])
s.send("\r", 0.4)
check("Enter closes detail pane", "detail — selected" not in s.text())

# help
s.send("?", 0.4)
check("help overlay shows", "kempt view — keys" in s.text())
s.send("x", 0.3)
check("any key closes help", "kempt view — keys" not in s.text())

# scrolling horizontally
s.send("l", 0.3)
check("horizontal scroll shifts text", "2026-11-03 10:00:00 INFO" not in s.line(0), s.line(0))
s.send("0", 0.3)

# quit & terminal restored
s.send("q", 0.2)
code = s.wait_exit()
check("q exits with status 0", code == 0, str(code))
check("alt screen left on exit", b"\x1b[?1049l" in s.raw)
s.kill()

# ---------- big JSON opens formatted ----------
print("== VIEWER formats JSON on open")
js = os.path.join(d, "data.json")
json.dump({"users": [{"id": i, "name": f"u{i}", "roles": ["a", "b"]} for i in range(200)]}, open(js, "w"), separators=(",", ":"))
s = Session(["view", js])
s.pump(1.0)
check("minified JSON shown pretty-printed", s.line(0).strip().endswith("{") and '"users": [' in s.text(), s.text()[:200])
check("status says formatted", "formatted" in s.line(29), s.line(29))
s.send("q", 0.2); s.wait_exit(); s.kill()

s = Session(["view", "--raw", js])
s.pump(1.0)
check("--raw shows the file as-is (one line)", s.line(0).strip().startswith('1 {"users":[{"id":0'), s.line(0)[:80])
s.send("q", 0.2); s.wait_exit(); s.kill()

# ---------- follow mode ----------
print("== VIEWER follow mode")
fl = os.path.join(d, "live.log")
open(fl, "w").write("2026-11-03 12:00:00 INFO start\n")
s = Session(["view", "-f", fl])
s.pump(0.8)
check("follow shows FOLLOW in status", "FOLLOW" in s.line(29), s.line(29))
with open(fl, "a") as f:
    f.write("2026-11-03 12:00:01 INFO appended one\n")
    f.flush()
s.pump(1.0)
check("appended line appears live", "appended one" in s.text(), s.text()[:200])
with open(fl, "a") as f:
    for i in range(60):
        f.write(f"2026-11-03 12:01:{i % 60:02d} INFO burst {i}\n")
s.pump(1.0)
check("follow sticks to the bottom after a burst", "burst 59" in s.text(), s.text()[-400:])
# rotation: truncate + new content
open(fl, "w").write("2026-11-03 13:00:00 INFO after rotation\n")
s.pump(1.0)
check("rotation/truncate is detected and reloaded", "after rotation" in s.text() and "burst 5" not in s.text(), s.text()[:200])
s.send("q", 0.2); s.wait_exit(); s.kill()

# ---------- DIFF TUI ----------
print("== DIFF TUI")
a = os.path.join(d, "a.txt"); b = os.path.join(d, "b.txt")
A = [f"line {i}" for i in range(200)]
B = list(A)
B[20] = "line 20 CHANGED"
del B[100]
B.insert(150, "brand new line")
open(a, "w").write("\n".join(A) + "\n")
open(b, "w").write("\n".join(B) + "\n")
s = Session(["diff", "--tui", a, b], cols=120)
s.pump(1.0)
t = s.text()
check("header shows both file names", "a.txt" in s.line(0) and "b.txt" in s.line(0), s.line(0))
check("first change shown with ~ marker", "line 20" in t and "~" in t and "CHANGED" in t, t[:600])
r, c = s.find("CHANGED")
# right side inline-changed word is bold on strong-green bg (34,134,58 = 22863a)
check("changed text has strong green background", s.bg_at(r, c + 1) == "22863a", s.bg_at(r, c + 1))
r2, c2 = s.find("line 20")
check("changed line tinted dark green on right / red on left", s.bg_at(r2, 8) in ("481e1e", "48181e", "48181e".upper()) or s.bg_at(r2, 8) != "default", s.bg_at(r2, 8))
check("status bar shows counts and diff position", "+1" in s.line(29) and "-1" in s.line(29) and "~1" in s.line(29) and "diff 1/3" in s.line(29), s.line(29))
check("ruler column present (coloured block)", any(s.screen.buffer[r][119].data == "█" for r in range(1, 29)))
s.send("n", 0.4)
check("n jumps to next diff (removed line 101)", "line 100" in s.text() and "diff 2/3" in s.line(29), s.line(29))
s.send("n", 0.4)
check("n jumps to third diff (added line)", "brand new line" in s.text() and "diff 3/3" in s.line(29), s.line(29))
s.send("p", 0.4)
check("p goes back", "diff 2/3" in s.line(29))
s.send("c", 0.5)
check("c switches to full-file view", "full file" in s.line(29), s.line(29))
s.send("c", 0.5)
check("c switches back to changes-only (folded)", "changes only" in s.line(29) and "unchanged lines" in s.text(), s.text()[:300])
s.send("?", 0.3)
check("help overlay in diff", "kempt diff — keys" in s.text())
s.send("z", 0.3)
s.send("q", 0.2)
check("q exits diff with 1 (files differ)", s.wait_exit() == 1)
s.kill()

# ---------- diff TUI with colour disabled keeps markers ----------
s = Session(["diff", "--tui", "--no-color", a, b], env={"NO_COLOR": "1"})
s.pump(1.0)
check("NO_COLOR: markers still convey changes", "~" in s.text() and "+" in s.text(), s.text()[:300])
s.send("q", 0.2); s.wait_exit(); s.kill()

print(f"\n{PASS} passed, {FAIL} failed")
sys.exit(1 if FAIL else 0)
