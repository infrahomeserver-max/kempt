import json, os, random, time
random.seed(7)
out = "/tmp/big"
os.makedirs(out, exist_ok=True)


def log_lines(n, start=0):
    levels = ["INFO"] * 90 + ["WARN"] * 7 + ["ERROR"] * 3
    for i in range(start, start + n):
        lvl = random.choice(levels)
        ts = f"2026-11-03 {(i // 3600) % 24:02d}:{(i // 60) % 60:02d}:{i % 60:02d}.{i % 1000:03d}"
        if i % 50 == 0:
            yield f'{ts} {lvl} {json.dumps({"event": "req", "id": i, "user": {"id": i % 977, "name": "u%d" % (i % 977)}, "tags": ["a", "b"], "ms": i % 500})}'
        else:
            yield f"{ts} {lvl} [worker-{i % 16}] request {i} handled path=/api/v1/items/{i % 5000} status={200 if lvl == 'INFO' else 500} in {i % 997}ms req=550e8400-e29b-41d4-a716-{i:012x}"


t = time.time()
N = 2_300_000
with open(f"{out}/big.log", "w") as f:
    for i in range(0, N, 100000):
        f.write("\n".join(log_lines(100000, i)) + "\n")
print("big.log", os.path.getsize(f"{out}/big.log") >> 20, "MB", round(time.time() - t, 1), "s", flush=True)


def variant(src, dst, frac, seed):
    r = random.Random(seed)
    with open(src) as a, open(dst, "w") as b:
        for line in a:
            x = r.random()
            if x < frac * 0.6:
                b.write(line.replace("handled", "HANDLED", 1))
            elif x < frac * 0.8:
                continue  # deleted
            elif x < frac:
                b.write(line)
                b.write("inserted line %d\n" % r.randrange(10**9))
            else:
                b.write(line)


variant(f"{out}/big.log", f"{out}/big_01.log", 0.001, 1)
variant(f"{out}/big.log", f"{out}/big_30.log", 0.30, 2)
print("variants done", round(time.time() - t, 1), "s", flush=True)


def chunk_json(k, n=2000):
    return ",".join(
        json.dumps(
            {"id": k * n + j, "name": f"user{j}", "email": f"u{j}@example.com", "active": j % 2 == 0, "score": j * 1.5,
             "tags": ["a", "b", {"x": [1, 2, 3]}], "note": 'he said "hi" {not} [real], ok'},
            separators=(",", ":"),
        )
        for j in range(n)
    )


def gen_json(path, mb):
    target = mb << 20
    with open(path, "w") as f:
        f.write('{"data":[')
        size = 0
        k = 0
        while size < target:
            c = chunk_json(k % 50)
            if k:
                f.write(",")
            f.write(c)
            size += len(c) + 1
            k += 1
        f.write("]}")


def chunk_xml(k, n=2000):
    return "".join(
        f'<item id="{k * n + j}"><name>user{j}</name><email>u{j}@example.com</email><tags><t>a</t><t>b</t></tags><!-- c{j} --><note><![CDATA[a < b & c]]></note></item>'
        for j in range(n)
    )


def gen_xml(path, mb):
    target = mb << 20
    with open(path, "w") as f:
        f.write('<?xml version="1.0"?><root>')
        size = 0
        k = 0
        while size < target:
            c = chunk_xml(k % 50)
            f.write(c)
            size += len(c)
            k += 1
        f.write("</root>")


gen_json(f"{out}/big.json", 250)
gen_xml(f"{out}/big.xml", 250)
gen_json(f"{out}/huge.json", 1024)
print("json/xml done", round(time.time() - t, 1), "s")
for fn in sorted(os.listdir(out)):
    print(f"  {fn:14s} {os.path.getsize(os.path.join(out, fn)) / 1048576:8.0f} MB")
