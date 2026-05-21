#![no_std]
#![no_main]

extern crate alloc;

mod arch;
mod console;
mod console_in;
mod cpu;
mod driver;
mod elf;
mod embed;
mod executor;
mod file;
mod fs;
mod heap;
mod kalloc;
mod proc;
mod sync;
mod syscall;
mod time;
mod trap;
mod uapi;
mod user_vm;
mod usertrap;
mod vm;
mod wait;

use core::panic::PanicInfo;
use core::sync::atomic::{AtomicBool, Ordering};

use alloc::sync::Arc;

use crate::arch::{Arch, Hal};
use crate::proc::Proc;

#[cfg(target_arch = "riscv64")]
extern crate hal_riscv64 as _;

#[cfg(target_arch = "aarch64")]
extern crate hal_aarch64 as _;

static STARTED: AtomicBool = AtomicBool::new(false);

#[no_mangle]
pub extern "C" fn kmain() -> ! {
    let id = Arch::hartid();
    if id == 0 {
        // Bring the UART up before the first println — otherwise the
        // boot banner is emitted while the device is disabled and
        // (on aarch64 PL011) QEMU spews "data written to disabled
        // UART" warnings into the trace.
        unsafe { Arch::init_console() };
        println!();
        // "S-mode" on riscv, "EL1" on aarch64 — same semantic level
        // (supervisor). Print a neutral phrase so the line is
        // useful as a milestone on either arch.
        println!("rust kmain (hart 0, supervisor)");
        kalloc::init();
        println!(
            "kalloc: {} free frames ({} MiB)",
            kalloc::KFRAMES.free_count(),
            kalloc::KFRAMES.free_count() * 4 / 1024,
        );
        // Register the global frame-free callback used by
        // `Drop for PageTable` to reclaim user pages on exec/exit.
        unsafe { Arch::install_free_frame(kernel_free_frame) };
        vm::init_and_install();
        println!("kvm: installed (satp={:#x})", vm::kernel_satp());
        unsafe { Arch::init_intc_global() };
        driver::virtio_blk::init();
        driver::bio::init();
        STARTED.store(true, Ordering::Release);
        // Now that paging + interrupts + drivers are up on hart 0,
        // ask secondary harts to come online (no-op on platforms
        // where firmware already started them at `_entry`).
        unsafe { Arch::start_secondary_harts(Arch::ncpus()) };
    } else {
        while !STARTED.load(Ordering::Acquire) {
            core::hint::spin_loop();
        }
        vm::install_on_this_hart();
    }
    cpu::init_this_hart();
    trap::init_this_hart();
    unsafe { Arch::init_intc_per_hart() };
    println!("hart {} up, paging on", id);

    if id == 0 {
        // Pin bringup to hart 0 so the load-balanced fork later can't
        // accidentally land it on a hart whose plumbing isn't up yet.
        executor::spawn_kernel_on(0, || {
            alloc::boxed::Box::pin(bringup_then_init())
        });
    }
    // Every hart runs its own executor loop. Tasks are assigned a
    // sticky home_cpu at spawn time; this loop drains only the local
    // ready queue.
    unsafe { Arch::intr_on() };
    executor::run();
}

async fn bringup_then_init() {
    // Bring the fs up before spawning the init proc — `initcode`
    // immediately calls `exec("/sh", ...)`, which needs `namei` and
    // `readi` ready.
    fs::superblock::init().await;
    let sb = fs::superblock::get();
    fs::log::init(sb.logstart, sb.nlog + 1).await;
    fs::inode::init_cache();
    println!(
        "fs: ready (sb@1, log@{}..{}, inodes@{}..{}, bmap@{}, data@{}..)",
        sb.logstart,
        sb.logstart + 1 + sb.nlog,
        sb.inodestart,
        sb.bmapstart,
        sb.bmapstart,
        sb.bmapstart + 1,
    );

    println!("spawning init proc ({} bytes)", embed::INITCODE.len());
    let init = Arc::new(Proc::new_initcode(embed::INITCODE));
    *init.cwd.lock() = Some(fs::inode::iget(0, 1));
    proc::spawn_proc_main(init);
}

/// Shim handed to `hal_riscv64::pagetable::install_free_frame` so the
/// page-table reaper can return frames to the kernel allocator without
/// hal-riscv64 depending on `crate::kalloc`.
unsafe fn kernel_free_frame(pa: usize) {
    use hal::FrameAllocator;
    unsafe { kalloc::KFRAMES.free(pa) };
}

#[panic_handler]
fn on_panic(info: &PanicInfo) -> ! {
    console::_print_unlocked(core::format_args!("PANIC: {info}\n"));
    loop {
        unsafe { Arch::wfi() }
    }
}
