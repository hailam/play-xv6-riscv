#
# Kernel entry. QEMU jumps here in M-mode on every hart. We set up a
# per-hart stack and call into Rust `mstart`, which does M-mode setup
# and `mret`s to `kmain` in S-mode.
#

.attribute arch, "rv64gc"

.section .text.entry
.global _entry
_entry:
        la      sp, _stack0
        csrr    a0, mhartid
        addi    t1, a0, 1
        slli    t0, t1, 14
        add     sp, sp, t0
        mv      tp, a0
        call    mstart
1:      wfi
        j       1b

.section .bss
.balign 16
.global _stack0
_stack0:
        .skip 16384 * 8
