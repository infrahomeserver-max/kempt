import os, re, subprocess, sys, tempfile, time
from playwright.sync_api import sync_playwright
BIN = "/home/claude/kempt/target/release/kempt"

class Gui:
    @staticmethod
    def _cfg():
        # the older tests drive the read-only viewer; they start with the 'open read-only' setting on
        d = tempfile.mkdtemp()
        if not os.environ.get('UI_EDITABLE'):
            open(os.path.join(d, 'state.json'), 'w').write('{"readonly": true}')
        return d
    def __init__(self, files=(), extra=()):
        self.p = subprocess.Popen([BIN, "gui", "--no-open", *extra, *files], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=dict(os.environ, KEMPT_CONFIG_DIR=os.environ.get("KEMPT_CONFIG_DIR") or self._cfg()))
        line = ""
        while "http://" not in line:
            line = self.p.stdout.readline()
            if not line and self.p.poll() is not None:
                raise RuntimeError(self.p.stderr.read())
        self.url = re.search(r"http://\S+", line).group(0)
    def stop(self):
        self.p.terminate(); self.p.wait(timeout=5)
