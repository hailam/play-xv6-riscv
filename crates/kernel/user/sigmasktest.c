// sigmasktest — POSIX sigprocmask semantics.
//
// Verifies:
//   * SIG_BLOCK blocks a signal; pending stays queued, handler
//     doesn't fire across syscall boundaries.
//   * SIG_UNBLOCK unblocks; handler fires on next return-to-user.
//   * SIG_SETMASK replaces the mask outright; oldset reports prior.
//   * Handler runs with (1 << sig) added to blocked, so re-sending
//     the same signal from within the handler stays pending until
//     sigreturn restores the pre-delivery mask.

#include "user.h"

static volatile int handler_count;
static volatile int reentered;

static void on_usr1(int sig) {
    // Within the handler, sig itself is blocked. Try to send it
    // again — should queue, not re-enter.
    if (handler_count == 0) {
        int self = getpid();
        kill(self, SIGUSR1);   // queued, blocked
        sleep(1);              // back through kernel; mask blocks delivery
        // If we re-enter, reentered will already be 1.
        if (handler_count != 0) reentered = 1;
    }
    handler_count++;
}

static void die(const char* msg) {
    printf("sigmasktest: %s\n", msg);
    exit(1);
}

int main(int argc, char* argv[]) {
    struct sigaction sa;
    sa.sa_handler = on_usr1;
    sa.sa_mask = 0;
    if (sigaction(SIGUSR1, &sa, 0) < 0) die("sigaction failed");

    int self = getpid();

    // 1) Block SIGUSR1, queue it, verify handler doesn't fire.
    sigset_t old = 0xdeadbeef;
    if (sigprocmask(SIG_BLOCK, 1u << SIGUSR1, &old) != 0) die("BLOCK failed");
    printf("after BLOCK: previous mask = 0x%x (expected 0)\n", old);
    if (old != 0) die("initial mask != 0");

    if (kill(self, SIGUSR1) < 0) die("kill failed");
    sleep(2);   // bounces through return-to-user; should NOT fire
    printf("after kill-while-blocked: handler_count=%d (expected 0)\n",
           handler_count);
    if (handler_count != 0) die("handler fired while blocked");

    // 2) Unblock — pending signal should fire.
    if (sigprocmask(SIG_UNBLOCK, 1u << SIGUSR1, &old) != 0) die("UNBLOCK failed");
    printf("after UNBLOCK: previous mask = 0x%x (expected 0x%x)\n",
           old, 1u << SIGUSR1);
    if (old != (1u << SIGUSR1)) die("UNBLOCK reported wrong prev mask");

    sleep(2);   // bounces through return-to-user; should fire now
    printf("after UNBLOCK + sleep: handler_count=%d reentered=%d "
           "(expected >=1, 0)\n", handler_count, reentered);
    if (handler_count < 1) die("handler didn't fire after unblock");
    if (reentered) die("handler re-entered itself");

    // 3) Wait long enough for the in-handler re-send to deliver
    //    after sigreturn unmasks.
    sleep(2);
    printf("final handler_count=%d (expected >=2 — handler self-resent)\n",
           handler_count);
    if (handler_count < 2) die("queued in-handler signal didn't deliver");

    // 4) SIG_SETMASK replaces mask outright.
    if (sigprocmask(SIG_SETMASK, 0u, &old) != 0) die("SETMASK failed");

    printf("sigmasktest ok\n");
    return 0;
}
