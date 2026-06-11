# Top-level Makefile for the Rust xv6-riscv port.

CPUS ?= 1
PROFILE ?= release
QEMU = qemu-system-riscv64
HOST := $(shell rustc -vV | sed -n 's/^host: //p')

ifeq ($(PROFILE),release)
CARGO_FLAGS = --release
TARGET_SUBDIR = release
else
CARGO_FLAGS =
TARGET_SUBDIR = debug
endif

KERNEL = target/riscv64gc-unknown-none-elf/$(TARGET_SUBDIR)/kernel
MKFS = target/$(HOST)/release/mkfs
USER_DIR = target/user

QEMUOPTS  = -machine virt -bios none -kernel $(KERNEL)
QEMUOPTS += -m 128M -smp $(CPUS) -nographic
QEMUOPTS += -global virtio-mmio.force-legacy=false
QEMUOPTS += -drive file=fs.img,if=none,format=raw,id=x0
QEMUOPTS += -device virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0
QEMUOPTS += -netdev user,id=n0,hostfwd=tcp:127.0.0.1:17878-:7878
QEMUOPTS += -device virtio-net-device,netdev=n0,bus=virtio-mmio-bus.1
QEMUOPTS += -device virtio-keyboard-device,bus=virtio-mmio-bus.2

.PHONY: build mkfs qemu qemu-gdb test-net test-fb test-input test-gui clean fmt

build:
	cargo build $(CARGO_FLAGS) -p kernel

mkfs: $(MKFS)

$(MKFS):
	cargo build --release -p mkfs --target=$(HOST)

# fs.img is built by the mkfs host tool, populated with the user
# binaries kernel build.rs copies to `target/user/<name>.elf`.
fs.img: build $(MKFS)
	$(MKFS) $@ \
		README:crates/kernel/user/README \
		init:$(USER_DIR)/init.elf \
		tcpecho:$(USER_DIR)/tcpecho.elf \
		fbtest:$(USER_DIR)/fbtest.elf \
		kbtest:$(USER_DIR)/kbtest.elf \
		wm:$(USER_DIR)/wm.elf \
		hello_wm:$(USER_DIR)/hello_wm.elf \
		clock:$(USER_DIR)/clock.elf \
		guidemo:$(USER_DIR)/guidemo.elf \
		echo:$(USER_DIR)/echo.elf \
		sh:$(USER_DIR)/sh.elf \
		cat:$(USER_DIR)/cat.elf \
		hello:$(USER_DIR)/hello.elf \
		pipetest:$(USER_DIR)/pipetest.elf \
		ls:$(USER_DIR)/ls.elf \
		mkdir:$(USER_DIR)/mkdir.elf \
		rm:$(USER_DIR)/rm.elf \
		wr:$(USER_DIR)/wr.elf \
		kill:$(USER_DIR)/kill.elf \
		killtest:$(USER_DIR)/killtest.elf \
		malloctest:$(USER_DIR)/malloctest.elf \
		smptest:$(USER_DIR)/smptest.elf \
		ln:$(USER_DIR)/ln.elf \
		faulttest:$(USER_DIR)/faulttest.elf \
		xv6test:$(USER_DIR)/xv6test.elf \
		lazytest:$(USER_DIR)/lazytest.elf \
		usertests:$(USER_DIR)/usertests.elf \
		seektest:$(USER_DIR)/seektest.elf \
		chmodtest:$(USER_DIR)/chmodtest.elf \
		credtest:$(USER_DIR)/credtest.elf \
		cloexectest:$(USER_DIR)/cloexectest.elf \
		trunctest:$(USER_DIR)/trunctest.elf \
		stattime:$(USER_DIR)/stattime.elf \
		sigtest:$(USER_DIR)/sigtest.elf \
		sigactest:$(USER_DIR)/sigactest.elf \
		sigmasktest:$(USER_DIR)/sigmasktest.elf \
		fdfiletest:$(USER_DIR)/fdfiletest.elf \
		alarmtest:$(USER_DIR)/alarmtest.elf \
		ctimetest:$(USER_DIR)/ctimetest.elf \
		envtest:$(USER_DIR)/envtest.elf \
		posix6test:$(USER_DIR)/posix6test.elf \
		mmaptest:$(USER_DIR)/mmaptest.elf \
		symlinktest:$(USER_DIR)/symlinktest.elf \
		ioctltest:$(USER_DIR)/ioctltest.elf \
		polltest:$(USER_DIR)/polltest.elf \
		pwd:$(USER_DIR)/pwd.elf \
		env:$(USER_DIR)/env.elf \
		picohello:$(USER_DIR)/picohello.elf \
		picotest:$(USER_DIR)/picotest.elf \
		bc:$(USER_DIR)/bc.elf \
		dc:$(USER_DIR)/dc.elf \
		lua:$(USER_DIR)/lua.elf

