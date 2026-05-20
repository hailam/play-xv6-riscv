//
// Kernel entry on aarch64. QEMU virt with `-bios none` enters us at
// EL2 by default on cortex-a72; we may also land at EL1 if the
// machine was launched with `el2=off`. Detect via CurrentEL and
// branch accordingly. After the drop, jump to Rust `kmain`.
//
// Layout (16 KiB stack per hart, 8 harts max — matches riscv64):
//   _stack0: [16384 × 8] bytes in .bss, .balign 16.
//

.section .text.entry
.global _entry
_entry:
        // QEMU's bootloader stub leaves: x0 = DTB phys, x1..x3 = 0.
        // Preserve x0 across the EL drop.
        mov     x19, x0

        // Per-hart kernel stack from MPIDR_EL1.Aff0.
        //   sp = &_stack0 + (aff0 + 1) * 16 KiB
        mrs     x1, mpidr_el1
        and     x1, x1, #0xff           // aff0
        add     x1, x1, #1
        lsl     x1, x1, #14             // × 16 KiB
        adrp    x2, _stack0
        add     x2, x2, :lo12:_stack0
        add     x2, x2, x1

        // Dispatch on CurrentEL — bits [3:2]: 0b10 = EL2, 0b01 = EL1.
        mrs     x3, CurrentEL
        cmp     x3, #(2 << 2)
        b.eq    drop_to_el1

        // ---- Already at EL1: just install sp and jump to Rust ----
        mov     sp, x2
        mov     x0, x19
        b       kmain

drop_to_el1:
        // SCTLR_EL1: canonical RES1 mask with M/C/I cleared.
        // RES1 bits per ARM ARM D13.2.118: 11, 20, 22, 23, 28, 29.
        ldr     x4, =0x30C50830
        msr     sctlr_el1, x4

        // HCR_EL2: only RW=1 (EL1 is AArch64). Everything else 0.
        mov     x4, #1
        lsl     x4, x4, #31
        msr     hcr_el2, x4

        // CNTHCTL_EL2 bits [1:0] = EL1PCEN | EL1PCTEN
        //   → EL1 can program & read the virtual / physical timer.
        mov     x4, #3
        msr     cnthctl_el2, x4
        msr     cntvoff_el2, xzr

        // CPACR_EL1.FPEN = 11 → don't trap FP/SIMD at EL0/1.
        mov     x4, #(3 << 20)
        msr     cpacr_el1, x4

        // SPSR_EL2 = 0x3C5 → eret to EL1h, AArch64, DAIF=1111
        //   M[3:0]=0101 EL1h, M[4]=0 AArch64, D=A=I=F=1.
        mov     x4, #0x3C5
        msr     spsr_el2, x4

        // ELR_EL2 = first Rust function (kmain). adr uses PC-relative
        // addressing within ±1 MiB, which is fine — kmain lives in
        // the same image.
        adr     x4, kmain
        msr     elr_el2, x4

        // SP_EL1 = per-hart stack we computed in x2.
        msr     sp_el1, x2

        // Restore x0 so kmain sees the original boot register.
        mov     x0, x19
        eret

.section .bss
.balign 16
.global _stack0
_stack0:
        .skip   16384 * 8
