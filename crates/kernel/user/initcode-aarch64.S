//
// initcode for aarch64 — first user process.
//
// Prints a hello banner via sys_write so we can verify that the
// trampoline + svc + sys_write actually deliver data to the
// console, then attempts to exec("/sh", 0). If exec fails (no
// aarch64-native /sh on disk yet), exits with -1.
//
// AArch64 ABI: syscall # in x8; args x0..x7; return x0; svc #0.

.section .text.entry, "ax"
.global _start
_start:
        // sys_write(fd=1, buf=msg, len=msg_len)
        mov     x0, #1
        adr     x1, msg
        mov     x2, #(msg_end - msg)
        mov     x8, #16              // SYS_WRITE
        svc     #0

        // exec("/sh", 0)
        adr     x0, sh_path
        mov     x1, #0
        mov     x8, #7               // SYS_EXEC
        svc     #0

        // If exec returned (failure), exit(-1).
        mov     x0, #-1
        mov     x8, #2               // SYS_EXIT
        svc     #0
1:      b       1b

.section .rodata, "a"
msg:
        .ascii  "hello from aarch64 init (svc dispatched)\n"
msg_end:
sh_path:
        .asciz  "/sh"
