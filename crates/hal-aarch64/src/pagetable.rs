//! ARMv8 long-descriptor page tables — 4 KiB granule, 48-bit VA,
//! single TTBR0 (xv6-style, mirrors RISC-V Sv39 in spirit).
//!
//! Layout (ARM ARM D8.3):
//!   VA[47:39] L0idx | [38:30] L1idx | [29:21] L2idx | [20:12] L3idx | [11:0] off
//!   Each table = 512 × 8 byte descriptors = 4 KiB.
//!
//! L3 page descriptor bit layout:
//!   [1:0]   type = 0b11
//!   [4:2]   AttrIndx[2:0]   (MAIR_EL1 index)
//!   [5]     NS              (non-secure)
//!   [7:6]   AP[2:1]         (00=EL1 R/W; 01=EL0+EL1 R/W; 10=EL1 RO; 11=EL0+EL1 RO)
//!   [9:8]   SH              (11 = Inner Shareable for RAM)
//!   [10]    AF              (Access Flag — must be 1)
//!   [11]    nG              (1 = non-global; we set for user mappings)
//!   [47:12] output PA
//!   [53]    PXN             (priv exec-never)
//!   [54]    UXN             (unpriv exec-never)
//!
//! `Drop for PageTable` uses the nG bit to identify user leaves to free.

use core::sync::atomic::{AtomicPtr, Ordering};

use hal::{FrameAllocator, PageTableOps, PtePerm, VmError};

use crate::memlayout::{KERNBASE, MAXVA, PGSIZE};

// ---------- descriptor bits ----------

const DESC_VALID: u64 = 1 << 0;
const DESC_TABLE: u64 = 1 << 1; // combined with VALID = table or L3 page
const DESC_AF: u64 = 1 << 10;
const DESC_SH_IS: u64 = 0b11 << 8;
const DESC_NG: u64 = 1 << 11;
const DESC_PXN: u64 = 1 << 53;
const DESC_UXN: u64 = 1 << 54;

const AP_KERNEL_RW: u64 = 0b00 << 6;
const AP_USER_RW: u64 = 0b01 << 6;
const AP_KERNEL_RO: u64 = 0b10 << 6;
const AP_USER_RO: u64 = 0b11 << 6;

const ATTRIDX_MMIO: u64 = 0 << 2; // MAIR_EL1 Attr0 = Device-nGnRnE
const ATTRIDX_RAM: u64 = 1 << 2; // MAIR_EL1 Attr1 = Normal WB-WA

const PA_MASK: u64 = 0x0000_FFFF_FFFF_F000; // bits [47:12]

fn make_branch(next_pa: u64) -> u64 {
    DESC_VALID | DESC_TABLE | (next_pa & PA_MASK)
}

fn make_leaf(pa: u64, perm: PtePerm, is_mmio: bool) -> u64 {
    let user = (perm.0 & PtePerm::USER) != 0;
    let exec = (perm.0 & PtePerm::EXEC) != 0;
    let write = (perm.0 & PtePerm::WRITE) != 0;

    let mut bits = DESC_VALID | DESC_TABLE | DESC_AF;

    if is_mmio {
        bits |= ATTRIDX_MMIO; // Device-nGnRnE; SH ignored for Device memory
    } else {
        bits |= ATTRIDX_RAM | DESC_SH_IS;
    }

    bits |= match (user, write) {
        (false, true) => AP_KERNEL_RW,
        (false, false) => AP_KERNEL_RO,
        (true, true) => AP_USER_RW,
        (true, false) => AP_USER_RO,
    };

    // Execute permission: PXN = "no EL1 exec", UXN = "no EL0 exec".
    if user {
        // User mappings: kernel never executes via this entry.
        bits |= DESC_PXN;
        if !exec {
            bits |= DESC_UXN;
        }
        // nG = 1 for user mappings (also used as our "free me on Drop"
        // marker, analogous to RISC-V's PTE_U).
        bits |= DESC_NG;
    } else {
        // Kernel mappings: user never executes.
        bits |= DESC_UXN;
        if !exec {
            bits |= DESC_PXN;
        }
    }

    bits | (pa & PA_MASK)
}

fn decode_perm(pte: u64) -> PtePerm {
    let mut p = PtePerm::READ;
    let ap = (pte >> 6) & 0b11;
    let writable = ap == 0b00 || ap == 0b01;
    let user = ap == 0b01 || ap == 0b11;
    if writable {
        p |= PtePerm::WRITE;
    }
    if user {
        p |= PtePerm::USER;
    }
    let exec_for_caller = if user {
        (pte & DESC_UXN) == 0
    } else {
        (pte & DESC_PXN) == 0
    };
    if exec_for_caller {
        p |= PtePerm::EXEC;
    }
    PtePerm(p)
}

// ---------- frame-free callback (used by Drop) ----------

static FREE_FRAME: AtomicPtr<()> = AtomicPtr::new(core::ptr::null_mut());

pub fn install_free_frame(f: unsafe fn(usize)) {
    FREE_FRAME.store(f as *mut (), Ordering::Release);
}

unsafe fn free_frame(pa: usize) {
    let p = FREE_FRAME.load(Ordering::Acquire);
    if p.is_null() {
        return;
    }
    let f: unsafe fn(usize) = unsafe { core::mem::transmute(p) };
    unsafe { f(pa) };
}

// ---------- PageTable ----------

pub struct PageTable {
    root_pa: usize,
}

impl PageTable {
    pub fn root_pa(&self) -> usize {
        self.root_pa
    }

