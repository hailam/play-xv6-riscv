#!/usr/bin/env python3
"""
Headless check for the virtio-input keyboard path (todo 12 M3).

Boots the riscv64 kernel with `-device virtio-keyboard-device` and a
QMP socket, runs `/kbtest` from the guest shell (it blocks reading
/dev/input/0), injects a key press via QMP `send-key`, and verifies
the evdev events reach userspace: kbtest must print the Linux keycode
for 'x' (45) going down then up.

QMP send-key routes only to the virtio keyboard; the serial console
(which drives the shell) is a completely separate input path.

Run from the repo root after `make build && make fs.img`:
    python3 scripts/test-input.py
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
KEY_X = 45  # Linux KEY_X


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
            "-device", "virtio-keyboard-device,bus=virtio-mmio-bus.2",
            "-qmp", f"unix:{qmp_path},server,nowait",
            "-global", "virtio-mmio.force-legacy=false",
            "-drive", "file=fs.img,if=none,format=raw,id=x0",
            "-device", "virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0",
        ])
        os._exit(127)

    buf = bytearray()
    stage = 0
    injected = False
    deadline = time.time() + 30

    def text():
        return buf.decode("utf-8", "replace")

    def qmp_send_key(key):
        sk = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        sk.connect(qmp_path)
        f = sk.makefile("rwb")
        f.readline()
        f.write(b'{"execute":"qmp_capabilities"}\n')
        f.flush()
        f.readline()
        f.write(json.dumps({
            "execute": "send-key",
            "arguments": {"keys": [{"type": "qcode", "data": key}]},
        }).encode() + b"\n")
        f.flush()
        f.readline()
        sk.close()

    try:
        while time.time() < deadline:
            r, _, _ = select.select([fd], [], [], 0.3)
            if r:
                try:
                    buf += os.read(fd, 4096)
                except OSError:
                    break
            s = text()
            if stage == 0 and "\n$" in s:
                stage = 1
                os.write(fd, b"kbtest\n")
            elif stage == 1 and "kbtest: waiting for keys" in s:
                stage = 2
                time.sleep(0.3)
                qmp_send_key("x")
                injected = True
            elif stage == 2 and "kbtest: done" in s:
                break

        out = text()
        down_ok = f"key {KEY_X} down" in out
        up_ok = f"key {KEY_X} up" in out
        done_ok = "kbtest: done" in out
        print("virtio_input ready :", "virtio_input: keyboard ready" in out)
        print("key injected       :", injected)
        print(f"KEY_X({KEY_X}) down      :", down_ok)
        print(f"KEY_X({KEY_X}) up        :", up_ok)
        print("clean exit         :", done_ok)
        ok = down_ok and up_ok and done_ok
        if not ok:
            print("serial tail:")
            print("\n".join(out.splitlines()[-10:]))
        print("RESULT             :", "PASS" if ok else "FAIL")
        return 0 if ok else 1
    finally:
        try:
            os.kill(pid, signal.SIGKILL)
            os.waitpid(pid, 0)
        except OSError:
            pass


if __name__ == "__main__":
    sys.exit(main())
