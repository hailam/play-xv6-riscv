// mmaptest — anonymous mmap/munmap.

#include "user.h"

static void die(const char* msg) {
    printf("mmaptest: %s\n", msg);
    exit(1);
}

int main(int argc, char* argv[]) {
    // 1) mmap 3 pages of anonymous RW.
    unsigned int len = 3 * 4096;
    void* p = mmap(0, len, PROT_READ | PROT_WRITE,
                   MAP_ANONYMOUS | MAP_PRIVATE, -1, 0);
    printf("mmap(3 pages) -> %p\n", p);
    if (p == MAP_FAILED) die("mmap failed");

    // 2) Lazy: touch each page; should fault in zeroed.
    char* c = (char*)p;
    for (int i = 0; i < (int)len; i++) {
        if (c[i] != 0) die("page not zero before write");
    }
    c[0] = 'A';
    c[4096] = 'B';
    c[8192] = 'C';
    if (c[0] != 'A' || c[4096] != 'B' || c[8192] != 'C')
        die("write/read mismatch");
    printf("touched pages: A=%c B=%c C=%c\n", c[0], c[4096], c[8192]);

    // 3) Second mmap returns a different region.
    void* p2 = mmap(0, 4096, PROT_READ | PROT_WRITE,
                    MAP_ANONYMOUS | MAP_PRIVATE, -1, 0);
    printf("mmap(1 page) -> %p (must differ from %p)\n", p2, p);
    if (p2 == MAP_FAILED) die("mmap#2 failed");
    if (p2 == p) die("mmap returned same VA");

    // 4) Bad flags: no MAP_ANONYMOUS → FAILED.
    void* p3 = mmap(0, 4096, PROT_READ, MAP_PRIVATE, -1, 0);
    printf("mmap(no-anon) -> %p (expected MAP_FAILED)\n", p3);
    if (p3 != MAP_FAILED) die("non-anon mmap accepted");

    // 5) munmap the first region; verify subsequent access faults.
    //    We can't catch the fault directly in this slim test
    //    framework, so we just verify munmap returns 0.
    int r = munmap(p, len);
    printf("munmap(%p, %u) -> %d (expected 0)\n", p, len, r);
    if (r != 0) die("munmap failed");
    r = munmap(p2, 4096);
    if (r != 0) die("munmap#2 failed");

    // 6) Double munmap → -1 (no VMA at that range).
    r = munmap(p, len);
    printf("munmap again -> %d (expected -1)\n", r);
    if (r != -1) die("double munmap accepted");

    // 7) Stress: 16 small mmaps + 16 munmaps.
    void* arr[16];
    for (int i = 0; i < 16; i++) {
        arr[i] = mmap(0, 4096, PROT_READ | PROT_WRITE,
                      MAP_ANONYMOUS | MAP_PRIVATE, -1, 0);
        if (arr[i] == MAP_FAILED) die("stress mmap failed");
        ((char*)arr[i])[0] = (char)('a' + i);
    }
    for (int i = 0; i < 16; i++) {
        if (((char*)arr[i])[0] != (char)('a' + i))
            die("stress data wrong");
    }
    for (int i = 0; i < 16; i++) {
        if (munmap(arr[i], 4096) != 0) die("stress munmap failed");
    }
    printf("stress 16x ok\n");

    printf("mmaptest ok\n");
    return 0;
}
