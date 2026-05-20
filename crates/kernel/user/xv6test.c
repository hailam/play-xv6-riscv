// xv6test — exercise the ported xv6 user C runtime (printf, strcpy,
// strcmp, strlen, atoi, memmove, memset).

#include "user.h"

int main(int argc, char** argv) {
    printf("xv6test: argc=%d\n", argc);
    for (int i = 0; i < argc; i++) {
        printf("  argv[%d] = %s (len=%d)\n", i, argv[i], strlen(argv[i]));
    }

    char buf[32];
    strcpy(buf, "hello");
    printf("xv6test: strcpy -> %s (len=%d)\n", buf, strlen(buf));

    int n = atoi("12345");
    printf("xv6test: atoi(\"12345\") -> %d (hex %x)\n", n, n);

    int eq = strcmp("foo", "foo");
    int lt = strcmp("foo", "fop");
    int gt = strcmp("fop", "foo");
    printf("xv6test: strcmp eq=%d lt=%d gt=%d\n", eq, lt, gt);

    memset(buf, '*', 8);
    buf[8] = 0;
    printf("xv6test: memset -> %s\n", buf);

    char src[] = "memmovetest";
    char dst[16];
    memmove(dst, src, sizeof(src));
    printf("xv6test: memmove -> %s\n", dst);

    printf("xv6test: pointer width = %d bytes, %%p of fn = %p\n",
           (int)sizeof(void*), (void*)main);

    printf("xv6test: ok\n");
    return 0;
}
