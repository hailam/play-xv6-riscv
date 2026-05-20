//
// AArch64 trampoline. Lives in .trampsec — a one-page section mapped
// at fixed VA TRAMPOLINE in both kernel and user pagetables (PtePerm
// RX, no PTE_U). Hardware reads VBAR_EL1 on every trap; while user
// mode is running, VBAR_EL1 points here (TRAMPOLINE).
//
// What's in this page:
//   * trampoline_vector_table   — 16-slot VBAR table (2 KiB)
//   * uservec_sync / uservec_irq — entry handlers for user-mode traps
//   * bad_trampoline_trap        — panic landing for unexpected slots
//   * trampoline_userret         — kernel-callable user-mode entry
//
// All addresses inside this file are PC-relative; everything fits in
// one 4 KiB page.
//
// TrapFrame field offsets (must match hal-aarch64::trapframe::TrapFrame):
//     0  kernel_satp     (kernel TTBR0 to restore on entry)
//     8  kernel_sp       (per-hart kernel stack top)
//    16  kernel_trap     (rust_usertrap kernel-VA function pointer)
//    24  kernel_hartid
//    32  elr_el1         (user PC = "epc")
//    40  sp_el0          (user SP)
//    48  spsr_el1        (saved PSTATE)
//    56  x0
//    64  x1
//    ...
//   296  x30
//
// The user-vec dance follows the RISC-V xv6 model:
//   1. Stash user x0 in TPIDRRO_EL0 (kernel writes, EL0 only reads).
//   2. mrs x0, tpidr_el1 — TRAPFRAME VA (kernel writes before each
//      return-to-user).
//   3. Save x1..x30, SP_EL0, ELR_EL1, SPSR_EL1, and the stashed x0
//      into TRAPFRAME (mapped in user PT).
//   4. Load kernel TTBR0 + SP + rust_usertrap address from TRAPFRAME.
//   5. msr ttbr0_el1, kernel_satp; TLB flush; isb.
//   6. Swap VBAR_EL1 back to the kernel-mode vector table (its VA is
//      now reachable since we're on kernel PT). br to rust_usertrap.

.section .trampsec, "ax", @progbits
.balign 2048
.global trampoline_vector_table
trampoline_vector_table:
.org 0x000
        b       bad_trampoline_trap
.org 0x080
        b       bad_trampoline_trap
.org 0x100
        b       bad_trampoline_trap
.org 0x180
        b       bad_trampoline_trap
.org 0x200
        b       bad_trampoline_trap
.org 0x280
        b       bad_trampoline_trap
.org 0x300
        b       bad_trampoline_trap
.org 0x380
        b       bad_trampoline_trap
.org 0x400
        b       uservec_sync
.org 0x480
        b       uservec_irq
.org 0x500
        b       bad_trampoline_trap
.org 0x580
        b       bad_trampoline_trap
.org 0x600
        b       bad_trampoline_trap
.org 0x680
        b       bad_trampoline_trap
.org 0x700
        b       bad_trampoline_trap
.org 0x780
        b       bad_trampoline_trap

.org 0x800

