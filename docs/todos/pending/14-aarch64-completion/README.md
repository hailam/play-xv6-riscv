# 14: aarch64 — boot to a shell

**Status:** Phase A done; Phases B–E remain.
**Estimated:** 3–4 focused sessions + cross-toolchain install.
**Depends on:** [[27-aarch64-hal-skeleton]] (DONE).
**Unblocks:** `qemu-system-aarch64 -M virt -cpu cortex-a72` running the shell.

This README contains the full research (deep enough to implement
each phase without going back to the docs) plus the per-phase
verification gates. Citations use the short forms:

- **ARM ARM** = "ARM Architecture Reference Manual for A-profile",
  DDI 0487.
- **GICv2** = "ARM Generic Interrupt Controller, GICv2", IHI 0048B.
- **virt.c** = `qemu/hw/arm/virt.c`.
- **boot.c** = `qemu/hw/arm/boot.c`.

---

## What's already done (Phase A)

- Trait widening: `PGSIZE / KERNBASE / PHYSTOP / TRAMPOLINE /
  TRAPFRAME / TIMER_INTERVAL / UART0 / VIRTIO0 / INTC_BASE /
  UART0_IRQ / VIRTIO0_IRQ` are all `Hal` consts. AArch64 impl
  provides real values.
- `TrapFrameAccess` trait: `epc/sp/arg/syscall_nr/set_*`. AArch64
  impl uses `ELR_EL1/SP_EL0/x0..x7/x8`.
- **`cargo build --target aarch64-unknown-none-softfloat -p kernel`
  succeeds** (1 expected linker warning — `_start` not defined yet).
- `Hal` trait exposes everything the kernel needs:
  - `decode_user_trap(tf) -> UserTrapCause`
  - `arm_timer`, `handle_external_irq`, `init_kernel_trap_vec`,
    `on_user_trap_entry`
  - `return_to_user(tf, user_satp) -> !` (noreturn — handles all
    arch-specific CSR setup + trampoline jump)
  - `init_console`, `init_intc_global`, `init_intc_per_hart`
  - `console_try_getc`
  - `install_free_frame`
- Kernel scrubbed: all arch-specific imports gone except in
  `arch.rs` (the selector, correct).
- AArch64 impls have **real values for the trait consts** +
  `unimplemented!()`-style stubs for the runtime methods (filled
  in by Phases B–E).

---

## Phase B — first `kmain_aarch64` print — **DONE**

Verified:

```
$ qemu-system-aarch64 -machine virt -cpu cortex-a72 \
    -kernel target/aarch64-unknown-none-softfloat/release/kernel \
    -m 128M -smp 1 -display none -serial stdio -monitor none
rust kmain (hart 0, supervisor)
kalloc: 32341 free frames (126 MiB)
kvm: installed (satp=0x47fff000)
PANIC: ... virtio_blk: bad version 1     <- Phase D territory
```

What landed:

- `crates/hal-aarch64/asm/entry.S` (~75 LoC) — full EL2→EL1 drop
  with `CurrentEL` dispatch + per-hart-stack from MPIDR_EL1.Aff0
  + `eret` into `kmain`.
- `crates/hal-aarch64/src/start.rs` (~10 LoC) — `global_asm!`
  includes the entry asm.
- `crates/hal-aarch64/src/uart.rs` — real PL011 init (baud,
  LCRH 8N1+FIFO, RXIM, CR.UARTEN|TXE|RXE) + `try_getc` reading
  PL011_DR after checking PL011_FR.RXFE.
- `crates/kernel/kernel-aarch64.ld` — linker script anchored at
  `0x4008_0000` (DRAM + 1 MiB, matches QEMU's direct-kernel
  boot convention).
- `.cargo/config.toml` — aarch64 rustflags pointing at the new
  linker script.
- `crates/kernel/src/main.rs` — added `extern crate hal_aarch64
  as _` so the entry asm gets linked in. Print phrase changed
  from "S-mode" to neutral "supervisor".

The kalloc count differs from riscv64 (32341 vs 32448) because of
the aarch64-specific BSS + stack region layout. Free count is
correct otherwise.

Note QEMU aarch64 doesn't accept `-bios none` (treats "none" as
a ROM filename); just leave the flag off. `-kernel` already
implies no firmware.

### B.1 Boot environment facts (QEMU virt + cortex-a72)

