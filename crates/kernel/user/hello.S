#
# hello — second distinct embedded binary.
#

.attribute arch, "rv64gc"

.section .text
.global _start
_start:
        li      a0, 1
        la      a1, msg
        li      a2, 17
        li      a7, 16
        ecall

        li      a0, 0
        li      a7, 2
        ecall

1:      j       1b

msg:
        .ascii "hello: greetings\n"
