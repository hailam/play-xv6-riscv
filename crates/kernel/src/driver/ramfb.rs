//! ramfb framebuffer driver, configured through QEMU's fw_cfg device
//! over its MMIO + DMA interface (todo 12 M1).
//!
//! ramfb is a "dumb" framebuffer: once we hand QEMU a config blob
//! (framebuffer physical address + geometry + DRM fourcc), QEMU scans
//! that guest RAM out continuously — there is no flush/scanout command,
//! the kernel just writes pixels into the buffer and they appear.
//!
//! Probe-and-disable: if fw_cfg's signature isn't "QEMU" or the DMA
//! feature bit is clear (or there's no `etc/ramfb` file because qemu
//! wasn't given `-device ramfb`), `init` returns false and the kernel
//! runs headless as before.
//!
//! Endianness trap: EVERYTHING on the fw_cfg control path — the
//! selector register, the DMA address register, the `FwCfgDmaAccess`
//! struct, and the `RamfbCfg` blob — is BIG-ENDIAN. The framebuffer
//! pixels themselves are native little-endian XRGB8888 (`0x00RRGGBB`).

use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};
use core::sync::atomic::{fence, AtomicBool, AtomicU32, Ordering};

use hal::Hal;

use crate::arch::Arch;

const BASE: usize = <Arch as Hal>::FWCFG;
const PGSIZE: usize = 4096;

// fw_cfg MMIO register offsets (see docs/specs/fw_cfg).
const REG_DATA: usize = 0x0; // 8 wide; we read 1 byte at a time
const REG_SELECTOR: usize = 0x8; // 2 wide, big-endian
const REG_DMA: usize = 0x10; // 8 wide, big-endian

// Fixed selector keys.
const FW_CFG_SIGNATURE: u16 = 0x0000;
const FW_CFG_ID: u16 = 0x0001;
const FW_CFG_FILE_DIR: u16 = 0x0019;

// DMA control flags.
const DMA_ERROR: u32 = 0x01;
const DMA_SELECT: u32 = 0x08;
const DMA_WRITE: u32 = 0x10;

// DRM_FORMAT_XRGB8888 — 32bpp, little-endian word 0x00RRGGBB.
const DRM_FORMAT_XRGB8888: u32 = 0x3432_5258;

/// Width/height we request. 640x480x4 = 1.2 MiB ≈ 300 frames.
pub const WIDTH: usize = 640;
pub const HEIGHT: usize = 480;
const BPP: usize = 4;
const FB_BYTES: usize = WIDTH * HEIGHT * BPP;

// The framebuffer lives in kernel BSS: page-aligned, physically
// contiguous by construction, and identity-mapped (VA == PA) so its
// address is the phys addr QEMU scans out. (The page-frame allocator
// is a LIFO free list that can't promise a 300-page contiguous run.)
#[repr(C, align(4096))]
struct FrameBuf([u32; WIDTH * HEIGHT]);
static mut FRAMEBUF: FrameBuf = FrameBuf([0; WIDTH * HEIGHT]);

#[repr(C, packed)]
struct FwCfgDmaAccess {
    control: u32, // big-endian
    length: u32,  // big-endian
    address: u64, // big-endian — phys addr of the data buffer
}

#[repr(C, packed)]
struct RamfbCfg {
    addr: u64,   // big-endian — phys addr of the framebuffer
    fourcc: u32, // big-endian
    flags: u32,  // big-endian
    width: u32,  // big-endian
    height: u32, // big-endian
    stride: u32, // big-endian
}

// Static DMA scratch (identity-mapped, so its address IS its phys
// addr). Only the boot path touches these, single-threaded.
static mut DMA: FwCfgDmaAccess = FwCfgDmaAccess { control: 0, length: 0, address: 0 };
static mut CFG: RamfbCfg =
    RamfbCfg { addr: 0, fourcc: 0, flags: 0, width: 0, height: 0, stride: 0 };

static PRESENT: AtomicBool = AtomicBool::new(false);
static STRIDE: AtomicU32 = AtomicU32::new(0);

#[inline]
unsafe fn sel(key: u16) {
    // Selector register is 16-bit big-endian.
    write_volatile((BASE + REG_SELECTOR) as *mut u16, key.to_be());
}

#[inline]
unsafe fn read_data_byte() -> u8 {
    read_volatile((BASE + REG_DATA) as *const u8)
}

/// Read `n` bytes of the currently-selected item into `buf`.
unsafe fn read_data(buf: &mut [u8]) {
    for b in buf.iter_mut() {
        *b = read_data_byte();
    }
}

/// Issue a DMA WRITE of `len` bytes from `src_pa` into the fw_cfg item
/// `key`, and poll for completion. Returns false on error.
unsafe fn dma_write(key: u16, src_pa: usize, len: u32) -> bool {
    let dma = addr_of_mut!(DMA);
    (*dma).control =
        (((key as u32) << 16) | DMA_SELECT | DMA_WRITE).to_be();
    (*dma).length = len.to_be();
    (*dma).address = (src_pa as u64).to_be();
    fence(Ordering::SeqCst);
    // Trigger: write the big-endian phys addr of the DMA struct to the
    // 64-bit DMA register.
    write_volatile((BASE + REG_DMA) as *mut u64, (dma as u64).to_be());
    fence(Ordering::SeqCst);
    // Poll: QEMU clears control to 0 on success, or sets the ERROR bit.
    loop {
        let ctl = u32::from_be(read_volatile(addr_of!((*dma).control)));
        if ctl == 0 {
            return true;
        }
        if ctl & DMA_ERROR != 0 {
            return false;
        }
        core::hint::spin_loop();
    }
}

