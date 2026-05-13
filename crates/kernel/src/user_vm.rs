//! User-image construction: build a fresh user pagetable from an ELF,
//! attach a stack page at a fixed VA, and lay out `argc`/`argv`/strings
//! on that stack.
//!
//! Layout (post-exec):
//!   * `[0..max_va)`       — code/data/bss from ELF PT_LOAD segments
//!   * `[STACK_BASE..STACK_TOP)` — one user stack page
//!   * `TRAPFRAME`         — trap frame (kernel writes user regs here)
//!   * `TRAMPOLINE`        — trap entry/exit code (shared)
//!
//! `STACK_TOP == TRAPFRAME`. The stack ends right where the trapframe
//! begins; the stack page is the one immediately below TRAPFRAME.

use alloc::string::String;

use hal::{FrameAllocator, PageTableOps, PtePerm};

use crate::arch::{Arch, Hal};
use crate::elf;
use crate::kalloc::KFRAMES;

#[cfg(target_arch = "riscv64")]
use hal_riscv64::{
    memlayout::{PGSIZE, TRAMPOLINE, TRAPFRAME},
    trampoline_pa,
};

pub const STACK_VA_TOP: usize = TRAPFRAME;
pub const STACK_VA_BASE: usize = STACK_VA_TOP - PGSIZE;

pub struct UserImage {
    pub pagetable: <Arch as Hal>::PageTable,
    pub entry: usize,
    pub code_end: usize,
    pub stack_pa: usize,
}

#[derive(Debug)]
pub enum UserVmError {
    Oom,
    MapFailed,
    Elf(elf::ElfError),
}

/// Allocate a fresh user pagetable, install trampoline + trapframe +
/// stack mappings, load the ELF segments. Returns the built image.
pub fn build_image_from_elf(
    elf_bytes: &[u8],
    trapframe_pa: usize,
) -> Result<UserImage, UserVmError> {
    let mut pt = <Arch as Hal>::PageTable::new(&KFRAMES).map_err(|_| UserVmError::Oom)?;
    pt.map(TRAMPOLINE, trampoline_pa(), PGSIZE, PtePerm::RX, &KFRAMES)
        .map_err(|_| UserVmError::MapFailed)?;
    pt.map(TRAPFRAME, trapframe_pa, PGSIZE, PtePerm::RW, &KFRAMES)
        .map_err(|_| UserVmError::MapFailed)?;

    let (entry, code_end) =
        elf::load_program(&mut pt, elf_bytes).map_err(UserVmError::Elf)?;

    let stack_pa = KFRAMES.alloc_zeroed().ok_or(UserVmError::Oom)?;
    pt.map(STACK_VA_BASE, stack_pa, PGSIZE, PtePerm::URW, &KFRAMES)
        .map_err(|_| UserVmError::MapFailed)?;

    Ok(UserImage {
        pagetable: pt,
        entry,
        code_end,
        stack_pa,
    })
}

/// Push the argv/argc layout near the top of the stack page. Returns
/// `(initial_sp_va, argv_array_va)`.
pub fn place_argv_on_stack(stack_pa: usize, argv: &[String]) -> (usize, usize) {
    let argc = argv.len();
    let strings_bytes: usize = argv.iter().map(|s| s.len() + 1).sum();
    let strings_aligned = (strings_bytes + 7) & !7;
    let argv_bytes = 8 * (argc + 1);
    let total = 8 + argv_bytes + strings_aligned;
    assert!(total < PGSIZE, "argv layout doesn't fit in one stack page");

    let sp_va = STACK_VA_TOP - total;
    let argv_array_va = sp_va + 8;
    let strings_start_va = sp_va + 8 + argv_bytes;

    let sp_off = sp_va - STACK_VA_BASE;
    let argv_off = argv_array_va - STACK_VA_BASE;
    let strings_off = strings_start_va - STACK_VA_BASE;

    unsafe {
        core::ptr::write_unaligned((stack_pa + sp_off) as *mut u64, argc as u64);
    }

    let mut str_off = strings_off;
    let mut str_va = strings_start_va;
    for (i, s) in argv.iter().enumerate() {
        unsafe {
            core::ptr::copy_nonoverlapping(
                s.as_ptr(),
                (stack_pa + str_off) as *mut u8,
                s.len(),
            );
            *((stack_pa + str_off + s.len()) as *mut u8) = 0;
            core::ptr::write_unaligned(
                (stack_pa + argv_off + i * 8) as *mut u64,
                str_va as u64,
            );
        }
        str_off += s.len() + 1;
        str_va += s.len() + 1;
    }
    unsafe {
        core::ptr::write_unaligned(
            (stack_pa + argv_off + argc * 8) as *mut u64,
            0,
        );
    }

    (sp_va, argv_array_va)
}
