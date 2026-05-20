//
// AArch64 VBAR_EL1 vector table + kernel-side trap entry/exit.
//
// 16 slots × 128 bytes = 2 KiB. Must be 2 KiB aligned. Each slot
// gets 128 bytes (32 insns); we just `b` to a shared handler.
//
// Slot map:
//   0x000  Current EL, SP_EL0, Sync    -> bad (we run on SP_EL1)
//   0x080  Current EL, SP_EL0, IRQ     -> bad
//   0x100  Current EL, SP_EL0, FIQ     -> bad
//   0x180  Current EL, SP_EL0, SError  -> bad
//   0x200  Current EL, SP_ELx, Sync    -> kernel_sync_entry
//   0x280  Current EL, SP_ELx, IRQ     -> kernel_irq_entry
//   0x300  Current EL, SP_ELx, FIQ     -> bad
//   0x380  Current EL, SP_ELx, SError  -> bad
//   0x400  Lower EL AArch64, Sync      -> user_sync_entry (Phase E)
//   0x480  Lower EL AArch64, IRQ       -> user_irq_entry  (Phase E)
//   0x500  Lower EL AArch64, FIQ       -> bad
//   0x580  Lower EL AArch64, SError    -> bad
//   0x600  Lower EL AArch32, Sync      -> bad
//   0x680  Lower EL AArch32, IRQ       -> bad
//   0x700  Lower EL AArch32, FIQ       -> bad
//   0x780  Lower EL AArch32, SError    -> bad
//
// Kernel IRQ handler saves caller-saved regs (x0..x18, x30) + ELR +
// SPSR onto SP_EL1, calls `rust_kerneltrap_irq` in Rust, restores,
// `eret`s.

.section .text
.balign 2048
.global kernel_vector_table
kernel_vector_table:
.org 0x000
        b       bad_trap
.org 0x080
        b       bad_trap
.org 0x100
        b       bad_trap
.org 0x180
        b       bad_trap
.org 0x200
        b       kernel_sync_entry
.org 0x280
        b       kernel_irq_entry
.org 0x300
        b       bad_trap
.org 0x380
        b       bad_trap
.org 0x400
        b       user_sync_entry
.org 0x480
        b       user_irq_entry
.org 0x500
        b       bad_trap
.org 0x580
        b       bad_trap
.org 0x600
        b       bad_trap
.org 0x680
        b       bad_trap
.org 0x700
        b       bad_trap
.org 0x780
        b       bad_trap

.org 0x800

// ----- Caller-saved register save/restore macros -----
//
// Saves x0..x18 (19 regs), x30 (LR), ELR_EL1, SPSR_EL1 — total
// 22 × 8 = 176 bytes. Round up to 192 for 16-byte SP alignment.

.macro KERNEL_SAVE
        sub     sp, sp, #192
        stp     x0,  x1,  [sp, #0]
        stp     x2,  x3,  [sp, #16]
        stp     x4,  x5,  [sp, #32]
        stp     x6,  x7,  [sp, #48]
        stp     x8,  x9,  [sp, #64]
        stp     x10, x11, [sp, #80]
        stp     x12, x13, [sp, #96]
        stp     x14, x15, [sp, #112]
        stp     x16, x17, [sp, #128]
        str     x18,      [sp, #144]
        str     x30,      [sp, #152]
        mrs     x0, elr_el1
        mrs     x1, spsr_el1
        stp     x0,  x1,  [sp, #160]
.endm

.macro KERNEL_RESTORE
        ldp     x0,  x1,  [sp, #160]
        msr     elr_el1, x0
        msr     spsr_el1, x1
        ldr     x30,      [sp, #152]
        ldr     x18,      [sp, #144]
        ldp     x16, x17, [sp, #128]
        ldp     x14, x15, [sp, #112]
        ldp     x12, x13, [sp, #96]
        ldp     x10, x11, [sp, #80]
        ldp     x8,  x9,  [sp, #64]
        ldp     x6,  x7,  [sp, #48]
        ldp     x4,  x5,  [sp, #32]
        ldp     x2,  x3,  [sp, #16]
        ldp     x0,  x1,  [sp, #0]
        add     sp, sp, #192
.endm

// ----- handlers -----

kernel_sync_entry:
        KERNEL_SAVE
        bl      rust_kerneltrap_sync
        // unreachable — rust_kerneltrap_sync panics
        b       .

kernel_irq_entry:
        KERNEL_SAVE
        bl      rust_kerneltrap_irq
        KERNEL_RESTORE
        eret

user_sync_entry:
        // Phase E will route this through the trampoline. For now,
        // we should never get here (no user mode active) — panic.
        b       bad_trap

user_irq_entry:
        b       bad_trap

bad_trap:
        // Unknown / unsupported trap. Land in Rust so we get a
        // useful panic message instead of a hang.
        b       rust_bad_trap
