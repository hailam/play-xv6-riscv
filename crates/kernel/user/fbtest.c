//
// fbtest — draw to /dev/fb0 from userspace (todo 12 M2).
//
// Queries the framebuffer geometry via ioctl(FBIOGET_DIMS), then
// paints a pattern DISTINCT from the kernel's boot test pattern
// (a magenta field with a yellow horizontal band) by seeking to each
// row and writing pixels. A host-side screendump confirms userspace
// reached the scanout. Stays alive so the harness can screendump.
//
#include "user.h"

#define FBIOGET_DIMS 0x4600

struct fb_dims { uint width, height, stride, bpp; };

int
main(void)
{
  int fd = open("/dev/fb0", O_RDWR);
  if(fd < 0){ printf("fbtest: open /dev/fb0 failed\n"); exit(1); }

  struct fb_dims d;
  if(ioctl(fd, FBIOGET_DIMS, &d) < 0){
    printf("fbtest: FBIOGET_DIMS failed\n"); exit(1);
  }
  printf("fbtest: %dx%d stride=%d bpp=%d\n", d.width, d.height, d.stride, d.bpp);
  if(d.bpp != 32){ printf("fbtest: unexpected bpp\n"); exit(1); }

  // One row of XRGB8888 pixels. 640*4 = 2560 bytes.
  static uint row[1024];
  for(uint y = 0; y < d.height; y++){
    uint color = (y >= d.height/2 - 20 && y < d.height/2 + 20)
                   ? 0x00FFFF00u   // yellow band through the middle
                   : 0x00FF00FFu;  // magenta field
    for(uint x = 0; x < d.width; x++)
      row[x] = color;
    // Seek to the start of row y, write the row.
    if(lseek(fd, (long)(y * d.stride), 0) < 0){
      printf("fbtest: lseek failed at row %d\n", y); exit(1);
    }
    int want = (int)(d.width * 4);
    if(write(fd, row, want) != want){
      printf("fbtest: short write at row %d\n", y); exit(1);
    }
  }
  printf("fbtest: drew\n");
  // Stay alive briefly so a screendump catches the result; the
  // harness kills qemu.
  for(;;) pause();
}
