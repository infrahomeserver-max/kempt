"""PTY harness: run kempt in a real pseudo-terminal, send keys, inspect the rendered screen (pyte)."""
import fcntl, os, pty, select, struct, subprocess, termios, time, signal
import pyte

BIN = "/home/claude/kempt/target/release/kempt"


class Session:
    def __init__(self, args, cols=110, rows=30, env=None):
        self.cols, self.rows = cols, rows
        self.screen = pyte.Screen(cols, rows)
        self.stream = pyte.ByteStream(self.screen)
        e = dict(os.environ, TERM="xterm-256color", COLORTERM="truecolor")
        e.pop("NO_COLOR", None)
        if env:
            e.update(env)
        self.pid, self.fd = pty.fork()
        if self.pid == 0:
            os.execve(BIN, [BIN] + args, e)
        fcntl.ioctl(self.fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
        os.kill(self.pid, signal.SIGWINCH)
        self.status = None
        self.raw = b""

    def pump(self, secs=0.5):
        end = time.time() + secs
        while time.time() < end:
            r, _, _ = select.select([self.fd], [], [], 0.05)
            if r:
                try:
                    data = os.read(self.fd, 65536)
                except OSError:
                    break
                if not data:
                    break
                self.raw += data
                self.stream.feed(data)
        return self

    def send(self, keys, wait=0.4):
        os.write(self.fd, keys.encode() if isinstance(keys, str) else keys)
        return self.pump(wait)

    def text(self):
        return "\n".join(l.rstrip() for l in self.screen.display)

    def line(self, i):
        return self.screen.display[i].rstrip()

    def bg_at(self, row, col):
        return self.screen.buffer[row][col].bg

    def fg_at(self, row, col):
        return self.screen.buffer[row][col].fg

    def find(self, needle):
        for r, l in enumerate(self.screen.display):
            c = l.find(needle)
            if c >= 0:
                return r, c
        return None

    def wait_exit(self, timeout=3):
        end = time.time() + timeout
        while time.time() < end:
            self.pump(0.1)
            p, st = os.waitpid(self.pid, os.WNOHANG)
            if p:
                self.status = os.waitstatus_to_exitcode(st)
                return self.status
        return None

    def kill(self):
        try:
            os.kill(self.pid, signal.SIGKILL)
            os.waitpid(self.pid, 0)
        except Exception:
            pass
