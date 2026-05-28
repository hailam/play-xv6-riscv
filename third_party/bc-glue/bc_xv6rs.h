// bc glue header — prepended to every bc/dc source file via `-include`
// when cross-compiling for xv6-rs against picolibc.
//
// bc's error-recovery machinery uses sigsetjmp/siglongjmp/sigjmp_buf
// throughout. picolibc supplies plain setjmp/longjmp but not the
// signal-mask variants (we have no POSIX signals). bc has the same
// fallback for Windows; we use it for xv6-rs too.

#ifndef BC_XV6RS_GLUE_H
#define BC_XV6RS_GLUE_H

#include <setjmp.h>

#define sigjmp_buf jmp_buf
#define sigsetjmp(j, s) setjmp(j)
#define siglongjmp(j, v) longjmp(j, v)

#endif
