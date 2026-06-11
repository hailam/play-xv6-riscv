//
// tcpecho — listen on 0.0.0.0:7878 and echo one connection until
// EOF. Driven by the host-side harness through qemu's SLIRP
// hostfwd to validate the virtio-net + smoltcp path end to end.
//
#include "user.h"

int
main(void)
{
  int s = socket(2, 1, 0);
  if(s < 0 || bind(s, "0.0.0.0:7878") < 0 || listen(s, 1) < 0){
    printf("tcpecho: setup failed\n");
    exit(1);
  }
  printf("tcpecho: listening on :7878\n");
  int c = accept(s);
  if(c < 0){
    printf("tcpecho: accept failed\n");
    exit(1);
  }
  char b[256];
  int n;
  while((n = read(c, b, sizeof(b))) > 0){
    if(write(c, b, n) != n){
      printf("tcpecho: write failed\n");
      exit(1);
    }
  }
  close(c);
  close(s);
  printf("tcpecho: done\n");
  exit(0);
}
