#!/usr/bin/env python3
"""
Headless check for the ramfb framebuffer driver (todo 12 M1).

Boots the riscv64 kernel under qemu with `-device ramfb` and a QMP
socket, waits for the kernel to draw its boot test pattern (four
quadrants: red / green / blue / white), then asks qemu to `screendump`
the scanout surface to a PPM and verifies the pixel at the centre of
each quadrant. No display backend or image library needed — PPM (P6)
is a trivial binary format.

Run from the repo root after `make build`:
    python3 scripts/test-fb.py
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

# Quadrant centres (the kernel draws 640x480) → expected (R, G, B).
EXPECT = [
    (160, 120, (0xFF, 0x00, 0x00)),   # top-left  red
    (480, 120, (0x00, 0xFF, 0x00)),   # top-right green
    (160, 360, (0x00, 0x00, 0xFF)),   # bot-left  blue
    (480, 360, (0xFF, 0xFF, 0xFF)),   # bot-right white
]


def read_ppm(path):
    with open(path, "rb") as f:
        data = f.read()
    assert data[:2] == b"P6", "not a P6 PPM"
    # Header: P6 <w> <h> <maxval>, whitespace-separated, then one
    # whitespace byte, then raw RGB.
    idx = 2
    fields = []
    while len(fields) < 3:
        # skip whitespace
        while data[idx] in b" \t\n\r":
            idx += 1
        start = idx
        while data[idx] not in b" \t\n\r":
            idx += 1
        fields.append(int(data[start:idx]))
    idx += 1  # single whitespace after maxval
    w, h, _maxval = fields
    return w, h, data[idx:]


def main():
    os.chdir(REPO)
    tmp = tempfile.mkdtemp()
    qmp_path = os.path.join(tmp, "qmp.sock")
    ppm_path = os.path.join(tmp, "fb.ppm")

    qemu = subprocess.Popen(
        [
            "qemu-system-riscv64",
            "-machine", "virt", "-bios", "none", "-kernel", KERNEL,
            "-m", "256M", "-smp", "1",
            "-device", "ramfb",
            "-display", "none",
            "-serial", "file:" + os.path.join(tmp, "serial.log"),
            "-qmp", f"unix:{qmp_path},server,nowait",
            "-global", "virtio-mmio.force-legacy=false",
            "-drive", "file=fs.img,if=none,format=raw,id=x0",
            "-device", "virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0",
        ],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    )
    try:
        # Wait for the QMP socket, then for the kernel to draw.
        sk = None
        deadline = time.time() + 10
        while time.time() < deadline:
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

        f = sk.makefile("rwb")
        f.readline()  # greeting
        f.write(b'{"execute":"qmp_capabilities"}\n')
        f.flush()
        f.readline()

        # Give the guest time to boot + draw the pattern.
        time.sleep(3.0)

        f.write(
            json.dumps(
                {"execute": "screendump", "arguments": {"filename": ppm_path}}
            ).encode()
            + b"\n"
        )
        f.flush()
        # Read responses until we see a non-event return.
        ok = False
        for _ in range(20):
            line = f.readline()
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
        print(f"screendump: {w}x{h}")
        all_ok = True
        for (x, y, want) in EXPECT:
            off = (y * w + x) * 3
            got = (pix[off], pix[off + 1], pix[off + 2])
            match = got == want
            all_ok &= match
            print(f"  ({x:>3},{y:>3}) want {want} got {got} "
                  f"{'OK' if match else 'MISMATCH'}")
        print("RESULT:", "PASS" if all_ok else "FAIL")
        return 0 if all_ok else 1
    finally:
        qemu.kill()
        qemu.wait()


if __name__ == "__main__":
    sys.exit(main())
