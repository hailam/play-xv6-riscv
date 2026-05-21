//! Tiny ELF64 loader. Parses Ehdr + Phdr table, iterates PT_LOAD
//! segments, allocates pages from the global frame allocator, copies
//! `p_filesz` bytes from the file image and leaves the rest zero
//! (which `alloc_zeroed` already arranged).

use hal::{FrameAllocator, PageTableOps, PtePerm};

use crate::arch::{Arch, Hal};
use crate::kalloc::KFRAMES;

use crate::arch::PGSIZE;

#[repr(C, packed)]
struct Elf64Hdr {
    e_ident: [u8; 16],
    e_type: u16,
    e_machine: u16,
    e_version: u32,
    e_entry: u64,
    e_phoff: u64,
    e_shoff: u64,
    e_flags: u32,
    e_ehsize: u16,
    e_phentsize: u16,
    e_phnum: u16,
    e_shentsize: u16,
    e_shnum: u16,
    e_shstrndx: u16,
}

#[repr(C, packed)]
struct Elf64Phdr {
    p_type: u32,
    p_flags: u32,
    p_offset: u64,
    p_vaddr: u64,
    p_paddr: u64,
    p_filesz: u64,
    p_memsz: u64,
    p_align: u64,
}

const PT_LOAD: u32 = 1;
const PF_X: u32 = 1;
const PF_W: u32 = 2;
const PF_R: u32 = 4;
const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];

// e_machine value we accept on this build. ELF spec, "Machine
// architecture": EM_RISCV=243, EM_AARCH64=183.
#[cfg(target_arch = "riscv64")]
const EXPECTED_MACHINE: u16 = 243;
#[cfg(target_arch = "aarch64")]
const EXPECTED_MACHINE: u16 = 183;

#[derive(Debug)]
pub enum ElfError {
    NotElf,
    WrongArch,
    Truncated,
    Oom,
    MapFailed,
}

/// Parse `elf` and load every PT_LOAD segment into `pt`. Returns
/// `(entry_point, max_va)` where `max_va` is the page-rounded highest
/// VA touched by any segment (used as `proc.size`).
pub fn load_program(
    pt: &mut <Arch as Hal>::PageTable,
    elf: &[u8],
) -> Result<(usize, usize), ElfError> {
    if elf.len() < core::mem::size_of::<Elf64Hdr>() {
        return Err(ElfError::Truncated);
    }
    if &elf[..4] != ELF_MAGIC.as_slice() {
        return Err(ElfError::NotElf);
    }

    // Read via `read_unaligned` because `elf` is a `&[u8]` from
    // `include_bytes!` — not 8-byte aligned in general.
    let hdr: Elf64Hdr = unsafe {
        core::ptr::read_unaligned(elf.as_ptr() as *const Elf64Hdr)
    };
    let e_machine = hdr.e_machine;
    if e_machine != EXPECTED_MACHINE {
        return Err(ElfError::WrongArch);
    }
    let entry = hdr.e_entry as usize;
    let phoff = hdr.e_phoff as usize;
    let phentsize = hdr.e_phentsize as usize;
    let phnum = hdr.e_phnum as usize;

    let mut max_va: usize = 0;
    for i in 0..phnum {
        let off = phoff + i * phentsize;
        if off + core::mem::size_of::<Elf64Phdr>() > elf.len() {
            return Err(ElfError::Truncated);
        }
        let phdr: Elf64Phdr = unsafe {
            core::ptr::read_unaligned(elf[off..].as_ptr() as *const Elf64Phdr)
        };
        if phdr.p_type != PT_LOAD {
            continue;
        }

        let vaddr = phdr.p_vaddr as usize;
        let filesz = phdr.p_filesz as usize;
        let memsz = phdr.p_memsz as usize;
        let file_off = phdr.p_offset as usize;
        let perm = phdr_perms(phdr.p_flags);

        let seg_start = vaddr & !(PGSIZE - 1);
        let seg_end = (vaddr + memsz + PGSIZE - 1) & !(PGSIZE - 1);

        let mut va = seg_start;
        while va < seg_end {
            let pa = KFRAMES.alloc_zeroed().ok_or(ElfError::Oom)?;
            pt.map(va, pa, PGSIZE, perm, &KFRAMES)
                .map_err(|_| ElfError::MapFailed)?;

            // Copy file bytes into the new page where they overlap
            // with [vaddr, vaddr+filesz). Anything outside is zero from
            // `alloc_zeroed`.
            let page_start = va;
            let page_end = va + PGSIZE;
            let lo = page_start.max(vaddr);
            let hi = page_end.min(vaddr + filesz);
            if hi > lo {
                let dst_off = lo - page_start;
                let src_off = file_off + (lo - vaddr);
                let len = hi - lo;
                if src_off + len > elf.len() {
                    return Err(ElfError::Truncated);
                }
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        elf[src_off..].as_ptr(),
                        (pa + dst_off) as *mut u8,
                        len,
                    );
                }
            }
            va += PGSIZE;
        }

        if seg_end > max_va {
            max_va = seg_end;
        }
    }

    Ok((entry, max_va))
}

fn phdr_perms(p_flags: u32) -> PtePerm {
    let mut bits = PtePerm::USER;
    if p_flags & PF_R != 0 {
        bits |= PtePerm::READ;
    }
    if p_flags & PF_W != 0 {
        bits |= PtePerm::WRITE;
    }
    if p_flags & PF_X != 0 {
        bits |= PtePerm::EXEC;
    }
    PtePerm(bits)
}
