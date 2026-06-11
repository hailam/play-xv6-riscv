#!/usr/bin/env python3
"""
Headless checks for the framebuffer (todo 12).

Default ("kernel"): the kernel draws its boot test pattern (four
quadrants red/green/blue/white). Verifies the fw_cfg/ramfb scanout
path (M1).

"user": runs /fbtest from the guest shell, which opens /dev/fb0,
queries geometry via ioctl, and paints a DISTINCT pattern (magenta
field with a yellow band through the middle). Verifies the device-file
write path (M2) — i.e. a userspace process reached the screen.

Either way: boot with `-device ramfb` + a QMP socket, let the guest
draw, `screendump` the scanout to PPM, check pixels. No display
backend or image library needed.

    python3 scripts/test-fb.py [kernel|user]
Exit 0 on success, 1 on failure.
"""
import json
import os
import socket
import subprocess
import sys
import tempfile
import time

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
KERNEL = "target/riscv64gc-unknown-none-elf/release/kernel"

MODE = sys.argv[1] if len(sys.argv) > 1 else "kernel"

# Quadrant centres for the kernel boot pattern.
KERNEL_EXPECT = [
    (160, 120, (0xFF, 0x00, 0x00)),   # top-left  red
    (480, 120, (0x00, 0xFF, 0x00)),   # top-right green
    (160, 360, (0x00, 0x00, 0xFF)),   # bot-left  blue
    (480, 360, (0xFF, 0xFF, 0xFF)),   # bot-right white
]
# fbtest paints magenta everywhere except a yellow band at rows
# [height/2-20, height/2+20). Sample a band row and a field row.
USER_EXPECT = [
    (320, 240, (0xFF, 0xFF, 0x00)),   # centre → yellow band
    (320, 100, (0xFF, 0x00, 0xFF)),   # upper  → magenta field
    (320, 380, (0xFF, 0x00, 0xFF)),   # lower  → magenta field
]


def read_ppm(path):
    with open(path, "rb") as f:
        data = f.read()
    assert data[:2] == b"P6", "not a P6 PPM"
    idx = 2
    fields = []
    while len(fields) < 3:
        while data[idx] in b" \t\n\r":
            idx += 1
        start = idx
        while data[idx] not in b" \t\n\r":
            idx += 1
        fields.append(int(data[start:idx]))
    idx += 1
    w, h, _maxval = fields
    return w, h, data[idx:]


def main():
    os.chdir(REPO)
    tmp = tempfile.mkdtemp()
    qmp_path = os.path.join(tmp, "qmp.sock")
    ppm_path = os.path.join(tmp, "fb.ppm")
    serial_path = os.path.join(tmp, "serial.in")
    # A FIFO-less approach: drive the guest shell via a pty.
    import pty
    pid, fd = pty.fork()
    if pid == 0:
        os.execvp("qemu-system-riscv64", [
            "qemu-system-riscv64",
            "-machine", "virt", "-bios", "none", "-kernel", KERNEL,
            "-m", "256M", "-smp", "1", "-nographic",
            "-device", "ramfb",
            "-qmp", f"unix:{qmp_path},server,nowait",
            "-global", "virtio-mmio.force-legacy=false",
            "-drive", "file=fs.img,if=none,format=raw,id=x0",
            "-device", "virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0",
        ])
        os._exit(127)

    import select
    buf = bytearray()
    booted = False
    ran = False
    drew = (MODE != "user")  # kernel mode: pattern is already drawn
    deadline = time.time() + 25
    expect = KERNEL_EXPECT if MODE == "kernel" else USER_EXPECT

    def text():
        return buf.decode("utf-8", "replace")

    try:
        # In user mode, drive the shell to run /fbtest.
        while time.time() < deadline and not drew:
            r, _, _ = select.select([fd], [], [], 0.3)
            if r:
                try:
                    buf += os.read(fd, 4096)
                except OSError:
                    break
            s = text()
            if not booted and "\n$" in s:
                booted = True
                os.write(fd, b"fbtest\n")
                ran = True
            elif ran and "fbtest: drew" in s:
                drew = True
        if MODE == "user" and not drew:
            print("FAIL: fbtest didn't report drawing; serial tail:")
            print("\n".join(text().splitlines()[-8:]))
            return 1
        if MODE == "kernel":
            time.sleep(2.5)  # let the kernel boot + draw

        # Screendump via QMP.
        sk = None
        qdl = time.time() + 8
        while time.time() < qdl:
            try:
                sk = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
                sk.connect(qmp_path)
                break
            except OSError:
                sk = None
                time.sleep(0.2)
        if sk is None:
            print("FAIL: no QMP socket")
            return 1
        qf = sk.makefile("rwb")
        qf.readline()
        qf.write(b'{"execute":"qmp_capabilities"}\n')
        qf.flush()
        qf.readline()
        qf.write(json.dumps(
            {"execute": "screendump", "arguments": {"filename": ppm_path}}
        ).encode() + b"\n")
        qf.flush()
        ok = False
        for _ in range(20):
            line = qf.readline()
            if not line:
                break
            msg = json.loads(line)
            if "return" in msg:
                ok = True
                break
            if "error" in msg:
                print("FAIL: screendump error:", msg["error"])
                return 1
        if not ok:
            print("FAIL: no screendump return")
            return 1

        w, h, pix = read_ppm(ppm_path)
        print(f"mode={MODE} screendump: {w}x{h}")
        all_ok = True
        for (x, y, want) in expect:
            off = (y * w + x) * 3
            got = (pix[off], pix[off + 1], pix[off + 2])
            m = got == want
            all_ok &= m
            print(f"  ({x:>3},{y:>3}) want {want} got {got} "
                  f"{'OK' if m else 'MISMATCH'}")
        print("RESULT:", "PASS" if all_ok else "FAIL")
        return 0 if all_ok else 1
    finally:
        import signal
        try:
            os.kill(pid, signal.SIGKILL)
            os.waitpid(pid, 0)
        except OSError:
            pass


if __name__ == "__main__":
    sys.exit(main())
