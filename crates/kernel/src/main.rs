#![no_std]
#![no_main]

extern crate alloc;

mod arch;
mod console;
mod cpu;
mod executor;
mod heap;
mod kalloc;
mod proc;
mod sync;
mod syscall;
mod trap;
mod uapi;
mod usertrap;
mod vm;

use core::panic::PanicInfo;
use core::sync::atomic::{AtomicBool, Ordering};

use alloc::sync::Arc;

use crate::arch::{Arch, Hal};
use crate::proc::Proc;

#[cfg(target_arch = "riscv64")]
extern crate hal_riscv64 as _;

const INITCODE: &[u8] = include_bytes!(env!("INITCODE_BIN_PATH"));

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
        STARTED.store(true, Ordering::Release);
    } else {
        while !STARTED.load(Ordering::Acquire) {
            core::hint::spin_loop();
        }
        vm::install_on_this_hart();
    }
    cpu::init_this_hart();
    trap::init_this_hart();
    println!("hart {} up, paging on", id);

    if id == 0 {
        println!("spawning init proc ({} bytes)", INITCODE.len());
        let init = Arc::new(Proc::new_initcode(INITCODE));
        proc::spawn_proc_main(init);
        unsafe { Arch::intr_on() };
        executor::run();
    }

    unsafe { Arch::intr_on() };
    loop {
        unsafe { Arch::wfi() }
    }
}

#[panic_handler]
fn on_panic(info: &PanicInfo) -> ! {
    console::_print_unlocked(core::format_args!("PANIC: {info}\n"));
    loop {
        unsafe { Arch::wfi() }
    }
}
