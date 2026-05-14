# Top-level Makefile for the Rust xv6-riscv port.

CPUS ?= 1
PROFILE ?= release
QEMU = qemu-system-riscv64

ifeq ($(PROFILE),release)
CARGO_FLAGS = --release
TARGET_SUBDIR = release
else
CARGO_FLAGS =
TARGET_SUBDIR = debug
endif

KERNEL = target/riscv64gc-unknown-none-elf/$(TARGET_SUBDIR)/kernel

QEMUOPTS  = -machine virt -bios none -kernel $(KERNEL)
QEMUOPTS += -m 128M -smp $(CPUS) -nographic
QEMUOPTS += -global virtio-mmio.force-legacy=false
QEMUOPTS += -drive file=fs.img,if=none,format=raw,id=x0
QEMUOPTS += -device virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0

.PHONY: build qemu qemu-gdb clean fmt fs.img

build:
	cargo build $(CARGO_FLAGS) -p kernel

# fs.img is a 1 MiB blob whose block 0 starts with a known banner. The
# rest is zeroed. Filesystem layout will replace this in Phase 6+.
fs.img:
	@dd if=/dev/zero of=$@ bs=1024 count=1024 status=none
	@printf 'BLOCK0_HELLO_FROM_DISK\n' | dd of=$@ bs=1 conv=notrunc status=none

qemu: build fs.img
	$(QEMU) $(QEMUOPTS)

qemu-gdb: build fs.img
	$(QEMU) $(QEMUOPTS) -S -s

clean:
	cargo clean
	rm -f fs.img

fmt:
	cargo fmt
