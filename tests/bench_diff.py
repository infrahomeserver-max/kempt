import json, os, subprocess, sys
sys.path.insert(0, os.path.dirname(__file__))
from bench import *
B = "/tmp/big"
print("== DIFF text (325 MB logs, 2.3M lines)")
subprocess.run(["cp", f"{B}/big.log", f"{B}/big_copy.log"])
code, dt, rss, err = run(["diff", "--stat", f"{B}/big.log", f"{B}/big_copy.log"], stdout=subprocess.PIPE)
row("identical copy (byte-compare fast path)", sz(f"{B}/big.log") * 2, code, dt, rss)
for tag, other in [("0.1% lines changed/added/removed", "big_01.log"), ("30% lines changed/added/removed", "big_30.log")]:
    p = subprocess.Popen([BIN, "diff", "--stat", f"{B}/big.log", f"{B}/{other}"], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    out = p.stdout.read().decode().strip()
    _, st, ru = os.wait4(p.pid, 0)
    # time it again for wall clock
    code, dt, rss, err = run(["diff", "--stat", f"{B}/big.log", f"{B}/{other}"], stdout=subprocess.PIPE)
    row(f"diff --stat  {tag}", sz(f"{B}/big.log") + sz(f"{B}/{other}"), code, dt, rss)
    print(f"      -> {out}")

print("\n== DIFF with log masking (timestamps / uuids ignored)")
code, dt, rss, err = run(["diff", "--stat", "--mask", f"{B}/big.log", f"{B}/big_01.log"], stdout=subprocess.PIPE)
row("diff --mask 0.1%", sz(f"{B}/big.log") * 2, code, dt, rss)

print("\n== PATCH export + exact apply (0.1% variant)")
code, dt, rss, err = run(["diff", "-q", f"{B}/big.log", f"{B}/big_01.log", "--patch", f"{B}/p.patch"])
row("diff --patch", sz(f"{B}/big.log") * 2, code, dt, rss, f"(patch {sz(f'{B}/p.patch'):.1f} MB)")
subprocess.run(["cp", f"{B}/big.log", f"{B}/applied.log"])
r = subprocess.run(["patch", "-s", f"{B}/applied.log", f"{B}/p.patch"])
same = subprocess.run(["cmp", "-s", f"{B}/applied.log", f"{B}/big_01.log"]).returncode == 0
print(f"      patch(1) applied cleanly (exit {r.returncode}); result byte-identical to the second file: {same}")
os.remove(f"{B}/applied.log")

print("\n== DIFF unified output to a pipe (what you'd page/grep)")
code, dt, rss, err = run(["diff", f"{B}/big.log", f"{B}/big_01.log"], stdout=subprocess.PIPE if False else open(os.devnull, "w"))
row("diff unified  0.1%", sz(f"{B}/big.log") * 2, code, dt, rss)
code, dt, rss, err = run(["diff", "--color", f"{B}/big.log", f"{B}/big_01.log"], stdout=open(os.devnull, "w"))
row("diff unified --color (inline word highlights) 0.1%", sz(f"{B}/big.log") * 2, code, dt, rss)
code, dt, rss, err = run(["diff", "--html", f"{B}/r.html", "-q", f"{B}/big.log", f"{B}/big_01.log"], stdout=open(os.devnull, "w"))
row("diff --html report 0.1%", sz(f"{B}/big.log") * 2, code, dt, rss, f"(report {sz(f'{B}/r.html'):.1f} MB)")

print("\n== DIFF JSON, 250 MB each (over the structural limit -> format-then-diff, streamed)")
s = open(f"{B}/big.json").read()
s2 = s.replace('"name":"user5"', '"name":"CHANGED"', 3).replace('"active":true', '"active":false', 2)
open(f"{B}/big2.json", "w").write(s2); del s, s2
code, dt, rss, err = run(["diff", "--stat", f"{B}/big.json", f"{B}/big2.json"], stdout=subprocess.PIPE)
row("diff big.json big2.json (auto -> format-then-diff)", sz(f"{B}/big.json") * 2, code, dt, rss, err.strip()[:70])

print("\n== STRUCTURAL JSON diff (100 MB each, key order/layout ignored)")
d = json.load(open(f"{B}/big.json"))
d["data"] = d["data"][:380000]
json.dump(d, open(f"{B}/s1.json", "w"), separators=(",", ":"))
d["data"][5]["name"] = "changed"; d["data"][70000]["tags"][2]["x"].append(9); d["data"].pop(200000)
# different key order + pretty layout in the second file
json.dump(d, open(f"{B}/s2.json", "w"), indent=1, sort_keys=True)
del d
print(f"      sizes: {sz(f'{B}/s1.json'):.0f} MB compact vs {sz(f'{B}/s2.json'):.0f} MB pretty/sorted")
p = subprocess.Popen([BIN, "diff", f"{B}/s1.json", f"{B}/s2.json"], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
out = p.stdout.read().decode(); _, st, ru = os.wait4(p.pid, 0)
print(f"      peak RSS {ru.ru_maxrss/1024:.0f} MB; output lines: {len(out.splitlines())}")
print("      " + "\n      ".join(out.splitlines()[:4]) + "\n      ...\n      " + out.splitlines()[-1][:110])
