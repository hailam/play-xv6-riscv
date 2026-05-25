// Stress-test picolibc against our POSIX-console glue.
//
// Exercises beyond the trivial `printf("hello")` of picohello:
//   - varied printf conversions (%d %x %s %p %lld %hd %hhu %c)
//   - malloc + free of multiple sizes (validates sbrk path)
//   - fopen / fread / fclose on /README (validates open/lseek/close)
//   - fprintf to stdout via FILE*
// Anything that fails here usually means a missing or wrong glue
// stub in ulib.S — picolibc itself is fine.

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int main(void) {
    printf("== picotest ==\n");

    // ---- printf conversions ----
    printf("int=%d  hex=0x%x  ptr=%p\n", -42, 0xdeadbeef, (void*)0x1234);
    printf("long=%lld  short=%hd  uchar=%hhu  char=%c\n",
           (long long)-1234567890123LL, (short)-12345, (unsigned char)200, 'Z');
    printf("str=\"%s\"  width=[%10d]  prec=[%.3s]\n",
           "hello", 7, "abcdef");

    // ---- malloc / free ----
    void* a = malloc(16);
    void* b = malloc(4096);
    void* c = malloc(1);
    if (!a || !b || !c) {
        printf("FAIL: malloc returned NULL (a=%p b=%p c=%p)\n", a, b, c);
        return 1;
    }
    memset(a, 0xAA, 16);
    memset(b, 0x55, 4096);
    *(char*)c = 7;
    printf("malloc: a=%p b=%p c=%p  *c=%d\n", a, b, c, *(unsigned char*)c);
    free(a); free(b); free(c);

    // ---- fopen / fread / fclose ----
    FILE* f = fopen("/README", "r");
    if (!f) {
        printf("FAIL: fopen(/README) returned NULL\n");
        return 1;
    }
    char buf[64];
    size_t n = fread(buf, 1, sizeof(buf) - 1, f);
    buf[n] = '\0';
    fclose(f);
    printf("fread(%zu bytes): \"%.40s...\"\n", n, buf);

    fprintf(stdout, "fprintf-to-stdout works\n");

    printf("== picotest OK ==\n");
    return 0;
}
