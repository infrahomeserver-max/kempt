import filecmp, os, subprocess, sys
sys.path.insert(0, os.path.dirname(__file__))
from bench import *

B = "/tmp/big"
print("== FORMAT (streaming, constant memory)")
for name in ["big.json", "big.xml", "huge.json"]:
    src = f"{B}/{name}"
    pretty = f"{B}/pretty_{name}"
    code, dt, rss, err = run(["format", "-q", src, "-o", pretty])
    row(f"format {name} -> pretty", sz(src), code, dt, rss, f"(output {sz(pretty):.0f} MB)")
    mini = f"{B}/mini_{name}"
    code, dt, rss, err = run(["format", "-q", "--minify", pretty, "-o", mini])
    row(f"minify pretty_{name} -> compact", sz(pretty), code, dt, rss)
    # roundtrip: original has no insignificant whitespace, so minify(format(x)) == x + "\n"
    a_sz, b_sz = os.path.getsize(src), os.path.getsize(mini)
    same = subprocess.run(["cmp", "-n", str(a_sz), src, mini]).returncode == 0 and b_sz == a_sz + 1
    print(f"      lossless roundtrip minify(format(x)) == x : {'IDENTICAL (byte for byte)' if same else 'MISMATCH'}", flush=True)
    os.remove(mini)

# check mode on already formatted big file (single pass, early exit not possible)
code, dt, rss, err = run(["format", "-q", "--check", f"{B}/pretty_big.json"])
row("format --check pretty_big.json (already formatted)", sz(f"{B}/pretty_big.json"), code, dt, rss)
code, dt, rss, err = run(["format", "-q", "--check", f"{B}/big.json"])
row("format --check big.json (unformatted; early exit)", sz(f"{B}/big.json"), code, dt, rss)

# in-place atomic on a copy
subprocess.run(["cp", f"{B}/big.xml", f"{B}/inplace.xml"])
code, dt, rss, err = run(["format", "-q", f"{B}/inplace.xml"])
row("format in place (atomic) big.xml", sz(f"{B}/inplace.xml"), code, dt, rss)
print("      identical to -o output:", filecmp.cmp(f"{B}/inplace.xml", f"{B}/pretty_big.xml", shallow=False))
os.remove(f"{B}/inplace.xml")
