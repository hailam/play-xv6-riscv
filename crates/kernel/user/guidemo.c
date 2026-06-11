//
// guidemo — launch the GUI stack (todo 12 M4). The shell has no
// background jobs, so this forks the display server and the demo
// clients itself: wm, then hello (cyan, key-reactive), then clock
// (color cycler). Stays alive reaping children.
//
#include "user.h"

static void
run(const char *path)
{
  int pid = fork();
  if(pid == 0){
    char *argv[] = { (char*)path, 0 };
    exec((char*)path, argv);
    printf("guidemo: exec %s failed\n", path);
    exit(1);
  }
}

int
main(void)
{
  run("/wm");
  sleep(30);     // let wm claim fb/input/socket and clear the screen
  run("/hello_wm");
  sleep(15);
  run("/clock");
  printf("guidemo: running\n");
  for(;;){
    int st;
    if(wait(&st) < 0)
      pause(); // no children left (shouldn't happen)
  }
}