- **Default entry EL = EL2** (cortex-a72 implements EL2; QEMU's
  `arm_setup_direct_kernel_boot` in boot.c picks `target_el = 2`
  when EL2 is implemented). Use `-cpu cortex-a72,el2=off` if you
  want entry at EL1 — but `entry.S` must handle both.
- **Kernel load:** DRAM base `0x4000_0000`; `arm_setup_direct_kernel_boot`
  honours ELF `p_paddr`. Conventional layout puts the image at
  `0x4008_0000` (DRAM + 0x80000, matches Linux). Our `memlayout.rs`
  already has `KERNBASE = 0x4000_0000`; link `.text` at
  `0x4008_0000`.
- **Register state on entry** (boot.c stub): `x0 = DTB phys`,
  `x1..x3 = 0`. Save `x0` immediately if we ever want DTB.
- **Hartid:** `MPIDR_EL1.Aff0` (bits [7:0]). On virt, single
  cluster, Aff1..3 = 0.
- **SMP:** QEMU virt uses PSCI for secondary release; secondaries
  are powered off until hart 0 calls `CPU_ON`. Defer SMP to
  Phase F (this todo's Phase B brings up hart 0 only).
- **Memory map** (virt.c `base_memmap`, already in our
  `memlayout.rs`):

  | Device | Base | Size |
  |---|---|---|
  | GIC distributor (GICD) | `0x0800_0000` | 64 KiB |
  | GIC CPU interface (GICC) | `0x0801_0000` | 64 KiB |
  | PL011 UART0 | `0x0900_0000` | 4 KiB |
  | virtio-mmio slot 0 | `0x0A00_0000` | 0x200 |
  | DRAM | `0x4000_0000` | up to 255 GiB |

- **IRQ numbers** (virt.c `a15irqmap`; INTID = SPI number + 32):

  | Source | INTID | Type |
  |---|---|---|
  | UART0 (PL011) | **33** | SPI |
  | virtio-mmio slot 0 | **48** | SPI |
  | Virtual EL1 timer | **27** | PPI 11 |
  | Non-secure EL1 physical timer | 30 | PPI 14 |

  Already wired in `memlayout.rs`. Comment "GIC SPI 1 (== PPI/SPI
  base 32 + 1)" is slightly off — fix to "GIC SPI INTID 33 (= SPI
  base 32 + relative ID 1)".

### B.2 EL2 → EL1 drop sequence (entry.S)

Required register writes, in order:

1. **Dispatch on `CurrentEL`** ([3:2] = `0b10` EL2, `0b01` EL1).
   QEMU may give us either depending on `-cpu` flags.
