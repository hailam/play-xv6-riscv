// smptest — fork several children concurrently. Each sleeps briefly
// (so the parent can fork the next one while the previous still
// occupies a per-CPU ready queue) then exits. The kernel's exit
// log prints which hart the child landed on; we expect to see
// non-hart-0 entries when running with -smp >= 2.

extern int  fork(void);
extern void exit(int);
extern int  wait(int* status);
extern int  sleep(int);
extern int  write(int, const void*, int);

#define N 6

int main(void) {
    for (int i = 0; i < N; i++) {
        int pid = fork();
        if (pid == 0) {
            sleep(2 + i);
            const char* msg = "smptest child done\n";
            int n = 0; while (msg[n]) n++;
            write(1, msg, n);
            exit(0);
        }
    }
    for (int i = 0; i < N; i++) wait(0);
    const char* msg = "smptest: all done\n";
    int n = 0; while (msg[n]) n++;
    write(1, msg, n);
    return 0;
}
