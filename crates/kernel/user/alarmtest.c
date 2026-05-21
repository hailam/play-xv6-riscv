// alarmtest — verify alarm(N) + SIGALRM handler + pause() wakeup +
// waitpid + WNOHANG.

#include "user.h"

static volatile int alarm_count;

static void on_alarm(int sig) {
    alarm_count++;
}

static void die(const char* msg) {
    printf("alarmtest: %s\n", msg);
    exit(1);
}

int main(int argc, char* argv[]) {
    // 1) Install SIGALRM handler, alarm(1), pause().
    struct sigaction sa;
    sa.sa_handler = on_alarm;
    sa.sa_mask = 0;
    if (sigaction(SIGALRM, &sa, 0) < 0) die("sigaction(SIGALRM) failed");

    unsigned prev = alarm(1);
    printf("alarm(1) -> previous=%u (expected 0)\n", prev);
    if (prev != 0) die("initial prev != 0");

    // pause() blocks; SIGALRM fires, handler runs, pause returns -1.
    int p = pause();
    printf("pause -> %d alarm_count=%d (expected -1, 1)\n", p, alarm_count);
    if (p != -1) die("pause didn't return -1");
    if (alarm_count != 1) die("alarm handler didn't fire");

    // 2) alarm(0) cancels pending. Schedule, cancel, sleep — handler
    //    must NOT fire.
    alarm(5);
    unsigned remaining = alarm(0);
    printf("alarm(5) then alarm(0): remaining=%u (expected ~5)\n", remaining);
    if (remaining < 4) die("alarm(0) didn't see the prior remaining");
    int before = alarm_count;
    sleep(15);
    printf("after cancel + sleep(15): alarm_count=%d (expected %d)\n",
           alarm_count, before);
    if (alarm_count != before) die("cancelled alarm still fired");

    // 3) waitpid with WNOHANG before child exits: 0; then specific pid.
    int pid = fork();
    if (pid < 0) die("fork failed");
    if (pid == 0) {
        sleep(10);
        exit(7);
    }
    // Tight WNOHANG poll — child is still sleeping.
    int status = 0;
    int r = waitpid(pid, &status, WNOHANG);
    printf("WNOHANG before child exit -> %d (expected 0)\n", r);
    if (r != 0) die("WNOHANG returned non-zero with live child");

    // Now wait blocking for the specific pid.
    r = waitpid(pid, &status, 0);
    printf("waitpid(pid, blocking) -> %d status=%d (expected pid=%d, 7)\n",
           r, status, pid);
    if (r != pid) die("waitpid wrong pid");
    if (status != 7) die("waitpid wrong status");

    // waitpid for a nonexistent pid → -1.
    r = waitpid(99999, &status, 0);
    printf("waitpid(99999) -> %d (expected -1)\n", r);
    if (r != -1) die("waitpid(nonexistent) didn't return -1");

    printf("alarmtest ok\n");
    return 0;
}
