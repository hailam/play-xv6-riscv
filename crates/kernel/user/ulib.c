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

// stat() is now a direct syscall (SYS_STAT). Defined in ulib.S /
// ulib-aarch64.S — no open+fstat indirection.

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

// sigaction(2) — user-facing wrapper that auto-fills the restorer
// from ulib's `_sigret` stub. The kernel never sees the C-struct
// shape; we explode `act` into individual syscall args.
extern int  _sys_sigaction(int signum, void* handler,
                           void* restorer, unsigned int mask);
extern void _sigret(void);

// Global pointer to the environment array. _start (in ulib.S /
// ulib-aarch64.S) stores envp from x2/a2 here at process start.
// init starts with environ == NULL (no envp from kernel); getenv
// returns NULL in that case.
char** environ;

// Track whether we own (i.e. have malloced) the current environ
// array. The original from the kernel lives on the user stack and
// must not be free()d. After the first setenv-that-grows we
// allocate a new array and switch.
static int environ_owned = 0;

static int env_match(const char* entry, const char* name) {
    while (*name) {
        if (*entry != *name) return 0;
        entry++;
        name++;
    }
    return *entry == '=';
}

char*
getenv(const char* name)
{
    if (!environ || !name) return 0;
    for (char** p = environ; *p; p++) {
        if (env_match(*p, name)) {
            char* eq = *p;
            while (*eq && *eq != '=') eq++;
            if (*eq == '=') return eq + 1;
            return (char*)"";
        }
    }
    return 0;
}

static int env_len(char** v) {
    int n = 0;
    if (v) while (v[n]) n++;
    return n;
}

int
setenv(const char* name, const char* value, int overwrite)
{
    if (!name || !value) return -1;
    // Build the "name=value" buffer.
    int nlen = strlen(name);
    int vlen = strlen(value);
    char* buf = (char*)malloc(nlen + 1 + vlen + 1);
    if (!buf) return -1;
    memcpy(buf, name, nlen);
    buf[nlen] = '=';
    memcpy(buf + nlen + 1, value, vlen);
    buf[nlen + 1 + vlen] = 0;

    // If `name` already in environ, replace (or skip if !overwrite).
    if (environ) {
        for (char** p = environ; *p; p++) {
            if (env_match(*p, name)) {
                if (!overwrite) {
                    free(buf);
                    return 0;
                }
                // Leak the prior entry — we don't know if it was
                // malloced or from the kernel-built initial array.
                *p = buf;
                return 0;
            }
        }
    }
    // Append: grow environ.
    int n = env_len(environ);
    char** new_env = (char**)malloc((n + 2) * sizeof(char*));
    if (!new_env) {
        free(buf);
        return -1;
    }
    for (int i = 0; i < n; i++) new_env[i] = environ[i];
    new_env[n] = buf;
    new_env[n + 1] = 0;
    if (environ_owned) free(environ);
    environ = new_env;
    environ_owned = 1;
    return 0;
}

int
unsetenv(const char* name)
{
    if (!environ || !name) return 0;
    int n = env_len(environ);
    int w = 0;
    for (int r = 0; r < n; r++) {
        if (!env_match(environ[r], name)) {
            environ[w++] = environ[r];
        }
    }
    environ[w] = 0;
    return 0;
}

int
sigaction(int signum, const struct sigaction* act,
          struct sigaction* oldact)
{
    void* handler = (act != 0) ? (void*)act->sa_handler : (void*)0;
    unsigned int mask = (act != 0) ? act->sa_mask : 0u;
    int prev = _sys_sigaction(signum, handler, (void*)_sigret, mask);
    if (prev < 0) return -1;
    if (oldact) {
        oldact->sa_handler = (sighandler_t)(long)prev;
        oldact->sa_mask = 0;  // we don't round-trip mask in this slice
    }
    return 0;
}

extern char* sys_sbrk(int n, int lazy);

char*
sbrk(int n) {
    return sys_sbrk(n, 0);  // SBRK_EAGER
}

char*
sbrklazy(int n) {
    return sys_sbrk(n, 1);  // SBRK_LAZY
}
