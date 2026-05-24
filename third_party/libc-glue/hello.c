// hello.c — first program linked against picolibc.
// Uses picolibc's <stdio.h> printf to verify the libc surface
// reaches our kernel via SYS_WRITE.

#include <stdio.h>

int main(int argc, char* argv[]) {
    printf("hello from picolibc, argc=%d\n", argc);
    return 0;
}