/// Scan FW_CFG_FILE_DIR for `name`; return its selector key + size.
unsafe fn find_file(name: &[u8]) -> Option<(u16, u32)> {
    sel(FW_CFG_FILE_DIR);
    let mut count_be = [0u8; 4];
    read_data(&mut count_be);
    let count = u32::from_be_bytes(count_be);
    for _ in 0..count {
        // FWCfgFile: be32 size, be16 select, be16 reserved, [u8;56] name.
        let mut entry = [0u8; 64];
        read_data(&mut entry);
        let size = u32::from_be_bytes(entry[0..4].try_into().unwrap());
        let select = u16::from_be_bytes(entry[4..6].try_into().unwrap());
        let nbytes = &entry[8..64];
        let nlen = nbytes.iter().position(|&b| b == 0).unwrap_or(56);
        if &nbytes[..nlen] == name {
            return Some((select, size));
        }
    }
    None
}

/// Probe fw_cfg + ramfb and configure the scanout. Returns true on
/// success (a usable framebuffer is now live).
pub fn init() -> bool {
    unsafe {
        // Signature must read "QEMU".
        sel(FW_CFG_SIGNATURE);
        let mut sig = [0u8; 4];
        read_data(&mut sig);
        if &sig != b"QEMU" {
            return false;
        }
        // DMA interface bit (this item is little-endian).
        sel(FW_CFG_ID);
        let mut id = [0u8; 4];
        read_data(&mut id);
        let features = u32::from_le_bytes(id);
        if features & 0x02 == 0 {
            return false;
        }
        // Locate etc/ramfb (only present with `-device ramfb`).
        let Some((key, size)) = find_file(b"etc/ramfb") else {
            return false;
        };
        if (size as usize) < core::mem::size_of::<RamfbCfg>() {
            return false;
        }
        // Allocate the framebuffer from page frames (identity-mapped).
        // Frames are 4 KiB; grab a contiguous run.
        let pages = FB_BYTES.div_ceil(PGSIZE);
        let _ = pages;
        let fb_pa = addr_of!(FRAMEBUF) as usize;
        // Build the (big-endian) config blob and DMA-write it.
        let cfg = addr_of_mut!(CFG);
        (*cfg).addr = (fb_pa as u64).to_be();
        (*cfg).fourcc = DRM_FORMAT_XRGB8888.to_be();
        (*cfg).flags = 0u32.to_be();
        (*cfg).width = (WIDTH as u32).to_be();
        (*cfg).height = (HEIGHT as u32).to_be();
        (*cfg).stride = ((WIDTH * BPP) as u32).to_be();
        fence(Ordering::SeqCst);
        if !dma_write(key, cfg as usize, core::mem::size_of::<RamfbCfg>() as u32) {
            return false;
        }
        STRIDE.store((WIDTH * BPP) as u32, Ordering::Release);
        PRESENT.store(true, Ordering::Release);
        crate::println!("ramfb: ready ({}x{} xrgb8888 @ {:#x})", WIDTH, HEIGHT, fb_pa);
        true
    }
}

pub fn present() -> bool {
    PRESENT.load(Ordering::Acquire)
}

pub fn dims() -> (usize, usize, usize) {
    (WIDTH, HEIGHT, STRIDE.load(Ordering::Acquire) as usize)
}

/// Total framebuffer size in bytes (for /dev/fb0's EOF / lseek-end).
pub fn size_bytes() -> usize {
    FB_BYTES
}

/// The framebuffer as a raw byte slice — for /dev/fb0 read/write at a
/// byte offset. None if ramfb isn't up.
pub fn bytes() -> Option<&'static mut [u8]> {
    if !present() {
        return None;
    }
    Some(unsafe {
        core::slice::from_raw_parts_mut(addr_of_mut!(FRAMEBUF) as *mut u8, FB_BYTES)
    })
}

/// Raw framebuffer as a mutable u32 slice (one XRGB8888 pixel each).
/// Returns None if ramfb didn't come up. The caller owns
/// synchronization — slice 1 only writes from a single kernel path.
pub fn framebuffer() -> Option<&'static mut [u32]> {
    if !present() {
        return None;
    }
    Some(unsafe { &mut (*addr_of_mut!(FRAMEBUF)).0 })
}

/// Fill the whole framebuffer with one XRGB8888 color.
pub fn clear(color: u32) {
    if let Some(fb) = framebuffer() {
        fb.fill(color);
    }
}

/// Draw a recognizable boot test pattern: four solid quadrants
/// (red / green / blue / white) so a host-side screendump can verify
/// pixels at known coordinates.
pub fn draw_test_pattern() {
    let Some(fb) = framebuffer() else { return };
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let top = y < HEIGHT / 2;
            let left = x < WIDTH / 2;
            let px: u32 = match (top, left) {
                (true, true) => 0x00FF_0000,   // red
                (true, false) => 0x0000_FF00,  // green
                (false, true) => 0x0000_00FF,  // blue
                (false, false) => 0x00FF_FFFF, // white
            };
            fb[y * WIDTH + x] = px;
        }
    }
}
