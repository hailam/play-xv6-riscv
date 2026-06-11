#!/usr/bin/env python3
"""
Headless check for the display server (todo 12 M4).

Boots with ramfb + virtio keyboard + QMP, runs `guidemo` from the
guest shell (forks: wm display server, hello_wm client, clock client),
then verifies via screendump + QMP send-key:

  1. both windows composite onto the framebuffer at their slots
     (hello_wm = cyan @ slot 0, clock = palette color @ slot 1, white
     window borders), and
  2. an injected key routes through wm's poll loop to the focused
     (want_keys) client, which repaints red.

Run from the repo root after `make build && make fs.img`:
    python3 scripts/test-gui.py
Exit 0 on success, 1 on failure.
"""
import json
import os
import pty
import select
import signal
import socket
import sys
import tempfile
import time

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
KERNEL = "target/riscv64gc-unknown-none-elf/release/kernel"

# wm geometry: slot i at x=8+i*168, y=8; clients are 120x90.
HELLO_CENTER = (8 + 60, 8 + 45)        # slot 0
CLOCK_CENTER = (8 + 168 + 60, 8 + 45)  # slot 1
BORDER_PX = (8 + 60, 6)                # top border row of slot 0
CYAN = (0x00, 0xFF, 0xFF)
RED = (0xFF, 0x00, 0x00)
WHITE = (0xFF, 0xFF, 0xFF)
CLOCK_PALETTE = {
    (0xFF, 0xA5, 0x00), (0x80, 0x00, 0x80),
    (0x00, 0x80, 0x00), (0xFF, 0xC0, 0xCB),
}


def read_ppm(path):
    with open(path, "rb") as f:
        data = f.read()
    assert data[:2] == b"P6"
    idx, fields = 2, []
    while len(fields) < 3:
        while data[idx] in b" \t\n\r":
            idx += 1
        start = idx
        while data[idx] not in b" \t\n\r":
            idx += 1
        fields.append(int(data[start:idx]))
    idx += 1
    return fields[0], fields[1], data[idx:]


def px(w, pix, x, y):
    off = (y * w + x) * 3
    return (pix[off], pix[off + 1], pix[off + 2])


def main():
    os.chdir(REPO)
    tmp = tempfile.mkdtemp()
    qmp_path = os.path.join(tmp, "qmp.sock")

    pid, fd = pty.fork()
    if pid == 0:
        os.execvp("qemu-system-riscv64", [
            "qemu-system-riscv64",
            "-machine", "virt", "-bios", "none", "-kernel", KERNEL,
            "-m", "256M", "-smp", "1", "-nographic",
            "-device", "ramfb",
            "-device", "virtio-keyboard-device,bus=virtio-mmio-bus.2",
            "-qmp", f"unix:{qmp_path},server,nowait",
            "-global", "virtio-mmio.force-legacy=false",
            "-drive", "file=fs.img,if=none,format=raw,id=x0",
            "-device", "virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0",
        ])
        os._exit(127)

    buf = bytearray()

    def text():
        return buf.decode("utf-8", "replace")

    def pump(until, timeout):
        deadline = time.time() + timeout
        while time.time() < deadline:
            r, _, _ = select.select([fd], [], [], 0.3)
            if r:
                try:
                    buf.extend(os.read(fd, 4096))
                except OSError:
                    return False
            if until in text():
                return True
        return False

    def qmp(cmd, args=None):
        sk = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        sk.connect(qmp_path)
        f = sk.makefile("rwb")
        f.readline()
        f.write(b'{"execute":"qmp_capabilities"}\n')
        f.flush()
        f.readline()
        msg = {"execute": cmd}
        if args:
            msg["arguments"] = args
        f.write(json.dumps(msg).encode() + b"\n")
        f.flush()
        f.readline()
        sk.close()

    def screendump(name):
        p = os.path.join(tmp, name)
        qmp("screendump", {"filename": p})
        time.sleep(0.4)
        return read_ppm(p)

    checks = []

    def check(label, ok):
        checks.append(ok)
        print(f"  [{'OK' if ok else 'FAIL'}] {label}")

    try:
        if not pump("\n$", 15):
            print("FAIL: no shell prompt")
            return 1
        os.write(fd, b"guidemo\n")
        check("wm ready", pump("wm: ready", 15))
        check("hello mapped", pump("hello: mapped", 15))
        # clock's startup print interleaves with wm's (two procs on
        # one console) — don't string-match it; the screendump below
        # is the real assertion for clock's window.
        time.sleep(4.0)  # guidemo's stagger + first clock blit

        w, h, pix = screendump("one.ppm")
        print(f"screendump 1: {w}x{h}")
        check("hello window cyan", px(w, pix, *HELLO_CENTER) == CYAN)
        check("window border white", px(w, pix, *BORDER_PX) == WHITE)
        check("clock window in palette",
              px(w, pix, *CLOCK_CENTER) in CLOCK_PALETTE)

        qmp("send-key", {"keys": [{"type": "qcode", "data": "x"}]})
        check("key routed to hello", pump("hello: key 45", 10))
        time.sleep(1.0)  # let the red repaint drain

        w, h, pix = screendump("two.ppm")
        check("hello repainted red", px(w, pix, *HELLO_CENTER) == RED)

        ok = all(checks)
        if not ok:
            print("serial tail:")
            print("\n".join(text().splitlines()[-12:]))
        print("RESULT:", "PASS" if ok else "FAIL")
        return 0 if ok else 1
    finally:
        try:
            os.kill(pid, signal.SIGKILL)
            os.waitpid(pid, 0)
        except OSError:
            pass


if __name__ == "__main__":
    sys.exit(main())
