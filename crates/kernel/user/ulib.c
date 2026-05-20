// Ported from xv6 user/ulib.c. Kept verbatim except for:
//   * the `start` wrapper isn't needed (our ulib.S `_start` already
//     calls main + exit)
//   * `sbrklazy` is omitted until the kernel-side lazy-sbrk path
//     lands (G5)
//   * sbrk() is the syscall wrapper from ulib.S, not the C wrapper
//     from xv6's ulib.c, so we don't redefine it here

#include "user.h"

char*
strcpy(char *s, const char *t)
{
    char *os = s;
    while ((*s++ = *t++) != 0)
        ;
    return os;
}

int
strcmp(const char *p, const char *q)
{
    while (*p && *p == *q) { p++; q++; }
    return (uchar)*p - (uchar)*q;
}

uint
strlen(const char *s)
{
    int n;
    for (n = 0; s[n]; n++)
        ;
    return n;
}

void*
memset(void *dst, int c, uint n)
{
    char *cdst = (char*)dst;
    for (uint i = 0; i < n; i++) cdst[i] = (char)c;
    return dst;
}

char*
strchr(const char *s, char c)
{
    for (; *s; s++)
        if (*s == c) return (char*)s;
    return 0;
}

char*
gets(char *buf, int max)
{
    int i, cc;
    char c;
    for (i = 0; i + 1 < max; ) {
        cc = read(0, &c, 1);
        if (cc < 1) break;
        buf[i++] = c;
        if (c == '\n' || c == '\r') break;
    }
    buf[i] = '\0';
    return buf;
}

int
stat(const char *n, struct stat *st)
{
    int fd = open(n, O_RDONLY);
    if (fd < 0) return -1;
    int r = fstat(fd, st);
    close(fd);
    return r;
}

int
atoi(const char *s)
{
    int n = 0;
    while ('0' <= *s && *s <= '9')
        n = n * 10 + *s++ - '0';
    return n;
}

void*
memmove(void *vdst, const void *vsrc, int n)
{
    char *dst = vdst;
    const char *src = vsrc;
    if (src > (const char*)dst) {
        while (n-- > 0) *dst++ = *src++;
    } else {
        dst += n;
        src += n;
        while (n-- > 0) *--dst = *--src;
    }
    return vdst;
}

int
memcmp(const void *s1, const void *s2, uint n)
{
    const char *p1 = s1, *p2 = s2;
    while (n-- > 0) {
        if (*p1 != *p2) return *p1 - *p2;
        p1++; p2++;
    }
    return 0;
}

void*
memcpy(void *dst, const void *src, uint n)
{
    return memmove(dst, src, n);
}
