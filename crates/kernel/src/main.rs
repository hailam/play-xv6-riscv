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
    let count = || driver::virtio_blk::IO_COUNT.load(Ordering::Relaxed);

    // Bring the log up. Hardcoded layout for now (replaced when fs
    // lands and we read these from a superblock):
    //   block 0  — unused
    //   block 1  — (future) superblock
    //   block 2  — log header
    //   blocks 3..33 — log data slots
    //   blocks 33..  — free space (where our test writes go)
    fs::log::init(2, 31).await;

    // --- Transaction: atomically update blocks 300 and 301 ---------
    let pre_tx = count();
    fs::log::begin_op().await;
    {
        let b = driver::bio::bread(300).await;
        unsafe {
            b.data_mut()[..10].copy_from_slice(b"TX-BLOCK-A");
        }
        fs::log::log_write(&b);
    }
    {
        let b = driver::bio::bread(301).await;
        unsafe {
            b.data_mut()[..10].copy_from_slice(b"TX-BLOCK-B");
        }
        fs::log::log_write(&b);
    }
    fs::log::end_op().await;
    println!(
        "log: 2-block transaction committed ({} I/Os: 2 log writes + 1 header + 2 home writes + 1 clear)",
        count() - pre_tx
    );

    // --- Force eviction so the next reads come from disk -----------
    for blk in 500..540 {
        let _b = driver::bio::bread(blk).await;
    }

    // --- Verify both blocks landed on disk -------------------------
    let dump = |label: &str, data: &[u8]| {
        print!("{}: ", label);
        for &c in &data[..10] {
            if (b' '..=b'~').contains(&c) {
                print!("{}", c as char);
            } else {
                print!(".");
            }
        }
        println!();
    };

    let b1 = driver::bio::bread(300).await;
    dump("block 300", b1.data());
    let b2 = driver::bio::bread(301).await;
    dump("block 301", b2.data());

    core::future::pending::<()>().await;
}

#[panic_handler]
fn on_panic(info: &PanicInfo) -> ! {
    console::_print_unlocked(core::format_args!("PANIC: {info}\n"));
    loop {
        unsafe { Arch::wfi() }
    }
}
