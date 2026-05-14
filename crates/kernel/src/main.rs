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

static STARTED: AtomicBool = AtomicBool::new(false);

#[no_mangle]
pub extern "C" fn kmain() -> ! {
    let id = Arch::hartid();
    if id == 0 {
        println!();
        println!("rust kmain (hart 0, S-mode)");
        kalloc::init();
        println!(
            "kalloc: {} free frames ({} MiB)",
            kalloc::KFRAMES.free_count(),
            kalloc::KFRAMES.free_count() * 4 / 1024,
        );
        vm::init_and_install();
        println!("kvm: installed (satp={:#x})", vm::kernel_satp());
        hal_riscv64::uart::init();
        hal_riscv64::plic::init();
        driver::virtio_blk::init();
        driver::bio::init();
        STARTED.store(true, Ordering::Release);
    } else {
        while !STARTED.load(Ordering::Acquire) {
            core::hint::spin_loop();
        }
        vm::install_on_this_hart();
    }
    cpu::init_this_hart();
    trap::init_this_hart();
    hal_riscv64::plic::init_for_hart();
    println!("hart {} up, paging on", id);

    if id == 0 {
        // Spawn an async kernel task that exercises the async virtio path.
        // It runs concurrently with the init proc; the executor interleaves
        // them via the disk IRQ waker.
        executor::spawn_kernel(|| alloc::boxed::Box::pin(disk_smoke_test()));

        println!("spawning init proc ({} bytes)", embed::INITCODE.len());
        let init = Arc::new(Proc::new_initcode(embed::INITCODE));
        proc::spawn_proc_main(init);
        unsafe { Arch::intr_on() };
        executor::run();
    }

    unsafe { Arch::intr_on() };
    loop {
        unsafe { Arch::wfi() }
    }
}

async fn disk_smoke_test() {
    let report = |stage: &str| {
        println!(
            "bio test ({}): {} I/Os submitted",
            stage,
            driver::virtio_blk::IO_COUNT.load(Ordering::Relaxed)
        );
    };

    // Read block 0 twice — second is a cache hit.
    {
        let _b1 = driver::bio::bread(0).await;
        let _b2 = driver::bio::bread(0).await;
    }
    report("after 2 reads of block 0");

    // Now read 64 distinct blocks. With NBUF=32 and the LRU evictor
    // running, the cache stays bounded; reads issue ~64 I/Os total.
    for blk in 0..64 {
        let _b = driver::bio::bread(blk).await;
    }
    report("after reading blocks 0..64");

    // Re-read block 63 — should be a hit (most recent).
    {
        let _b = driver::bio::bread(63).await;
    }
    report("after re-reading block 63 (expect hit)");

    core::future::pending::<()>().await;
}

#[panic_handler]
fn on_panic(info: &PanicInfo) -> ! {
    console::_print_unlocked(core::format_args!("PANIC: {info}\n"));
    loop {
        unsafe { Arch::wfi() }
    }
}
