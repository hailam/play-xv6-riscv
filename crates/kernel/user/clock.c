//
// clock — demo client (todo 12 M4): a window that cycles its color
// roughly every second, proving multi-window compositing + that a
// second client animates independently.
//
#include "user.h"
#include "wm.h"

#define W 120
#define H 90

static uint px[W];

int
main(void)
{
  int s = socket(1, 1, 0);
  if(s < 0 || connect(s, WM_SOCK) < 0){
    printf("clock: connect failed\n");
    exit(1);
  }
  uint msg[4] = { WM_CREATE, W, H, 0 }; // no keys
  writen(s, msg, sizeof(msg));
  uint id;
  if(readn(s, &id, 4) != 4){
    printf("clock: create failed\n");
    exit(1);
  }
  static const uint palette[4] =
    { 0x00FFA500u, 0x00800080u, 0x00008000u, 0x00FFC0CBu };
  printf("clock: ticking\n");
  for(uint t = 0;; t++){
    uint hdr[5] = { WM_BLIT, 0, 0, W, H };
    writen(s, hdr, sizeof(hdr));
    for(int i = 0; i < W; i++)
      px[i] = palette[t % 4];
    for(int r = 0; r < H; r++)
      writen(s, px, sizeof(px));
    sleep(10); // ~1s
  }
}