2. **SCTLR_EL1** = `0x30C50830` (canonical "all RES1 set, M/C/I
   clear" — RES1 bits 11/20/22/23/28/29 per ARM ARM D13.2.118).
3. **HCR_EL2.RW = 1** (bit 31). Everything else 0 →
   `HCR_EL2 = 0x8000_0000`.
4. **CNTHCTL_EL2** = `3` (bits [1:0] = EL1PCEN | EL1PCTEN —
   let EL1 access virtual+physical counters/timers).
5. **CNTVOFF_EL2 = 0** (no virtual-counter offset).
6. **CPACR_EL1** = `(3 << 20)` (FPEN = 11, don't trap FP).
7. **SPSR_EL2 = 0x3C5** — `0011_1100_0101`:
   - `[3:0] M = 0101` → EL1h (use SP_EL1)
   - `[4]   M = 0`    → AArch64
   - `[6]   F = 1` (FIQ masked)
   - `[7]   I = 1` (IRQ masked)
   - `[8]   A = 1` (SError masked)
   - `[9]   D = 1` (Debug masked)
8. **ELR_EL2 = `kmain_aarch64`** (Rust entry).
9. **SP_EL1** = per-hart stack (use MPIDR_EL1.Aff0 to index the
   `_stack0` array; 16 KiB per hart matching riscv layout).
10. `eret`.

### B.3 `entry.S` skeleton

```asm
.section .text.entry
.global _entry
_entry:
        mov     x19, x0                  // preserve DTB ptr

        // Per-hart stack from MPIDR_EL1.Aff0
        mrs     x1, mpidr_el1
        and     x1, x1, #0xff
        add     x1, x1, #1
        lsl     x1, x1, #14              // 16 KiB
        adrp    x2, _stack0
        add     x2, x2, :lo12:_stack0
        add     x2, x2, x1

        // Dispatch on CurrentEL
        mrs     x3, CurrentEL
        cmp     x3, #(2 << 2)
        b.eq    1f
        // ---- already at EL1 ----
        mov     sp, x2
        b       kmain_aarch64

1:      // ---- at EL2: prepare to drop ----
        ldr     x4, =0x30C50830
        msr     sctlr_el1, x4
        mov     x4, #(1 << 31)
        msr     hcr_el2, x4
        mov     x4, #3
        msr     cnthctl_el2, x4
        msr     cntvoff_el2, xzr
        mov     x4, #(3 << 20)
        msr     cpacr_el1, x4
        mov     x4, #0x3C5
        msr     spsr_el2, x4
        adr     x4, kmain_aarch64
        msr     elr_el2, x4
        msr     sp_el1, x2
        mov     x0, x19
        eret

.section .bss
.balign 16
.global _stack0
_stack0:
        .skip   16384 * 8                // 16 KiB × 8 harts
```

### B.4 PL011 real init (replace the no-op `uart::init`)

```rust
// PL011 reg offsets, base = UART0 = 0x0900_0000.
const PL011_DR:    usize = UART0 + 0x000;
const PL011_FR:    usize = UART0 + 0x018;
const PL011_IBRD:  usize = UART0 + 0x024;
const PL011_FBRD:  usize = UART0 + 0x028;
const PL011_LCRH:  usize = UART0 + 0x02C;
const PL011_CR:    usize = UART0 + 0x030;
const PL011_IMSC:  usize = UART0 + 0x038;
const PL011_ICR:   usize = UART0 + 0x044;

pub unsafe fn init() {
    // 1) Disable.
    write_u32(PL011_CR, 0);
    // 2) Clear pending interrupts.
    write_u32(PL011_ICR, 0x7FF);
    // 3) Baud: 115200 @ 24 MHz UARTCLK → divider 13.020833.
    //    IBRD = 13, FBRD = 1 (= 0.020833 * 64 ≈ 1)
    write_u32(PL011_IBRD, 13);
    write_u32(PL011_FBRD, 1);
    // 4) Line control: 8N1 + FIFO enable.
    write_u32(PL011_LCRH, (1 << 4) | (3 << 5));   // FEN | WLEN=11(8)
    // 5) Enable RX interrupt (RXIM bit 4).
    write_u32(PL011_IMSC, 1 << 4);
    // 6) Enable UART (CR.UARTEN=1, TXE=1, RXE=1).
    write_u32(PL011_CR, (1 << 9) | (1 << 8) | (1 << 0));
}
```

The existing `putc` (spin on TXFF) is already correct; add a
matching `try_getc` reading PL011_DR after checking PL011_FR.RXFE.

---

## Phase C — pagetable populate + MMU enable  (~250 LoC, ~1 session)

Gate: `kvm: installed (ttbr0=...)` prints, kernel continues
through subsequent prints. Validate `(SCTLR_EL1 & 1) == 1`.

### C.1 4 K granule, 48-bit VA, single TTBR0

48-bit VA splits as `L0[47:39] | L1[38:30] | L2[29:21] |
L3[20:12] | offset[11:0]`. 4 levels × 512 entries × 8 bytes = 4
KiB per table.

```rust
fn idx(va: usize, level: u32) -> usize {
    let shift = 12 + 9 * (3 - level);   // L0=39, L1=30, L2=21, L3=12
    (va >> shift) & 0x1FF
}
```

### C.2 Descriptor types (ARM ARM D8.3)

`desc[1:0]`:
- `0b00` / any with bit 0 = 0 → **invalid**.
- `0b01` at L0/L1/L2 → **block** (huge: 1 GiB L1, 2 MiB L2; L0
  blocks not legal at 4 K granule).
- `0b11` at L0/L1/L2 → **table** (next-level PA in bits [47:12]).
- `0b11` at L3 → **page** (4 KiB leaf).
- `0b01` at L3 → reserved / invalid.

**For us:** every leaf at L3 is `0b11`; every interior node is
`0b11`; no blocks. Same as Sv39's 4 KiB-only behaviour.

### C.3 L3 page descriptor bit layout

| Bits | Field | Meaning |
|---|---|---|
| [1:0] | type | `0b11` |
| [4:2] | **AttrIndx[2:0]** | index into MAIR_EL1 (0..7) |
| [5] | NS | non-secure (0 for us at EL1NS) |
| [7:6] | **AP[2:1]** | access perm (see below) |
| [9:8] | **SH[1:0]** | shareability (`11` = Inner Shareable for RAM) |
| [10] | **AF** | Access Flag — **must be 1** or first access faults |
| [11] | **nG** | non-Global (1 for user mappings) |
| [47:12] | **OA** | output PA (PPN) |
| [52] | Contiguous | TLB-coalesce hint; leave 0 |
| [53] | **PXN** | Privileged eXecute Never (1 = no exec at EL1) |
| [54] | **UXN** | Unprivileged eXecute Never (1 = no exec at EL0) |

### C.4 AP[2:1] encoding (ARM ARM D8.4.3 Table D8-37)

| AP[2:1] | EL1 | EL0 |
|---|---|---|
| `00` | R/W | none |
| `01` | R/W | R/W |
| `10` | R | none |
| `11` | R | R |

Map our `PtePerm`:

| Want | AP | PXN | UXN |
|---|---|---|---|
| Kernel R/W (data) | `00` | 1 | 1 |
| Kernel R/X (text, trampoline) | `00` | 0 | 1 |
| User R/W (data, stack, heap) | `01` | 1 | 1 |
| User R/X (code) | `01` | 1 | 0 |
| User RO | `11` | 1 | per perm |

Note ARMv8 always grants EL1 ≥ EL0 access — there is no
"user-only" page perm. xv6's `copyin`/`copyout` already handle
this in software via `translate_user_perm`.

### C.5 MAIR_EL1

Two slots are enough:

| Attr | Use | Byte | Encoding |
|---|---|---|---|
| Attr0 | MMIO (UART, GIC, virtio) | `0x00` | Device-nGnRnE |
| Attr1 | Normal RAM | `0xFF` | WB-WA inner+outer |

`MAIR_EL1 = (0xFF << 8) | 0x00 = 0xFF00`.

### C.6 TCR_EL1 — single TTBR0, 4 K, 48-bit

```rust
const TCR_EL1_VAL: u64 =
      (16  << 0)               // T0SZ = 16  → 48-bit VA
    | (0b01 << 8) | (0b01 << 10)  // IRGN0/ORGN0 = WB-RW-Alloc
    | (0b11 << 12)             // SH0 = Inner Shareable
    | (0b00 << 14)             // TG0 = 4 KB
    | (1   << 23)              // EPD1 = 1 (disable TTBR1 walks)
    | (0b010 << 32);           // IPS = 40-bit PA (ample for 128 MiB)
```

(IPS values: `000`=32-bit, `001`=36, `010`=40, `100`=44, `101`=48.
cortex-a72 has 44-bit PA; 40 is enough for our 128 MiB heap.)

### C.7 SCTLR_EL1 — enable MMU + caches

Pre-enable: RES1 bits already set in entry.S. To turn on the MMU:

```asm
        // (after loading TTBR0/TCR/MAIR)
        dsb     ish
        tlbi    vmalle1is
        dsb     ish
        isb
        mrs     x0, sctlr_el1
        orr     x0, x0, #(1 << 0)     // M
        orr     x0, x0, #(1 << 2)     // C
        orr     x0, x0, #(1 << 12)    // I
        msr     sctlr_el1, x0
        isb                            // commit: now translating
```

### C.8 TTBR0_EL1 write format

| Bits | Field | Value |
|---|---|---|
| [0] | CnP | 0 single-core / 1 SMP common |
| [47:1] | BADDR | root PA, bits [47:1] (4 KiB aligned) |
| [63:48] | ASID | 0 for kernel; non-zero per process if using ASIDs |

For Phase C, just `TTBR0_EL1 = root_pa`. ASIDs come later (cheap
TLB invalidation by ASID).

### C.9 TLB management cheatsheet

| Insn | Effect | When |
|---|---|---|
| `tlbi vmalle1is` | Invalidate all stage-1 EL1, IS broadcast | TTBR0 switch / large mapping change |
| `tlbi vaale1is, Xn` | Invalidate VA `(Xn >> 12) << 12` at any ASID | Single-page unmap |
| `tlbi aside1is, Xn` | Invalidate all entries for ASID `Xn >> 48` | ASID retirement |

Always wrap: `dsb ishst` → write PTE → `dsb ish` → `tlbi …` →
`dsb ish` → `isb`.

### C.10 Cache maintenance for trampoline writes

ARM ARM B2.4.5: after kernel writes into a page that EL0 will
execute, need to clean D-cache + invalidate I-cache by VA at PoU:

```asm
        dc      cvau, x0
        dsb     ish
        ic      ivau, x0
        dsb     ish
        isb
```

---

## Phase D — vector table + GIC v2 + ARM timer  (~270 LoC, ~1 session)

Gate: timer ticks visible (kernel's `kalloc.free=` log printed
every few seconds, or `/usertests stacktest`'s deliberate fault
gets caught and the kernel survives).

### D.1 VBAR_EL1 vector table

16 slots × 128 bytes = 2 KiB. Must be 2 KiB aligned.

| Offset | Entry | Reachable for us? |
|---|---|---|
| `0x000..0x180` | Current EL, SP_EL0 (×4) | no — we use SP_EL1 |
| **`0x200`** | Current EL, SP_ELx, Sync | yes — kernel faults |
| **`0x280`** | Current EL, SP_ELx, IRQ | yes — kernel IRQs (timer) |
| `0x300` | Current EL, SP_ELx, FIQ | no |
| `0x380` | Current EL, SP_ELx, SError | yes — panic |
| **`0x400`** | Lower EL AArch64, Sync | yes — syscalls, EL0 faults |
| **`0x480`** | Lower EL AArch64, IRQ | yes — IRQs while in user |
| `0x500..0x780` | Lower EL AArch32 / FIQ / SError | no (panic or skip) |

Each slot is 128 bytes (32 instructions). Either branch to a
shared handler (1 ins, 31 wasted) or inline the regs-save
preamble (fits comfortably).

```asm
.balign 2048
.global vector_table
vector_table:
.org 0x200
        b   kernel_sync_handler
.org 0x280
        b   kernel_irq_handler
.org 0x380
        b   panic_unhandled
.org 0x400
        b   uservec                  // → trampoline.S
.org 0x480
        b   user_irq_handler
.org 0x780
```

### D.2 ESR_EL1 layout

`esr[31:26] = EC` (exception class), `[25] IL`, `[24:0] ISS`.

EC values worth handling:

| EC (hex) | From | Meaning |
|---|---|---|
| **0x15** | EL0 | SVC AArch64 — syscall |
| 0x18 | EL0 | MSR/MRS trap |
| **0x20** | EL0→EL1 | Instruction abort (user code page fault) |
| 0x21 | EL1→EL1 | Instruction abort same EL → kernel bug, panic |
| **0x24** | EL0→EL1 | Data abort (user data page fault) |
| 0x25 | EL1→EL1 | Data abort same EL → kernel bug, panic |
| 0x26 | any | SP alignment |
| 0x2F | any | SError |
| 0x3C | any | BRK |

For aborts (0x20/0x21/0x24/0x25), **`FAR_EL1`** holds the
faulting VA. ISS[5:0] is the IFSC/DFSC code; `0x04..0x07` =
translation fault at L0..L3 (the common "page not present" case).

### D.3 `decode_user_trap` impl

```rust
fn decode_user_trap(tf: &mut TrapFrame) -> UserTrapCause {
    // Save PC before any further trap reads it.
    let elr = csr::read_elr_el1();
    tf.set_epc(elr as u64);

    let esr = csr::read_esr_el1();
    let ec = (esr >> 26) & 0x3F;
    let far = csr::read_far_el1();

    match ec {
        0x15 => UserTrapCause::Syscall,  // ELR_EL1 already past the SVC
        0x20 => UserTrapCause::PageFault { va: far as usize, write: false },
        0x24 => UserTrapCause::PageFault {
            va: far as usize,
            write: (esr & (1 << 6)) != 0,    // ISS[6] = WnR
        },
        _ => UserTrapCause::Unknown { code: esr, va: far as usize },
    }
}
```

(For IRQs, the trap handler enters via VBAR+0x480 — a different
vector slot — and signals `Devintr` directly without calling
decode_user_trap. So decode only sees synchronous causes.)

### D.4 GIC v2 driver

**Distributor (GICD) offsets:**

| Reg | Offset | Notes |
|---|---|---|
| `GICD_CTLR` | 0x000 | bit 0 = EnableGrp0 |
| `GICD_ISENABLER(n)` | 0x100 + 4n | 1-write to enable; banked for PPIs |
| `GICD_ICENABLER(n)` | 0x180 + 4n | 1-write to disable |
| `GICD_IPRIORITYR(n)` | 0x400 + n | 8 bits/IRQ (lower = higher) |
| `GICD_ITARGETSR(n)` | 0x800 + n | 8-bit CPU mask (SPIs only) |
| `GICD_ICFGR(n)` | 0xC00 + 4n | 2 bits/IRQ; 0=level, 2=edge |
| `GICD_SGIR` | 0xF00 | SGI dispatch |

**CPU interface (GICC) offsets:**

| Reg | Offset | Notes |
|---|---|---|
| `GICC_CTLR` | 0x000 | bit 0 = enable |
| `GICC_PMR` | 0x004 | priority mask (0xFF = pass all) |
| `GICC_BPR` | 0x008 | preemption mask |
| `GICC_IAR` | 0x00C | RO; reading claims top pending |
| `GICC_EOIR` | 0x010 | WO; signal end-of-interrupt |

**Global init (hart 0):**
```rust
unsafe fn init_intc_global() {
    write_u32(GICD + 0x000, 0);              // disable

    for spi in [UART0_IRQ, VIRTIO0_IRQ] {
        write_u8(GICD + 0x400 + spi, 0xA0);  // mid priority
        write_u8(GICD + 0x800 + spi, 0x01);  // target hart 0
    }

    write_u32(GICD + 0x000, 1);              // enable group 0
}
```

**Per-hart init:**
```rust
unsafe fn init_intc_per_hart() {
    write_u32(GICC + 0x004, 0xFF);           // PMR open
    write_u32(GICC + 0x008, 0);              // BPR
    write_u32(GICC + 0x000, 1);              // enable CPU interface

    enable_irq(UART0_IRQ);                   // 33
    enable_irq(VIRTIO0_IRQ);                 // 48
    enable_irq(27);                          // virtual timer PPI
}

fn enable_irq(id: usize) {
    let reg = GICD + 0x100 + (id / 32) * 4;
    let bit = 1u32 << (id % 32);
    unsafe { write_u32(reg, bit); }
}
```

**Claim / complete:**
```rust
fn claim() -> u32 { unsafe { read_u32(GICC + 0x00C) } }    // 1023 = spurious
fn complete(intid: u32) { unsafe { write_u32(GICC + 0x010, intid); } }
```

### D.5 ARM generic timer

```rust
unsafe fn arm_timer() {
    let interval = csr::read_cntfrq_el0() / 100;     // 10 ms tick
    csr::write_cntv_tval_el0(interval);
    csr::write_cntv_ctl_el0(1);                       // ENABLE=1
}
```

To disarm (e.g. inside timer ISR before doing kernel work):

```rust
csr::write_cntv_tval_el0(i64::MAX);   // push next fire far out
```

IRQ delivery: virtual timer PPI INTID 27. Per-hart banked in
`GICD_ISENABLER0`. Don't forget to enable it in
`init_intc_per_hart`.

---

## Phase E — trampoline + EL0↔EL1 + cross-toolchain  (~200 LoC + toolchain, ~1 session)

Gate: same shell session as riscv64 — `qemu-system-aarch64 -M
virt -cpu cortex-a72 -bios none -kernel ... -drive file=fs.img,...`
runs `/ls /`, `/echo`, `/cat`, `/usertests exitwait`.

### E.1 Single-TTBR0 model (xv6/riscv style)

Unlike Linux (which uses TTBR0 for low/user, TTBR1 for high/kernel),
our xv6 layout uses **TTBR0_EL1 only**. `EPD1=1` in TCR_EL1
disables TTBR1 walks entirely. Switching to user = write user
root to TTBR0_EL1. Switching back = write kernel root to TTBR0_EL1.

Layout (from `memlayout.rs`):
- Kernel VAs: low half, e.g. `0x4000_0000+` (identity-mapped
  physmem during boot).
- User VAs: low addresses (0..MAXVA, but only what the ELF and
  sbrk maps).
- `TRAMPOLINE` and `TRAPFRAME` live near MAXVA (matching riscv).

### E.2 TTBR0 switch (trampoline-resident)

```asm
        msr     ttbr0_el1, x1            // x1 = new root PA
        tlbi    vmalle1is
        dsb     ish
        isb
```

Adding ASIDs later (TCR.A1=0, ASID in TTBR0[63:48]) lets us swap
`tlbi vmalle1is` for `tlbi aside1is, X` — much cheaper.

### E.3 SPSR_EL1 for ERET to EL0

```
SPSR_EL1 fields (for EL0t, AArch64, IRQs unmasked):
  M[3:0] = 0  (EL0t)
  M[4]   = 0  (AArch64)
  F = 0, I = 0, A = 0, D = 0    → SPSR_EL1 = 0x0
```

For IRQs initially masked (re-enable later): `SPSR_EL1 = 0x80`.

### E.4 TrapFrame layout

Match `crates/hal-aarch64/src/trapframe.rs`:

| Offset | Field |
|---|---|
| 0 | kernel_satp (kernel TTBR0) |
| 8 | kernel_sp |
| 16 | kernel_trap (rust_usertrap addr) |
| 24 | kernel_hartid |
| 32 | elr_el1 (== epc) |
| 40 | sp_el0 (user sp) |
| 48 | spsr_el1 |
| 56 | x[0..30] — 31 × 8 = 248 bytes |

Total = 304 bytes. Add a `const _: () =
assert!(core::mem::offset_of!(TrapFrame, x) == 56)` compile-time
check (mandatory).

### E.5 `uservec` (trampoline.S — user → kernel)

ARM lacks RISC-V's `sscratch`. Trick: use `TPIDR_EL1` (kernel
TLS-like CSR we own at EL1) to hold the per-hart TRAPFRAME VA;
use `TPIDRRO_EL0` (read-only at EL0; kernel still writes via
`msr`) as a single scratch register.

```asm
.section .trampsec
.balign 4
.global uservec
uservec:
        msr     tpidrro_el0, x0          // stash x0
        mrs     x0, tpidr_el1            // x0 = TRAPFRAME VA

        // Save x1..x30 at offset 56 + n*8
        str     x1,  [x0, #(56 + 1*8)]
        str     x2,  [x0, #(56 + 2*8)]
        // ... x3..x30 ...
        str     x30, [x0, #(56 + 30*8)]

        // Save user SP and original x0
        mrs     x1, sp_el0
        str     x1, [x0, #40]
        mrs     x1, tpidrro_el0
        str     x1, [x0, #(56 + 0*8)]

        // Save ELR_EL1, SPSR_EL1
        mrs     x1, elr_el1
        str     x1, [x0, #32]
        mrs     x1, spsr_el1
        str     x1, [x0, #48]

        // Load kernel state from trapframe
        ldr     x1, [x0, #0]             // kernel_satp
        ldr     x2, [x0, #8]             // kernel_sp
        ldr     x3, [x0, #16]            // kernel_trap

        // Switch to kernel pagetable
        msr     ttbr0_el1, x1
        tlbi    vmalle1is
        dsb     ish
        isb

        // Switch SP, jump to Rust
        mov     sp, x2
        br      x3
```

### E.6 `userret` (kernel → user)

Mirror image:

```asm
.global userret
userret:
        // x0 = TRAPFRAME VA (passed in)
        // x1 = user_satp

        // Save TRAPFRAME VA in TPIDR_EL1 for next uservec
        msr     tpidr_el1, x0

        // Switch to user pagetable
        msr     ttbr0_el1, x1
        tlbi    vmalle1is
        dsb     ish
        isb

        // Restore ELR_EL1, SPSR_EL1
        ldr     x1, [x0, #32]
        msr     elr_el1, x1
        ldr     x1, [x0, #48]
        msr     spsr_el1, x1

        // Restore user SP
        ldr     x1, [x0, #40]
        msr     sp_el0, x1

        // Restore x1..x30
        ldr     x1,  [x0, #(56 + 1*8)]
        // ... x2..x30 ...

        // Finally restore x0 (our base reg)
        ldr     x0, [x0, #(56 + 0*8)]

        eret
```

### E.7 Cross-toolchain

**Recommended: Homebrew `aarch64-elf-gcc`**

```fish
brew install aarch64-elf-gcc aarch64-elf-binutils aarch64-elf-newlib
```

Provides `aarch64-elf-{gcc,ld,objcopy}`. Bare-metal target — fine
for `-nostdlib`.

User-binary compile flags:
```
aarch64-elf-gcc -march=armv8-a -mgeneral-regs-only \
                -nostdlib -nostartfiles -static -fno-pie -fno-pic \
                -ffreestanding -O2 -Wall \
                -T user/user.ld user/<prog>.c -o target/user/aarch64/<prog>.elf
```

`-mgeneral-regs-only` skips FP/SIMD lazy save (we're soft-float).

### E.8 build.rs dispatch

```rust
let cc = match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
    Ok("riscv64") => "riscv64-elf-gcc",
    Ok("aarch64") => "aarch64-elf-gcc",
    _ => panic!("unsupported target arch"),
};
let user_out = format!("target/user/{arch}");
```

---

## Phase F — aarch64 SMP + IPI (optional, defer)  (~100 LoC)

- PSCI `CPU_ON` to release secondaries.
- GIC SGI for cross-CPU wakeups (write GICD_SGIR with target list
  + INTID 0..15).
- The riscv64 path also defers real IPI to timer-tick polling;
  both arches can land this together as a separate todo.

PSCI call convention (SMCCC):
```
HVC #0   ; or SMC #0 depending on EL of caller
  x0 = function ID (0xC400_0003 for CPU_ON)
  x1 = target affinity (MPIDR-style)
  x2 = entry PA
  x3 = context ID
```

GIC SGI:
```rust
fn sgi_all_except_self(intid: u32) {
    let val = (0b01u32 << 24) | (intid & 0xF);
    unsafe { write_u32(GICD + 0xF00, val); }
}
```

---

## Verification gate (whole-todo done)

Same shell session as our riscv64 build:

```
$ /ls /
$ /echo hello on aarch64
$ /mkdir /work
$ /cd /work
$ /wr greet hi
$ /cat greet
$ /usertests exitwait
```

Output identical (modulo hartid in the exit log) under
`qemu-system-aarch64 -M virt -cpu cortex-a72 -nographic -bios
none -kernel target/aarch64-unknown-none-softfloat/release/kernel
-drive file=fs.img,...`.

---

## Risks

- **GICv2 vs v3.** QEMU virt defaults to v2 with `-cpu cortex-a72`;
  pin that.
- **Cache invalidation.** ARMv8 is stricter than RISC-V about
  I-cache vs D-cache coherency. After installing the trampoline
  page, may need `dc cvau` + `dsb ish` + `ic ivau` + `dsb ish` +
  `isb` before EL0 fetches from it.
- **EL2 entry assumptions.** QEMU virt boots at EL2 by default
  on cortex-a72, but `-cpu cortex-a72,el2=off` would land us at
  EL1 directly. `entry.S` dispatches on `CurrentEL`.
- **Trapframe field offsets vs trampoline.S.** Just like the
  riscv side, asm offsets must exactly match the `repr(C)` struct.
  Compile-time assertions in `trapframe.rs` are mandatory.
- **The `TPIDR_EL1` / `TPIDRRO_EL0` trick** assumes the kernel
  never uses `TPIDRRO_EL0` for user TLS. If we later add ARM TLS
  (Linux uses `TPIDR_EL0` for that), we're fine. If we want to
  use `TPIDRRO_EL0` for read-only user TLS, switch the scratch
  to a memory location.

---

## Code touch points

- New: `crates/hal-aarch64/asm/{entry.S, kernelvec.S, trampoline.S}`.
- New: `crates/hal-aarch64/src/{start.rs, trap.rs, gic.rs}`.
- Fill stubs in `crates/hal-aarch64/src/{lib.rs, csr.rs,
  pagetable.rs, trapframe.rs, uart.rs}`.
- `crates/kernel/build.rs` — multi-arch user-toolchain dispatch.
- `Makefile` — new `qemu-aarch64` target.

---

## References

- ARM ARM DDI 0487 — <https://developer.arm.com/documentation/ddi0487/latest>
- GICv2 spec IHI 0048B — <https://developer.arm.com/documentation/ihi0048/b/>
- QEMU virt machine docs — <https://www.qemu.org/docs/master/system/arm/virt.html>
- `qemu/hw/arm/virt.c` — <https://github.com/qemu/qemu/blob/master/hw/arm/virt.c>
- `qemu/hw/arm/boot.c` — <https://github.com/qemu/qemu/blob/master/hw/arm/boot.c>
- ARM Trusted Firmware `gic_common.h` — <https://github.com/ARM-software/arm-trusted-firmware/blob/master/include/drivers/arm/gic_common.h>
- AArch64 virtual-memory walkthrough — <https://krinkinmu.github.io/2024/01/14/aarch64-virtual-memory.html>
- TCR_EL1 reference — <https://arm.jonpalmisc.com/latest_sysreg/AArch64-tcr_el1>
- `aarch64-elf-gcc` Homebrew formula — <https://formulae.brew.sh/formula/aarch64-elf-gcc>
- ESR_EL1.EC table — <https://docs.rs/aarch64-cpu/latest/aarch64_cpu/registers/ESR_EL1/EC/index.html>
