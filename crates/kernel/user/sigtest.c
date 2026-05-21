// sigtest — POSIX kill(pid, sig) dispositions.
//
// We only implement default dispositions in this slice — no
// user-installed handlers. So this exercises:
//   * kill(pid, 0)        — existence check
//   * kill(pid, SIGCHLD)  — ignorable; returns 0 without killing
//   * kill(pid, SIGTERM)  — fatal; child exits -1
//   * kill(pid, 9999)     — invalid signum returns -1

#include "user.h"

static void die(const char* msg) {
    printf("sigtest: %s\n", msg);
    exit(1);
}

static void busy_wait(void) {
    sleep(3);
}

int main(int argc, char* argv[]) {
    // 1) Existence check — kill(self, 0) returns 0.
    int self = getpid();
    int r = kill(self, 0);
    printf("kill(self, 0) -> %d (expected 0)\n", r);
    if (r != 0) die("existence on self failed");

    // 2) Existence check on nonexistent pid — returns -1.
    r = kill(0x7fffffff, 0);
    printf("kill(0x7fffffff, 0) -> %d (expected -1)\n", r);
    if (r != -1) die("existence on absent pid not -1");

    // 3) Invalid signum.
    r = kill(self, 9999);
    printf("kill(self, 9999) -> %d (expected -1)\n", r);
    if (r != -1) die("invalid signum not -1");

    // 4) Fork child that sleeps; parent sends SIGCHLD — no-op, child
    //    still sleeps. Then parent sends SIGTERM — child exits.
    int pid = fork();
    if (pid < 0) die("fork failed");
    if (pid == 0) {
        sleep(200);
        // Should never reach here if SIGTERM landed.
        printf("sigtest: CHILD WOKE UP (BAD)\n");
        exit(0);
    }
    busy_wait();  // let child reach `sleep`

    // SIGCHLD — should not kill.
    r = kill(pid, SIGCHLD);
    printf("kill(pid, SIGCHLD) -> %d (expected 0, child still alive)\n", r);
    if (r != 0) die("SIGCHLD returned nonzero");

    // Confirm still alive.
    r = kill(pid, 0);
    printf("after SIGCHLD: kill(pid, 0) -> %d (expected 0, still exists)\n", r);
    if (r != 0) die("SIGCHLD killed the child (it shouldn't have)");

    // SIGTERM — should kill.
    r = kill(pid, SIGTERM);
    printf("kill(pid, SIGTERM) -> %d (expected 0, child dies)\n", r);
    if (r != 0) die("SIGTERM returned nonzero");

    int status = 0;
    int reaped = wait(&status);
    printf("wait reaped=%d status=%d (expected pid=%d, status=-1)\n",
           reaped, status, pid);
    if (reaped != pid) die("wait reaped wrong pid");
    if (status != -1) die("child didn't exit with -1");

    printf("sigtest ok\n");
    return 0;
}
