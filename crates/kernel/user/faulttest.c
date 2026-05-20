// faulttest — fork a child that deliberately dereferences a bogus
// pointer. With xv6-style fault handling, the child gets killed
// cleanly and the parent reaps it; the kernel survives.

#include "user.h"

int main(void) {
    int pid = fork();
    if (pid < 0) {
        printf("faulttest: fork failed\n");
        return -1;
    }
    if (pid == 0) {
        // Child — touch a never-mapped VA. With G6 the child gets
        // killed cleanly and the kernel stays up.
        volatile int* bad = (volatile int*)0xdeadc0de;
        int v = *bad;
        printf("child survived (BAD)\n");
        exit(v);
    }
    int status = 0;
    int reaped = wait(&status);
    printf("faulttest: reaped pid=%d, status=%d\n", reaped, status);
    if (reaped != pid) {
        printf("faulttest: BAD — reaped pid mismatch\n");
        return -1;
    }
    if (status != -1) {
        printf("faulttest: BAD — expected status -1\n");
        return -1;
    }
    printf("faulttest: ok\n");
    return 0;
}