.global uservec_sync
uservec_sync:
        // Stash user x0 in TPIDRRO_EL0 (read-only at EL0; kernel
        // writes via msr). We could also use TPIDR_EL0 but that's
        // user TLS in many ABIs.
        msr     tpidrro_el0, x0

        // x0 = TRAPFRAME VA (kernel saved here before eret).
        mrs     x0, tpidr_el1

        // Save x1..x30 at offsets 64..296.
        stp     x1,  x2,  [x0, #64]
        stp     x3,  x4,  [x0, #80]
        stp     x5,  x6,  [x0, #96]
        stp     x7,  x8,  [x0, #112]
        stp     x9,  x10, [x0, #128]
        stp     x11, x12, [x0, #144]
        stp     x13, x14, [x0, #160]
        stp     x15, x16, [x0, #176]
        stp     x17, x18, [x0, #192]
        stp     x19, x20, [x0, #208]
        stp     x21, x22, [x0, #224]
        stp     x23, x24, [x0, #240]
        stp     x25, x26, [x0, #256]
        stp     x27, x28, [x0, #272]
        stp     x29, x30, [x0, #288]

        // Save original x0 (stashed in tpidrro_el0) at offset 56.
        mrs     x1, tpidrro_el0
        str     x1, [x0, #56]

        // Save SP_EL0, ELR_EL1, SPSR_EL1.
        mrs     x1, sp_el0
        str     x1, [x0, #40]
        mrs     x1, elr_el1
        str     x1, [x0, #32]
        mrs     x1, spsr_el1
        str     x1, [x0, #48]

        // Load kernel state from trapframe.
        ldr     x1, [x0, #0]              // kernel_satp = TTBR0
        ldr     x2, [x0, #8]              // kernel_sp
        ldr     x3, [x0, #16]             // kernel_trap (rust_usertrap)

        // Switch to kernel pagetable. The next instruction fetch still
        // works because the trampoline page is mapped in both PTs.
        msr     ttbr0_el1, x1
        dsb     ish
        tlbi    vmalle1is
        dsb     ish
        isb

        // VBAR_EL1 gets swapped to the kernel-mode vector table by
        // `Arch::on_user_trap_entry` (called at the top of
        // `rust_usertrap`). Doing it here would require an external
        // symbol reference that the linker can't resolve PC-relative
        // (the trampoline page sits at a high user VA at runtime).

        // Switch to the kernel stack and call into Rust.
        mov     sp, x2
        br      x3

.global uservec_irq
uservec_irq:
        // For now route to the same Rust dispatcher — decode_user_trap
        // checks ESR_EL1 and figures out what to do.
        b       uservec_sync

.global bad_trampoline_trap
bad_trampoline_trap:
        // Spin loudly. Means VBAR_EL1 was still pointing at us during a
        // kernel-mode trap, which shouldn't happen.
1:      wfi
        b       1b

// trampoline_userret(tf_va, user_satp) — noreturn.
//   x0 = TRAPFRAME VA (the constant TRAPFRAME, mapped in both PTs)
//   x1 = user satp (= TTBR0 value)
//
// Mirror of uservec_sync. Called by Hal::return_to_user from kernel
// context with VBAR_EL1 still on kernel_vector_table.
.global trampoline_userret
trampoline_userret:
        // Stash TRAPFRAME VA in TPIDR_EL1 — uservec_sync will pull
        // it back out on the next trap.
        msr     tpidr_el1, x0

        // Swap VBAR_EL1 to the trampoline vector table. Its VA is
        // TRAMPOLINE (= adr to ourselves). Reachable in both kernel
        // and user PTs.
        adr     x2, trampoline_vector_table
        msr     vbar_el1, x2

        // Read out ELR_EL1, SPSR_EL1, SP_EL0 BEFORE we lose access
        // to trapframe-via-kernel-PT... actually trapframe is at
        // user VA TRAPFRAME, which the kernel PT does NOT map. We
        // must switch TTBR0 first. The trapframe IS mapped in user
        // PT, so post-swap is fine.

        // Switch TTBR0_EL1 to user.
        msr     ttbr0_el1, x1
        dsb     ish
        tlbi    vmalle1is
        dsb     ish
        isb

        // Restore CSRs from trapframe (user PT now active).
        ldr     x2, [x0, #32]
        msr     elr_el1, x2
        ldr     x2, [x0, #48]
        msr     spsr_el1, x2
        ldr     x2, [x0, #40]
        msr     sp_el0, x2

        // Restore user x1..x30 from trapframe.
        ldp     x29, x30, [x0, #288]
        ldp     x27, x28, [x0, #272]
        ldp     x25, x26, [x0, #256]
        ldp     x23, x24, [x0, #240]
        ldp     x21, x22, [x0, #224]
        ldp     x19, x20, [x0, #208]
        ldp     x17, x18, [x0, #192]
        ldp     x15, x16, [x0, #176]
        ldp     x13, x14, [x0, #160]
        ldp     x11, x12, [x0, #144]
        ldp     x9,  x10, [x0, #128]
        ldp     x7,  x8,  [x0, #112]
        ldp     x5,  x6,  [x0, #96]
        ldp     x3,  x4,  [x0, #80]
        ldp     x1,  x2,  [x0, #64]
        // Restore x0 last (we lose our base register).
        ldr     x0, [x0, #56]

        eret
