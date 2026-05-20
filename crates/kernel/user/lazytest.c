// lazytest — exercise lazy sbrk:
//   * sbrklazy(N) grows proc.size by N without mapping pages.
//   * Writing through the new range should fault-and-allocate
//     on demand. We never see a fault (it's transparent).
//   * Reading back the written bytes should return what we wrote.

#include "user.h"

#define N_PAGES   8
#define PAGE_SIZE 4096

int main(void) {
    char* base = sbrklazy(N_PAGES * PAGE_SIZE);
    if (base == (char*)-1) {
        printf("lazytest: sbrklazy failed\n");
        return -1;
    }
    printf("lazytest: lazy region at %p, %d pages\n", base, N_PAGES);

    // Touch one byte on each page. With lazy allocation, the first
    // touch on each page triggers a fault that the kernel resolves
    // by mapping a fresh zero frame.
    for (int i = 0; i < N_PAGES; i++) {
        base[i * PAGE_SIZE] = (char)('A' + i);
    }
    printf("lazytest: wrote initial stamps\n");

    // Read back, verify.
    int bad = 0;
    for (int i = 0; i < N_PAGES; i++) {
        char want = (char)('A' + i);
        if (base[i * PAGE_SIZE] != want) {
            printf("lazytest: BAD at page %d: got %c want %c\n",
                   i, base[i * PAGE_SIZE], want);
            bad = 1;
        }
    }
    if (bad) {
        printf("lazytest: FAIL\n");
        return -1;
    }
    printf("lazytest: %d stamps verified, lazy mapping works\n", N_PAGES);

    // Also verify the rest of each page is zero (kernel must
    // alloc_zeroed before mapping).
    for (int i = 0; i < N_PAGES; i++) {
        for (int o = 1; o < PAGE_SIZE; o++) {
            if (base[i * PAGE_SIZE + o] != 0) {
                printf("lazytest: BAD: page %d offset %d nonzero (%d)\n",
                       i, o, base[i * PAGE_SIZE + o]);
                return -1;
            }
        }
    }
    printf("lazytest: pages are zero-filled, ok\n");
    return 0;
}
