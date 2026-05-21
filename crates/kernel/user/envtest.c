// envtest — execve + envp + getenv/setenv round-trip.
//
// Mode 1 (no args): parent. execve's itself with envp = {A=hello, B=world}.
// Mode 2 (arg "child"): verify env, set a third var, execve grandchild.
// Mode 3 (arg "gchild"): verify all three vars came through.

#include "user.h"

static int u_strcmp(const char* a, const char* b) {
    while (*a && *a == *b) { a++; b++; }
    return (unsigned char)*a - (unsigned char)*b;
}

static void die(const char* tag, const char* msg) {
    printf("envtest:%s: %s\n", tag, msg);
    exit(1);
}

int main(int argc, char* argv[]) {
    if (argc < 2) {
        // Parent.
        printf("parent: environ=%p (expected (nil) — no kernel envp)\n",
               (void*)environ);

        char* env[] = {
            (char*)"A=hello",
            (char*)"B=world",
            0
        };
        char* av[] = {(char*)"envtest", (char*)"child", 0};
        execve("/envtest", av, env);
        die("parent", "execve failed");
    }
    if (u_strcmp(argv[1], "child") == 0) {
        printf("child: environ=%p\n", (void*)environ);
        // Verify A and B came through.
        char* a = getenv("A");
        char* b = getenv("B");
        printf("child: A=%s B=%s (expected hello world)\n",
               a ? a : "(null)", b ? b : "(null)");
        if (!a || u_strcmp(a, "hello") != 0) die("child", "A missing");
        if (!b || u_strcmp(b, "world") != 0) die("child", "B missing");
        // setenv C, then execve grandchild with current environ.
        if (setenv("C", "added", 1) < 0) die("child", "setenv failed");
        char* cc = getenv("C");
        printf("child: C after setenv = %s (expected added)\n",
               cc ? cc : "(null)");
        if (!cc || u_strcmp(cc, "added") != 0) die("child", "setenv broken");

        char* av[] = {(char*)"envtest", (char*)"gchild", 0};
        execve("/envtest", av, environ);
        die("child", "execve gchild failed");
    }
    if (u_strcmp(argv[1], "gchild") == 0) {
        // Verify all three came through.
        char* a = getenv("A");
        char* b = getenv("B");
        char* c = getenv("C");
        printf("gchild: A=%s B=%s C=%s\n",
               a ? a : "(null)", b ? b : "(null)", c ? c : "(null)");
        if (!a || u_strcmp(a, "hello") != 0) die("gchild", "A lost");
        if (!b || u_strcmp(b, "world") != 0) die("gchild", "B lost");
        if (!c || u_strcmp(c, "added") != 0) die("gchild", "C lost");
        // unsetenv B; verify gone.
        unsetenv("B");
        if (getenv("B") != 0) die("gchild", "unsetenv didn't take");
        printf("envtest ok\n");
        return 0;
    }
    die("main", "unknown mode");
    return 1;
}
