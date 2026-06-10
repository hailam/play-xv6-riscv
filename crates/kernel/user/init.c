//
// init — the first real user process (pid 1). The kernel's embedded
// initcode execs us. Two jobs, exactly like xv6's init:
//   * keep a shell running (restart it if it exits), and
//   * sit in wait() so orphaned children the kernel reparents to us
//     get reaped promptly (their exit status is discarded).
//
// fds 0/1/2 arrive pre-wired to the console (the kernel populates the
// first proc's fd table), so unlike xv6 we don't open("console").
//

#include "user.h"

int
main(void)
{
  for(;;){
    int shpid = fork();
    if(shpid < 0){
      printf("init: fork failed\n");
      sleep(10);
      continue;
    }
    if(shpid == 0){
      char *argv[] = { "sh", 0 };
      exec("/sh", argv);
      printf("init: exec /sh failed\n");
      exit(1);
    }
    for(;;){
      int st;
      int wpid = wait(&st);
      if(wpid == shpid){
        printf("init: sh exited, restarting\n");
        break;
      }
      if(wpid < 0){
        printf("init: wait error %d\n", wpid);
        sleep(10);
        break;
      }
      // Otherwise: an orphan we inherited — nothing to do.
    }
  }
}
