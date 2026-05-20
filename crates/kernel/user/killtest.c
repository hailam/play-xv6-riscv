// killtest — fork a child that sleeps for a long time, then kill it
// from the parent and verify the child was reaped promptly.

extern int fork(void);
extern void exit(int);
extern int wait(int* status);
extern int sleep(int);
extern int kill(int);
extern int write(int, const void*, int);

static int u_strlen(const char* s) { int n = 0; while (s[n]) n++; return n; }

static void put_int(int n) {
    char buf[16];
    int  i = 0;
    int  neg = n < 0;
    unsigned u = neg ? -n : n;
    if (u == 0) buf[i++] = '0';
    else { while (u) { buf[i++] = (char)('0' + u % 10); u /= 10; } }
    if (neg) write(1, "-", 1);
    while (i--) write(1, &buf[i], 1);
}

int main(void) {
    int pid = fork();
    if (pid < 0) {
        write(2, "killtest: fork failed\n", 22);
        return -1;
    }
    if (pid == 0) {
        // Child: long sleep. If kill works, the sleep should return
        // early and the proc should exit(-1). If kill is broken we'll
        // see this completion message.
        sleep(10000);
        write(1, "killtest: CHILD WOKE UP (BAD — kill didn't fire)\n", 49);
        exit(0);
    }
    // Parent: small delay so the child has reached its `sleep` before
    // we kill it.
    sleep(20);
    write(1, "killtest: sending kill to pid ", 30);
    put_int(pid);
    write(1, "\n", 1);
    if (kill(pid) < 0) {
        write(1, "killtest: kill syscall failed\n", 30);
        wait(0);
        return -1;
    }
    int status = 0;
    int reaped = wait(&status);
    int code = (reaped == pid) ? status : -1;
    write(1, "killtest: child reaped, exit=", 29);
    put_int(code);
    write(1, "\n", 1);
    return 0;
}
