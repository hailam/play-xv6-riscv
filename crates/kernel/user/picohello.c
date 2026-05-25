// First program linked against picolibc rather than our minimal ulib
// printf. Verifies that <stdio.h>/printf/vfprintf reach our kernel
// through the POSIX-console glue (read/write/lseek/_exit) exported
// from ulib.S.

#include <stdio.h>

int main(int argc, char* argv[]) {
    (void)argv;
    printf("hello from picolibc, argc=%d\n", argc);
    return 0;
}
