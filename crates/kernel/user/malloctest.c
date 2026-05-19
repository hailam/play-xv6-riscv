// malloctest — exercise the user-space malloc / free.
//
//   * allocate a small region, fill with a pattern, write it, free.
//   * allocate ~64 KiB to force several `sbrk` calls, write a
//     stamp at the start and end of each chunk, free.
//   * stress alloc/free in a long loop to exercise the freelist
//     coalescing path.

typedef unsigned long uintptr_t;

extern void* malloc(unsigned int);
extern void  free(void*);
extern int   write(int, const void*, int);
extern char* sbrk(int);

static int u_strlen(const char* s) {
    int n = 0;
    while (s[n]) n++;
    return n;
}

static void puts_raw(const char* s) { write(1, s, u_strlen(s)); }

static void put_u64(unsigned long n) {
    char buf[24];
    int i = 0;
    if (n == 0) buf[i++] = '0';
    else { while (n > 0) { buf[i++] = (char)('0' + (n % 10)); n /= 10; } }
    while (i--) write(1, &buf[i], 1);
}

int main(void) {
    puts_raw("malloctest: small alloc — ");
    char* p = (char*)malloc(100);
    if (!p) { puts_raw("FAIL\n"); return -1; }
    for (int i = 0; i < 99; i++) p[i] = (char)('A' + (i % 26));
    p[99] = 0;
    write(1, p, 99);
    write(1, "\n", 1);
    free(p);

    puts_raw("malloctest: pre-bulk sbrk(0) = ");
    put_u64((unsigned long)(uintptr_t)sbrk(0));
    write(1, "\n", 1);

    // Force the heap to grow several pages.
    const int N = 32;
    const int CHUNK = 2048;
    char* arr[32];
    for (int i = 0; i < N; i++) {
        arr[i] = (char*)malloc(CHUNK);
        if (!arr[i]) {
            puts_raw("malloctest: bulk malloc FAIL at i=");
            put_u64((unsigned long)i);
            write(1, "\n", 1);
            return -1;
        }
        arr[i][0]         = (char)('a' + (i % 26));
        arr[i][CHUNK - 1] = (char)('z' - (i % 26));
    }

    // Verify the stamps survived (no overlap, no clobbering).
    int bad = 0;
    for (int i = 0; i < N; i++) {
        if (arr[i][0] != (char)('a' + (i % 26))
         || arr[i][CHUNK - 1] != (char)('z' - (i % 26)))
        {
            bad = 1;
            break;
        }
    }
    puts_raw(bad ? "malloctest: bulk stamp FAIL\n" : "malloctest: bulk stamps survived\n");

    puts_raw("malloctest: post-bulk sbrk(0) = ");
    put_u64((unsigned long)(uintptr_t)sbrk(0));
    write(1, "\n", 1);

    // Free every other one to fragment, then realloc.
    for (int i = 0; i < N; i += 2) free(arr[i]);
    char* refills[16];
    for (int i = 0; i < N / 2; i++) {
        refills[i] = (char*)malloc(CHUNK);
        if (!refills[i]) { puts_raw("malloctest: refill FAIL\n"); return -1; }
    }
    for (int i = 0; i < N / 2; i++) free(refills[i]);
    for (int i = 1; i < N; i += 2) free(arr[i]);

    puts_raw("malloctest: done\n");
    return 0;
}

