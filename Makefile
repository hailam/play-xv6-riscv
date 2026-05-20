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

.PHONY: build mkfs qemu qemu-gdb clean fmt

build:
	cargo build $(CARGO_FLAGS) -p kernel

mkfs: $(MKFS)

$(MKFS):
	cargo build --release -p mkfs --target=$(HOST)

# fs.img is built by the mkfs host tool, populated with the user
# binaries kernel build.rs copies to `target/user/<name>.elf`.
fs.img: build $(MKFS)
	$(MKFS) $@ \
		init:$(USER_DIR)/initcode.elf \
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
		smptest:$(USER_DIR)/smptest.elf

qemu: build fs.img
	$(QEMU) $(QEMUOPTS)

qemu-gdb: build fs.img
	$(QEMU) $(QEMUOPTS) -S -s

clean:
	cargo clean
	rm -f fs.img

fmt:
	cargo fmt
