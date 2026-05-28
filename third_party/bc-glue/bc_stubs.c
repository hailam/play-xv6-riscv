// Glue stubs for picolibc symbols our ulib doesn't expose:
//
//   _exit    — picolibc's abort() and raise() call _exit; our
//              syscall stub is named exit. Forward.
//   isatty   — bc uses isatty for TTY mode detection. We don't
//              expose a tty syscall yet; report "not a tty" so bc
//              picks the non-interactive code path.

extern void exit(int code) __attribute__((noreturn));

__attribute__((noreturn))
void _exit(int code) {
    exit(code);
}

int isatty(int fd) {
    (void)fd;
    return 0;
}
