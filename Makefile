# Top-level Makefile for the Rust xv6-riscv port.
# Mirrors upstream xv6's `make qemu` UX.

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

.PHONY: build qemu clean fmt

build:
	cargo build $(CARGO_FLAGS) -p kernel

qemu: build
	$(QEMU) $(QEMUOPTS)

qemu-gdb: build
	$(QEMU) $(QEMUOPTS) -S -s

clean:
	cargo clean

fmt:
	cargo fmt