    /// Walk to the L3 PTE pointer for `va`. If `alloc` is `Some`,
    /// allocates missing intermediate tables; otherwise returns
    /// `None` for missing branches.
    fn walk(
        &mut self,
        va: usize,
        alloc: Option<&dyn FrameAllocator>,
    ) -> Option<*mut u64> {
        if va >= MAXVA {
            return None;
        }
        let mut table: *mut u64 = self.root_pa as *mut u64;
        for level in 0..3 {
            let shift = 12 + 9 * (3 - level);
            let idx = (va >> shift) & 0x1FF;
            let pte_ptr = unsafe { table.add(idx) };
            let pte = unsafe { core::ptr::read(pte_ptr) };
            if (pte & DESC_VALID) == 0 {
                let a = alloc?;
                let new_pa = a.alloc_zeroed()?;
                unsafe {
                    core::ptr::write(pte_ptr, make_branch(new_pa as u64));
                }
                table = new_pa as *mut u64;
            } else {
                table = (pte & PA_MASK) as *mut u64;
            }
        }
        let idx = (va >> 12) & 0x1FF;
        Some(unsafe { table.add(idx) })
    }
}

impl Drop for PageTable {
    fn drop(&mut self) {
        unsafe { free_subtree(self.root_pa, 3) }
    }
}

/// Walks a (sub)tree rooted at `pt_pa` and frees:
///   * leaves with `nG` set (user pages)
///   * every non-leaf intermediate table
/// Skips non-user leaves (TRAMPOLINE / TRAPFRAME / MMIO — owned by
/// the kernel image or `Proc`).
///
/// `depth` is the *number of levels below* this one — root = 3
/// (=L0, has 3 levels below it: L1/L2/L3). Recursion bottoms out
/// at depth 0 (L3 leaves).
unsafe fn free_subtree(pt_pa: usize, depth: i32) {
    let ents = pt_pa as *mut u64;
    for i in 0..512 {
        let pte = unsafe { core::ptr::read(ents.add(i)) };
        if (pte & DESC_VALID) == 0 {
            continue;
        }
        let target_pa = (pte & PA_MASK) as usize;
        if depth > 0 {
            unsafe { free_subtree(target_pa, depth - 1) };
        } else {
            // L3 leaf. Free only user pages (nG set).
            if (pte & DESC_NG) != 0 {
                unsafe { free_frame(target_pa) };
            }
        }
        unsafe { core::ptr::write(ents.add(i), 0) };
    }
    unsafe { free_frame(pt_pa) };
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
        let end = va.checked_add(size).ok_or(VmError::OutOfRange)?;
        if end > MAXVA {
            return Err(VmError::OutOfRange);
        }
        // Range is uniformly MMIO or RAM in our callers — pick by
        // the starting PA.
        let is_mmio = pa < KERNBASE;
        let mut va = va;
        let mut pa = pa;
        while va < end {
            let pte_ptr = self.walk(va, Some(alloc)).ok_or(VmError::Oom)?;
            let existing = unsafe { core::ptr::read(pte_ptr) };
            if (existing & DESC_VALID) != 0 {
                return Err(VmError::Remap);
            }
            unsafe {
                core::ptr::write(pte_ptr, make_leaf(pa as u64, perm, is_mmio));
            }
            va += PGSIZE;
            pa += PGSIZE;
        }
        Ok(())
    }

    fn translate(&self, va: usize) -> Option<(usize, PtePerm)> {
        if va >= MAXVA {
            return None;
        }
        let mut table = self.root_pa as *const u64;
        for level in 0..3 {
            let shift = 12 + 9 * (3 - level);
            let idx = (va >> shift) & 0x1FF;
            let pte = unsafe { core::ptr::read(table.add(idx)) };
            if (pte & DESC_VALID) == 0 {
                return None;
            }
            table = (pte & PA_MASK) as *const u64;
        }
        let idx = (va >> 12) & 0x1FF;
        let pte = unsafe { core::ptr::read(table.add(idx)) };
        if (pte & DESC_VALID) == 0 {
            return None;
        }
        let pa = (pte & PA_MASK) as usize | (va & 0xFFF);
        Some((pa, decode_perm(pte)))
    }

    fn unmap_page(&mut self, va: usize) -> Option<usize> {
        if va >= MAXVA || va & (PGSIZE - 1) != 0 {
            return None;
        }
        let pte_ptr = self.walk(va, None)?;
        let pte = unsafe { core::ptr::read(pte_ptr) };
        if (pte & DESC_VALID) == 0 {
            return None;
        }
        let pa = (pte & PA_MASK) as usize;
        unsafe { core::ptr::write(pte_ptr, 0) };
        Some(pa)
    }
}

/// TTBR0_EL1 value for a given root. ASID = 0, CnP = 0.
pub fn ttbr0_value(pt: &PageTable) -> usize {
    pt.root_pa()
}

// ---------- MMU enable / TLB invalidation ----------

/// MAIR_EL1: Attr0 = Device-nGnRnE (0x00), Attr1 = Normal WB-WA (0xFF).
pub const MAIR_EL1_VAL: u64 = (0xFF << 8) | 0x00;

/// TCR_EL1 for single-TTBR0, 4 K granule, 48-bit VA.
///
///   T0SZ  = 16    → TTBR0 covers 2^48 bytes
///   IRGN0 = 01    → Inner WB-RW-Alloc
///   ORGN0 = 01    → Outer WB-RW-Alloc
///   SH0   = 11    → Inner Shareable
///   TG0   = 00    → 4 KiB granule
///   EPD1  = 1     → disable TTBR1 walks
///   IPS   = 010   → 40-bit PA (ample for 128 MiB)
pub const TCR_EL1_VAL: u64 = (16 << 0)
    | (0b01 << 8)
    | (0b01 << 10)
    | (0b11 << 12)
    | (0b00 << 14)
    | (1 << 23)
    | (0b010 << 32);