qemu: build fs.img
	$(QEMU) $(QEMUOPTS)

qemu-gdb: build fs.img
	$(QEMU) $(QEMUOPTS) -S -s

# Host<->guest TCP echo over virtio-net (todo 16 Tier 7 M2). Loopback
# TCP is gated in-guest by the `tcploop` usertest; this covers the
# real-NIC path qemu's SLIRP provides.
test-net: build fs.img
	python3 scripts/test-net.py

# ramfb framebuffer (todo 12): headless screendump checks. `kernel`
# verifies the M1 boot pattern via the fw_cfg/ramfb scanout path;
# `user` runs /fbtest, which draws through /dev/fb0 (M2 device file).
test-fb: build fs.img
	python3 scripts/test-fb.py kernel
	python3 scripts/test-fb.py user

# virtio-input keyboard (todo 12 M3): boot with a virtio keyboard +
# QMP, run /kbtest in the guest, inject a key via QMP send-key and
# verify the evdev events reach userspace through /dev/input/0.
test-input: build fs.img
	python3 scripts/test-input.py

# Display server (todo 12 M4): boot with ramfb + keyboard + QMP, run
# guidemo (wm + hello_wm + clock), screendump to verify both windows
# composite, inject a key and verify it routes to the focused client.
test-gui: build fs.img
	python3 scripts/test-gui.py

# ---- aarch64 ----
AARCH64_TARGET   = aarch64-unknown-none-softfloat
AARCH64_KERNEL   = target/$(AARCH64_TARGET)/$(TARGET_SUBDIR)/kernel
AARCH64_USER_DIR = target/user-aarch64
QEMU_AARCH64     = qemu-system-aarch64
AARCH64_QEMUOPTS  = -machine virt -cpu cortex-a72 -kernel $(AARCH64_KERNEL)
AARCH64_QEMUOPTS += -m 128M -smp $(CPUS) -nographic
AARCH64_QEMUOPTS += -global virtio-mmio.force-legacy=false
AARCH64_QEMUOPTS += -drive file=fs-aarch64.img,if=none,format=raw,id=x0
AARCH64_QEMUOPTS += -device virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0
AARCH64_QEMUOPTS += -netdev user,id=n0,hostfwd=tcp:127.0.0.1:17879-:7878
AARCH64_QEMUOPTS += -device virtio-net-device,netdev=n0,bus=virtio-mmio-bus.1
AARCH64_QEMUOPTS += -device virtio-keyboard-device,bus=virtio-mmio-bus.2

build-aarch64:
	cargo build $(CARGO_FLAGS) -p kernel --target $(AARCH64_TARGET)

