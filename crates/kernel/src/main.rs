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
    // 1. Superblock.
    fs::superblock::init().await;
    let sb = fs::superblock::get();
    println!(
        "fs: superblock OK (size={}, log@{}, inodes@{}, bmap@{})",
        sb.size, sb.logstart, sb.inodestart, sb.bmapstart
    );

    // 2. Log uses superblock now.
    fs::log::init(sb.logstart, sb.nlog + 1).await;

    // 3. Inode cache.
    fs::inode::init_cache();

    // 4. List the root directory.
    let root = fs::inode::iget(0, 1);
    let rli = fs::inode::ilock(&root).await;
    println!("fs: / contents ({} bytes of dirents):", rli.state().size);
    fs::dir::for_each_entry(&rli, |inum, name| {
        println!("  inum {:3}  {}", inum, name);
    })
    .await;
    drop(rli);

    // 5. Resolve /echo, read its first 32 bytes, verify ELF magic.
    let ip = fs::namei("/echo").await.expect("namei /echo");
    let li = fs::inode::ilock(&ip).await;
    let typ = li.state().typ;
    let size = li.state().size;
    let mut head = [0u8; 32];
    let n = fs::inode::readi(&li, &mut head, 0).await;
    drop(li);

    println!("fs: /echo typ={} size={} bytes, read {}", typ, size, n);
    print!("     first 16 bytes:");
    for &b in &head[..16] {
        print!(" {:02x}", b);
    }
    println!();
    if &head[..4] == b"\x7fELF" {
        println!("fs: ELF magic confirmed — exec-from-disk path is unblocked!");
    } else {
        println!("fs: WARNING — no ELF magic at /echo offset 0");
    }

    core::future::pending::<()>().await;
}

#[panic_handler]
fn on_panic(info: &PanicInfo) -> ! {
    console::_print_unlocked(core::format_args!("PANIC: {info}\n"));
    loop {
        unsafe { Arch::wfi() }
    }
}
