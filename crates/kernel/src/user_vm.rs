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

use crate::arch::{trampoline_pa, PGSIZE, TRAMPOLINE, TRAPFRAME};

pub const STACK_VA_TOP: usize = TRAPFRAME;
/// Number of pages in the fixed user stack region. 8 pages (32 KiB)
/// is enough for moderately recursive interpreters (lua, awk) and
/// xv6's usertests; single-page was the original xv6 size but
/// blows up immediately under any picolibc-stdio + libm code path.
pub const STACK_PAGES: usize = 8;
pub const STACK_VA_BASE: usize = STACK_VA_TOP - STACK_PAGES * PGSIZE;
/// VA of the topmost (highest-address) stack page. argv/envp layout
/// always lands in this page.
pub const STACK_TOP_PAGE_VA: usize = STACK_VA_TOP - PGSIZE;
/// Top of the mmap region. Leave a 16 MiB guard below the stack
/// page so a wild sbrk into the stack-guard area isn't immediately
/// indistinguishable from an mmap region.
pub const MMAP_TOP: usize = STACK_VA_BASE - 16 * 1024 * 1024;

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
    // ElfError payload is for `Debug` output on exec failure.
    #[allow(dead_code)]
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

    // Allocate STACK_PAGES separate physical frames, map them
    // consecutively into [STACK_VA_BASE, STACK_VA_TOP). The TOP page
    // is what argv/envp layout uses; lower pages give us recursion
    // headroom.
    let mut top_stack_pa: usize = 0;
    for i in 0..STACK_PAGES {
        let pa = KFRAMES.alloc_zeroed().ok_or(UserVmError::Oom)?;
        let va = STACK_VA_BASE + i * PGSIZE;
        pt.map(va, pa, PGSIZE, PtePerm::URW, &KFRAMES)
            .map_err(|_| UserVmError::MapFailed)?;
        if i == STACK_PAGES - 1 {
            top_stack_pa = pa;
        }
    }
    debug_assert!(top_stack_pa != 0);

    Ok(UserImage {
        pagetable: pt,
        entry,
        code_end,
        stack_pa: top_stack_pa,
    })
}

/// Push the argv/argc layout near the top of the stack page. Returns
/// `(initial_sp_va, argv_array_va)`.
pub fn place_argv_on_stack(stack_pa: usize, argv: &[String]) -> (usize, usize) {
    let (sp, argv_va, _) = place_argv_envp_on_stack(stack_pa, argv, &[]);
    (sp, argv_va)
}

/// Lay out (argc, argv\[\], envp\[\]) + their backing strings near
/// the top of the stack page. Returns `(initial_sp_va, argv_array_va,
/// envp_array_va)`. envp_array_va == 0 when envp is empty.
///
/// Stack layout (low → high):
///   sp_va        : argc (u64)
///   sp_va + 8    : argv[0..argc+1] (NULL-terminated)
///   ...          : envp[0..n+1]    (NULL-terminated; absent if empty)
///   ...          : argv strings  (NUL-terminated, packed)
///   ...          : envp strings
///   STACK_VA_TOP : end
pub fn place_argv_envp_on_stack(
    stack_pa: usize,
    argv: &[String],
    envp: &[String],
) -> (usize, usize, usize) {
    let argc = argv.len();
    let envc = envp.len();
    let argv_strings: usize = argv.iter().map(|s| s.len() + 1).sum();
    let envp_strings: usize = envp.iter().map(|s| s.len() + 1).sum();
    let strings_aligned = (argv_strings + envp_strings + 7) & !7;
    let argv_array_bytes = 8 * (argc + 1);
    let envp_array_bytes = if envc == 0 { 0 } else { 8 * (envc + 1) };
    let total = 8 + argv_array_bytes + envp_array_bytes + strings_aligned;
    assert!(total < PGSIZE, "argv/envp layout doesn't fit in one stack page");

    let sp_va = STACK_VA_TOP - total;
    let argv_array_va = sp_va + 8;
    let envp_array_va = if envc == 0 { 0 } else { argv_array_va + argv_array_bytes };
    let strings_start_va = sp_va + 8 + argv_array_bytes + envp_array_bytes;

    // argv/envp layout always lives in the topmost stack page; that
    // page's PA is what the caller hands us as `stack_pa`. Offset
    // from the TOP-page VA base, not from STACK_VA_BASE (which now
    // points to the lowest of N pages and would underflow).
    let sp_off = sp_va - STACK_TOP_PAGE_VA;
    let argv_off = argv_array_va - STACK_TOP_PAGE_VA;
    let envp_off = if envp_array_va == 0 {
        0
    } else {
        envp_array_va - STACK_TOP_PAGE_VA
    };
    let strings_off = strings_start_va - STACK_TOP_PAGE_VA;

    unsafe {
        core::ptr::write_unaligned((stack_pa + sp_off) as *mut u64, argc as u64);
    }

    // argv strings + array
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

    // envp strings + array (after argv strings)
    if envc != 0 {
        for (i, s) in envp.iter().enumerate() {
            unsafe {
                core::ptr::copy_nonoverlapping(
                    s.as_ptr(),
                    (stack_pa + str_off) as *mut u8,
                    s.len(),
                );
                *((stack_pa + str_off + s.len()) as *mut u8) = 0;
                core::ptr::write_unaligned(
                    (stack_pa + envp_off + i * 8) as *mut u64,
                    str_va as u64,
                );
            }
            str_off += s.len() + 1;
            str_va += s.len() + 1;
        }
        unsafe {
            core::ptr::write_unaligned(
                (stack_pa + envp_off + envc * 8) as *mut u64,
                0,
            );
        }
    }

    (sp_va, argv_array_va, envp_array_va)
}