fs-aarch64.img: build-aarch64 $(MKFS)
	$(MKFS) $@ \
		README:crates/kernel/user/README \
		init:$(AARCH64_USER_DIR)/init.elf \
		tcpecho:$(AARCH64_USER_DIR)/tcpecho.elf \
		fbtest:$(AARCH64_USER_DIR)/fbtest.elf \
		kbtest:$(AARCH64_USER_DIR)/kbtest.elf \
		wm:$(AARCH64_USER_DIR)/wm.elf \
		hello_wm:$(AARCH64_USER_DIR)/hello_wm.elf \
		clock:$(AARCH64_USER_DIR)/clock.elf \
		guidemo:$(AARCH64_USER_DIR)/guidemo.elf \
		echo:$(AARCH64_USER_DIR)/echo.elf \
		sh:$(AARCH64_USER_DIR)/sh.elf \
		cat:$(AARCH64_USER_DIR)/cat.elf \
		ls:$(AARCH64_USER_DIR)/ls.elf \
		mkdir:$(AARCH64_USER_DIR)/mkdir.elf \
		rm:$(AARCH64_USER_DIR)/rm.elf \
		wr:$(AARCH64_USER_DIR)/wr.elf \
		kill:$(AARCH64_USER_DIR)/kill.elf \
		killtest:$(AARCH64_USER_DIR)/killtest.elf \
		malloctest:$(AARCH64_USER_DIR)/malloctest.elf \
		smptest:$(AARCH64_USER_DIR)/smptest.elf \
		ln:$(AARCH64_USER_DIR)/ln.elf \
		faulttest:$(AARCH64_USER_DIR)/faulttest.elf \
		xv6test:$(AARCH64_USER_DIR)/xv6test.elf \
		lazytest:$(AARCH64_USER_DIR)/lazytest.elf \
		usertests:$(AARCH64_USER_DIR)/usertests.elf \
		seektest:$(AARCH64_USER_DIR)/seektest.elf \
		chmodtest:$(AARCH64_USER_DIR)/chmodtest.elf \
		credtest:$(AARCH64_USER_DIR)/credtest.elf \
		cloexectest:$(AARCH64_USER_DIR)/cloexectest.elf \
		trunctest:$(AARCH64_USER_DIR)/trunctest.elf \
		stattime:$(AARCH64_USER_DIR)/stattime.elf \
		sigtest:$(AARCH64_USER_DIR)/sigtest.elf \
		sigactest:$(AARCH64_USER_DIR)/sigactest.elf \
		sigmasktest:$(AARCH64_USER_DIR)/sigmasktest.elf \
		fdfiletest:$(AARCH64_USER_DIR)/fdfiletest.elf \
		alarmtest:$(AARCH64_USER_DIR)/alarmtest.elf \
		ctimetest:$(AARCH64_USER_DIR)/ctimetest.elf \
		envtest:$(AARCH64_USER_DIR)/envtest.elf \
		posix6test:$(AARCH64_USER_DIR)/posix6test.elf \
		mmaptest:$(AARCH64_USER_DIR)/mmaptest.elf \
		symlinktest:$(AARCH64_USER_DIR)/symlinktest.elf \
		ioctltest:$(AARCH64_USER_DIR)/ioctltest.elf \
		polltest:$(AARCH64_USER_DIR)/polltest.elf \
		pwd:$(AARCH64_USER_DIR)/pwd.elf \
		env:$(AARCH64_USER_DIR)/env.elf \
		picohello:$(AARCH64_USER_DIR)/picohello.elf \
		picotest:$(AARCH64_USER_DIR)/picotest.elf \
		bc:$(AARCH64_USER_DIR)/bc.elf \
		dc:$(AARCH64_USER_DIR)/dc.elf \
		lua:$(AARCH64_USER_DIR)/lua.elf

qemu-aarch64: build-aarch64 fs-aarch64.img
	$(QEMU_AARCH64) $(AARCH64_QEMUOPTS)

qemu-aarch64-gdb: build-aarch64 fs-aarch64.img
	$(QEMU_AARCH64) $(AARCH64_QEMUOPTS) -S -s

clean:
	cargo clean
	rm -f fs.img

fmt:
	cargo fmt
