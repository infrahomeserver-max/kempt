import os, subprocess, sys, time

BIN = os.environ.get("KEMPT_BIN") or os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "target", "release", "kempt" + (".exe" if os.name == "nt" else ""))


def run(args, stdout=subprocess.DEVNULL, label=None):
    """run kempt, return (exit, seconds, peak RSS MB) using wait4 rusage of this exact child"""
    t = time.time()
    p = subprocess.Popen([BIN] + args, stdout=stdout, stderr=subprocess.PIPE)
    _, st, ru = os.wait4(p.pid, 0)
    dt = time.time() - t
    err = p.stderr.read().decode()
    code = os.waitstatus_to_exitcode(st)
    return code, dt, ru.ru_maxrss / 1024, err


def row(name, size_mb, code, dt, rss, extra=""):
    print(f"  {name:48s} {size_mb:7.0f} MB  {dt:6.2f}s  {size_mb / dt:6.0f} MB/s  peak RSS {rss:7.1f} MB  exit={code} {extra}", flush=True)


def sz(p):
    return os.path.getsize(p) / 1048576
