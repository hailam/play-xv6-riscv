// Tiny shell. Supports a single command or `left | right`.

extern int   fork(void);
extern __attribute__((noreturn)) void exit(int);
extern int   wait(void);
extern int   pipe(int*);
extern int   read(int, void*, int);
extern int   exec(const char*, char**);
extern int   dup(int);
extern int   write(int, const void*, int);
extern int   close(int);

static char buf[128];

static void run_simple(const char* cmd) {
    if (fork() == 0) {
        exec(cmd, 0);
        write(2, "exec failed\n", 12);
        exit(-1);
    }
    wait();
}

static void run_pipeline(const char* left, const char* right) {
    int p[2];
    if (pipe(p) < 0) {
        write(2, "pipe failed\n", 12);
        return;
    }

    if (fork() == 0) {
        // left: stdout → pipe write end
        close(1);
        dup(p[1]);
        close(p[0]);
        close(p[1]);
        exec(left, 0);
        write(2, "exec L failed\n", 14);
        exit(-1);
    }

    if (fork() == 0) {
        // right: stdin → pipe read end
        close(0);
        dup(p[0]);
        close(p[0]);
        close(p[1]);
        exec(right, 0);
        write(2, "exec R failed\n", 14);
        exit(-1);
    }

    close(p[0]);
    close(p[1]);
    wait();
    wait();
}

int main(void) {
    for (;;) {
        write(1, "$ ", 2);
        int n = read(0, buf, (int)sizeof(buf) - 1);
        if (n <= 0) {
            write(1, "bye\n", 4);
            return 0;
        }
        // Strip trailing newline if any.
        if (n > 0 && buf[n - 1] == '\n') {
            buf[n - 1] = 0;
        } else {
            buf[n] = 0;
        }
        if (buf[0] == 0) continue;

        // Look for '|'.
        int bar = -1;
        for (int i = 0; buf[i]; i++) {
            if (buf[i] == '|') {
                bar = i;
                buf[i] = 0;
                break;
            }
        }

        if (bar < 0) {
            run_simple(buf);
        } else {
            // Trim trailing space on left, leading space on right.
            int li = bar - 1;
            while (li >= 0 && buf[li] == ' ') {
                buf[li] = 0;
                li--;
            }
            char* right = buf + bar + 1;
            while (*right == ' ') right++;
            if (buf[0] == 0 || *right == 0) {
                write(2, "syntax\n", 7);
                continue;
            }
            run_pipeline(buf, right);
        }
    }
}
