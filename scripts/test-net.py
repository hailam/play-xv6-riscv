#!/usr/bin/env python3
"""
Host-side check for the virtio-net + smoltcp TCP path (todo 16 Tier 7,
M2). Boots the riscv64 kernel under qemu with a virtio-net device +
SLIRP hostfwd, runs `/tcpecho` in the guest (listens on :7878), then
connects from the host through the forwarded port and verifies a real
round-trip echo over the wire.

Loopback TCP is gated by the in-guest `tcploop` usertest; this covers
the part that needs an actual NIC + host network, which usertests
can't reach. Run from the repo root after `make build && make fs.img`:

    python3 scripts/test-net.py

Exit 0 on success, 1 on failure.
"""
import os
import pty
import select
import signal
import socket
import sys
import time

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
KERNEL = "target/riscv64gc-unknown-none-elf/release/kernel"
HOST_PORT = 17878
PAYLOAD = b"net-hello-42-over-virtio"

QEMU = [
    "qemu-system-riscv64",
    "-machine", "virt", "-bios", "none", "-kernel", KERNEL,
    "-m", "128M", "-smp", "1", "-nographic",
    "-global", "virtio-mmio.force-legacy=false",
    "-drive", "file=fs.img,if=none,format=raw,id=x0",
    "-device", "virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0",
    "-netdev", f"user,id=n0,hostfwd=tcp:127.0.0.1:{HOST_PORT}-:7878",
    "-device", "virtio-net-device,netdev=n0,bus=virtio-mmio-bus.1",
]


def main():
    os.chdir(REPO)
    pid, fd = pty.fork()
    if pid == 0:
        os.execvp(QEMU[0], QEMU)
        os._exit(127)

    buf = bytearray()
    stage = 0
    result = None
    deadline = time.time() + 30

    def text():
        return buf.decode("utf-8", "replace")

    try:
        while time.time() < deadline:
            r, _, _ = select.select([fd], [], [], 0.3)
            if r:
                try:
                    chunk = os.read(fd, 4096)
                except OSError:
                    break
                if not chunk:
                    break
                buf += chunk
            s = text()
            if stage == 0 and "\n$" in s:
                stage = 1
                os.write(fd, b"tcpecho\n")
            elif stage == 1 and "listening on :7878" in s:
                stage = 2
                time.sleep(0.3)
                try:
                    sk = socket.create_connection(
                        ("127.0.0.1", HOST_PORT), timeout=10)
                    sk.sendall(PAYLOAD)
                    sk.settimeout(10)
                    data = b""
                    while len(data) < len(PAYLOAD):
                        ch = sk.recv(128)
                        if not ch:
                            break
                        data += ch
                    result = data == PAYLOAD
                    sk.close()
                except Exception as e:  # noqa: BLE001
                    result = False
                    print(f"host connect/echo failed: {e}")
                stage = 3
            elif stage == 3 and "tcpecho: done" in s:
                break
    finally:
        try:
            os.kill(pid, signal.SIGKILL)
            os.waitpid(pid, 0)
        except OSError:
            pass

    out = text()
    ok = (
        result is True
        and "virtio_net: ready" in out
        and "tcpecho: done" in out
    )
    print("virtio_net ready :", "virtio_net: ready" in out)
    print("echo round-trip  :", result is True)
    print("clean exit       :", "tcpecho: done" in out)
    print("RESULT           :", "PASS" if ok else "FAIL")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
