#
# Minimal user-mode initcode: write a banner to fd 1 then exit.
# Linked at VA 0 (user pagetable maps VA 0 -> this page).
#

.attribute arch, "rv64gc"

.section .text
.global _start
_start:
        # write(1, msg, 21)
        li      a0, 1
        la      a1, msg
        li      a2, 21
        li      a7, 16              # SYS_WRITE
        ecall

        # exit(0)
        li      a0, 0
        li      a7, 2               # SYS_EXIT
        ecall

        # exit should not return; spin defensively just in case.
1:      j       1b

msg:
        .ascii "hello from user mode\n"
