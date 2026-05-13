// Tiny shell. Tokenizes a line into argv, supports `cmd args...` and
// `left ... | right ...`.

extern int   fork(void);
extern __attribute__((noreturn)) void exit(int);
extern int   wait(void);
extern int   pipe(int*);
extern int   read(int, void*, int);
extern int   exec(const char*, char* const argv[]);
extern int   dup(int);
extern int   write(int, const void*, int);
extern int   close(int);

#define MAX_ARGS 8

static char buf[256];

// Split a NUL-terminated `line` in place on space runs. Fills `out`
// with pointers to each token plus a trailing NULL. Returns argc.
static int tokenize(char* line, char** out) {
    int argc = 0;
    char* p = line;
    while (*p && argc < MAX_ARGS - 1) {
        while (*p == ' ') p++;
        if (*p == 0) break;
        out[argc++] = p;
        while (*p && *p != ' ') p++;
        if (*p) {
            *p++ = 0;
        }
    }
    out[argc] = 0;
    return argc;
}

static void run_simple(char* line) {
    char* argv[MAX_ARGS];
    int argc = tokenize(line, argv);
    if (argc == 0) return;
    if (fork() == 0) {
        exec(argv[0], argv);
        write(2, "exec failed\n", 12);
        exit(-1);
    }
    wait();
}

static void run_pipeline(char* left, char* right) {
    int p[2];
    if (pipe(p) < 0) {
        write(2, "pipe failed\n", 12);
        return;
    }

    char* largv[MAX_ARGS];
    char* rargv[MAX_ARGS];
    if (tokenize(left, largv) == 0 || tokenize(right, rargv) == 0) {
        close(p[0]); close(p[1]);
        return;
    }

    if (fork() == 0) {
        close(1); dup(p[1]); close(p[0]); close(p[1]);
        exec(largv[0], largv);
        write(2, "exec L failed\n", 14);
        exit(-1);
    }
    if (fork() == 0) {
        close(0); dup(p[0]); close(p[0]); close(p[1]);
        exec(rargv[0], rargv);
        write(2, "exec R failed\n", 14);
        exit(-1);
    }
    close(p[0]); close(p[1]);
    wait(); wait();
}

int main(void) {
    for (;;) {
        write(1, "$ ", 2);
        int n = read(0, buf, (int)sizeof(buf) - 1);
        if (n <= 0) {
            write(1, "bye\n", 4);
            return 0;
        }
        if (n > 0 && buf[n - 1] == '\n') {
            buf[n - 1] = 0;
        } else {
            buf[n] = 0;
        }
        if (buf[0] == 0) continue;

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
            int li = bar - 1;
            while (li >= 0 && buf[li] == ' ') { buf[li] = 0; li--; }
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
