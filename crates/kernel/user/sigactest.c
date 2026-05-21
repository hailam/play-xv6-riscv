// sigactest — user-installed signal handler dispatch.
//
// Installs a handler for SIGUSR1, sends `kill(self, SIGUSR1)`, and
// checks that:
//   * The handler fires (sets a counter)
//   * The handler runs with the right signum in arg 0
//   * Control returns past the kill() call (sigreturn restored
//     epc/sp/regs so the user code resumed cleanly).

#include "user.h"

static volatile int handler_count;
static volatile int handler_saw_sig;

static void on_usr1(int sig) {
    handler_saw_sig = sig;
    handler_count++;
}

static void die(const char* msg) {
    printf("sigactest: %s\n", msg);
    exit(1);
}

int main(int argc, char* argv[]) {
    struct sigaction sa;
    sa.sa_handler = on_usr1;
    sa.sa_mask = 0;
    if (sigaction(SIGUSR1, &sa, 0) < 0) die("sigaction failed");

    int self = getpid();
    if (kill(self, SIGUSR1) < 0) die("kill(self, SIGUSR1) failed");
    // The kernel queues the signal pending; delivery happens on the
    // next user-mode return. The next syscall (sleep) bounces us
    // through the kernel and back — handler runs on that return.
    // Use a short sleep to force the trip.
    sleep(2);

    printf("handler_count=%d handler_saw_sig=%d (expected 1, %d)\n",
           handler_count, handler_saw_sig, SIGUSR1);
    if (handler_count != 1) die("handler didn't fire once");
    if (handler_saw_sig != SIGUSR1) die("handler got wrong signum");

    // SIG_DFL — installing it should disarm the handler. Send
    // SIGCHLD (default-ignore) — should not change handler_count.
    sa.sa_handler = SIG_DFL;
    if (sigaction(SIGUSR1, &sa, 0) < 0) die("sigaction restore failed");
    handler_count = 0;
    if (kill(self, SIGCHLD) < 0) die("kill SIGCHLD failed");
    sleep(2);
    printf("after SIGCHLD: handler_count=%d (expected 0)\n", handler_count);
    if (handler_count != 0) die("handler shouldn't have fired on SIGCHLD");

    // SIGKILL should not be catchable.
    sa.sa_handler = on_usr1;
    int r = sigaction(SIGKILL, &sa, 0);
    printf("sigaction(SIGKILL) -> %d (expected -1)\n", r);
    if (r != -1) die("SIGKILL was catchable (BAD)");

    printf("sigactest ok\n");
    return 0;
}
