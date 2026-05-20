// Tiny shell.
//
// Supported syntax:
//   cmd args...           — simple
//   left ... | right ...  — pipeline (2 stages)
//   cmd > path            — stdout redirect (O_CREATE | O_TRUNC)
//   cmd >> path           — stdout append (O_CREATE)
//   cd path               — builtin (must run in the shell itself, not a child)
//
// All other syntax stays the responsibility of the user program (e.g.
// `ls /` parses its own argv).

extern int   fork(void);
extern __attribute__((noreturn)) void exit(int);
extern int   wait(void);
extern int   pipe(int*);
extern int   read(int, void*, int);
extern int   exec(const char*, char* const argv[]);
extern int   dup(int);
extern int   write(int, const void*, int);
extern int   close(int);
extern int   open(const char*, int);
extern int   chdir(const char*);

#define O_RDONLY 0
#define O_WRONLY 0x001
#define O_CREATE 0x200
#define O_TRUNC  0x400

#define MAX_ARGS 8

static char buf[256];

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

// Locate `>` or `>>` in the line and split it. Returns 1 if a
// redirect was found, with `*target_out` pointing at the path and
// `*append_out` set. Returns 0 if none.
static int strip_redirect(char* line, char** target_out, int* append_out) {
    for (int i = 0; line[i]; i++) {
        if (line[i] != '>') continue;
        int append = 0;
        if (line[i + 1] == '>') { append = 1; line[i + 1] = 0; }
        line[i] = 0;
        char* p = line + i + 1 + (append ? 1 : 0);
        while (*p == ' ') p++;
        char* end = p;
        while (*end && *end != ' ') end++;
        *end = 0;
        if (*p == 0) return -1; // syntax: `>` with no target
        *target_out = p;
        *append_out = append;
        // Trim trailing spaces from the command part.
        int j = i - 1;
        while (j >= 0 && line[j] == ' ') { line[j] = 0; j--; }
        return 1;
    }
    return 0;
}

static void apply_redirect(const char* target, int append) {
    int flags = O_WRONLY | O_CREATE | (append ? 0 : O_TRUNC);
    int fd = open(target, flags);
    if (fd < 0) {
        write(2, "open failed\n", 12);
        exit(-1);
    }
    close(1);
    if (dup(fd) != 1) {
        write(2, "dup failed\n", 11);
        exit(-1);
    }
    close(fd);
}

static int try_builtin(int argc, char** argv) {
    if (argc < 1) return 0;
    if (argv[0][0] == 'c' && argv[0][1] == 'd' && argv[0][2] == 0) {
        if (argc < 2) {
            write(2, "cd: missing path\n", 17);
            return 1;
        }
        if (chdir(argv[1]) < 0) {
            write(2, "cd: failed\n", 11);
        }
        return 1;
    }
    return 0;
}

static void run_simple(char* line) {
    char* target = 0;
    int   append = 0;
    int   r = strip_redirect(line, &target, &append);
    if (r < 0) {
        write(2, "syntax\n", 7);
        return;
    }

    char* argv[MAX_ARGS];
    int argc = tokenize(line, argv);
    if (argc == 0) return;

    // Builtins run in the shell process itself (no fork).
    if (try_builtin(argc, argv)) return;

    if (fork() == 0) {
        if (r > 0) apply_redirect(target, append);
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
