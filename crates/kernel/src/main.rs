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
    let dump = |label: &str, data: &[u8]| {
        print!(
            "{} (after {} I/Os): ",
            label,
            driver::virtio_blk::IO_COUNT.load(Ordering::Relaxed)
        );
        for &b in &data[..24] {
            let c = if (b' '..=b'~').contains(&b) {
                b as char
            } else if b == b'\n' {
                '_'
            } else {
                '.'
            };
            print!("{}", c);
        }
        println!();
    };

    let b1 = driver::bio::bread(0).await;
    dump("bio block 0 #1", b1.data());
    drop(b1);

    let b2 = driver::bio::bread(0).await;
    dump("bio block 0 #2", b2.data());
    drop(b2);

    // Park forever.
    core::future::pending::<()>().await;
}

#[panic_handler]
fn on_panic(info: &PanicInfo) -> ! {
    console::_print_unlocked(core::format_args!("PANIC: {info}\n"));
    loop {
        unsafe { Arch::wfi() }
    }
}
