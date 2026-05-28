// Shared glue between picolibc and ulib for user programs.
//
// Picolibc's posix-console build assumes a few thin POSIX
// primitives that our ulib syscall stubs don't expose under the
// exact name picolibc expects. Rather than patch ulib (which is
// shared with non-picolibc programs), we ship a tiny adapter.
//
//   _exit   — picolibc's abort() and raise() call _exit. ulib
//             exposes the syscall as exit; forward.
//   isatty  — bc + lua check this to decide interactive prompts.
//             We don't have a tty syscall yet; report "no" so the
//             non-interactive code path runs.
//   times   — picolibc's clock() calls times(2). We zero the
//             returned struct so clock() returns 0 — Lua's
//             os.clock() will measure 0 seconds, which is a
//             benign loss of precision compared to crashing.

extern void exit(int code) __attribute__((noreturn));

__attribute__((noreturn))
void _exit(int code) {
    exit(code);
}

int isatty(int fd) {
    (void)fd;
    return 0;
}

// Mirrors struct tms from <sys/times.h>. We don't include the
// header — picolibc may or may not ship it.
struct tms_stub {
    long tms_utime;
    long tms_stime;
    long tms_cutime;
    long tms_cstime;
};

long times(struct tms_stub* buf) {
    if (buf) {
        buf->tms_utime = 0;
        buf->tms_stime = 0;
        buf->tms_cutime = 0;
        buf->tms_cstime = 0;
    }
    return 0;
}
