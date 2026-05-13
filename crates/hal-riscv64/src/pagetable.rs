//! Sv39 three-level page table.
//!
//! Virtual address layout (Sv39):
//!   [38:30] L2 index (9 bits)
//!   [29:21] L1 index (9 bits)
//!   [20:12] L0 index (9 bits)
//!   [11:0]  page offset
//!
//! PTE layout (64 bits):
//!   [9:0]  flags (V/R/W/X/U/G/A/D + 2 RSW)
//!   [53:10] PPN  (44 bits)

use hal::{FrameAllocator, PageTableOps, PtePerm, VmError};

use crate::memlayout::{MAXVA, PGSIZE};

const PTE_V: u64 = 1 << 0;
const PTE_R: u64 = 1 << 1;
const PTE_W: u64 = 1 << 2;
const PTE_X: u64 = 1 << 3;
const PTE_U: u64 = 1 << 4;

#[derive(Copy, Clone)]
#[repr(transparent)]
struct Pte(u64);

impl Pte {
    const fn empty() -> Self {
        Pte(0)
    }
    fn is_valid(self) -> bool {
        (self.0 & PTE_V) != 0
    }
    fn is_leaf(self) -> bool {
        self.is_valid() && (self.0 & (PTE_R | PTE_W | PTE_X)) != 0
    }
    fn pa(self) -> u64 {
        ((self.0 >> 10) & ((1u64 << 44) - 1)) << 12
    }
    fn flags_perm(self) -> PtePerm {
        let mut p = 0u32;
        if (self.0 & PTE_R) != 0 {
            p |= PtePerm::READ;
        }
        if (self.0 & PTE_W) != 0 {
            p |= PtePerm::WRITE;
        }
        if (self.0 & PTE_X) != 0 {
            p |= PtePerm::EXEC;
        }
        if (self.0 & PTE_U) != 0 {
            p |= PtePerm::USER;
        }
        PtePerm(p)
    }
    fn make_leaf(pa: u64, perm: PtePerm) -> Self {
        let mut bits = PTE_V;
        if perm.0 & PtePerm::READ != 0 {
            bits |= PTE_R;
        }
        if perm.0 & PtePerm::WRITE != 0 {
            bits |= PTE_W;
        }
        if perm.0 & PtePerm::EXEC != 0 {
            bits |= PTE_X;
        }
        if perm.0 & PtePerm::USER != 0 {
            bits |= PTE_U;
        }
        Pte(((pa >> 12) << 10) | bits)
    }
    fn make_branch(pa: u64) -> Self {
        Pte(((pa >> 12) << 10) | PTE_V)
    }
}

/// Owned Sv39 root page table. Drop frees the entire tree.
pub struct PageTable {
    root_pa: usize,
}

impl PageTable {
    /// Physical address of the root pagetable.
    pub fn root_pa(&self) -> usize {
        self.root_pa
    }

    fn root_ptr(&self) -> *mut [Pte; 512] {
        self.root_pa as *mut [Pte; 512]
    }

    /// Walks (and optionally allocates) to the leaf PTE for `va`. Returns
    /// a raw pointer because the lifetime of the table memory is governed
    /// by the allocator; we only dereference under `unsafe` near use.
    fn walk(
        &mut self,
        va: usize,
        alloc: Option<&dyn FrameAllocator>,
    ) -> Option<*mut Pte> {
        if va >= MAXVA {
            return None;
        }
        let mut table: *mut [Pte; 512] = self.root_ptr();
        for level in (1..=2).rev() {
            let idx = (va >> (12 + 9 * level)) & 0x1ff;
            // Safety: `table` is page-aligned and 4 KiB long; idx < 512.
            let pte_ptr = unsafe { (*table).as_mut_ptr().add(idx) };
            let pte = unsafe { core::ptr::read(pte_ptr) };
            if pte.is_valid() {
                table = pte.pa() as *mut [Pte; 512];
            } else if let Some(a) = alloc {
                let new_pa = a.alloc_zeroed()?;
                let new_pte = Pte::make_branch(new_pa as u64);
                unsafe { core::ptr::write(pte_ptr, new_pte) };
                table = new_pa as *mut [Pte; 512];
            } else {
                return None;
            }
        }
        let idx = (va >> 12) & 0x1ff;
        Some(unsafe { (*table).as_mut_ptr().add(idx) })
    }
}

impl PageTableOps for PageTable {
    fn new(alloc: &dyn FrameAllocator) -> Result<Self, VmError> {
        let pa = alloc.alloc_zeroed().ok_or(VmError::Oom)?;
        Ok(Self { root_pa: pa })
    }

    fn map(
        &mut self,
        va: usize,
        pa: usize,
        size: usize,
        perm: PtePerm,
        alloc: &dyn FrameAllocator,
    ) -> Result<(), VmError> {
        if va % PGSIZE != 0 || pa % PGSIZE != 0 || size % PGSIZE != 0 || size == 0 {
            return Err(VmError::Misaligned);
        }
        let mut va = va;
        let mut pa = pa;
        let end = va.checked_add(size).ok_or(VmError::OutOfRange)?;
        if end > MAXVA {
            return Err(VmError::OutOfRange);
        }
        while va < end {
            let pte_ptr = self.walk(va, Some(alloc)).ok_or(VmError::Oom)?;
            // Safety: walk returned Some => pte_ptr is valid.
            let existing = unsafe { core::ptr::read(pte_ptr) };
            if existing.is_valid() {
                return Err(VmError::Remap);
            }
            unsafe { core::ptr::write(pte_ptr, Pte::make_leaf(pa as u64, perm)) };
            va += PGSIZE;
            pa += PGSIZE;
        }
        Ok(())
    }

    fn translate(&self, va: usize) -> Option<(usize, PtePerm)> {
        if va >= MAXVA {
            return None;
        }
        let mut table: *mut [Pte; 512] = self.root_pa as *mut _;
        for level in (1..=2).rev() {
            let idx = (va >> (12 + 9 * level)) & 0x1ff;
            let pte = unsafe { core::ptr::read((*table).as_ptr().add(idx)) };
            if !pte.is_valid() {
                return None;
            }
            if pte.is_leaf() {
                let off = va & ((1usize << (12 + 9 * level)) - 1);
                return Some((pte.pa() as usize + off, pte.flags_perm()));
            }
            table = pte.pa() as *mut _;
        }
        let idx = (va >> 12) & 0x1ff;
        let pte = unsafe { core::ptr::read((*table).as_ptr().add(idx)) };
        if !pte.is_valid() {
            return None;
        }
        Some((pte.pa() as usize | (va & 0xfff), pte.flags_perm()))
    }
}

// Sv39 satp encoding: mode(4) | ASID(16) | PPN(44)
const SATP_SV39: usize = 8 << 60;

pub fn satp_value(pt: &PageTable) -> usize {
    SATP_SV39 | (pt.root_pa() >> 12)
}
