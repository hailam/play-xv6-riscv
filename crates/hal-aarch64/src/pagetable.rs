//! ARMv8 long-descriptor page-table skeleton.
//!
//! Placeholder implementation: provides the shape so `impl Hal`
//! compiles. Real 4-level translation, AP/AttrIndx encoding, and
//! TLB shootdown land with the boot follow-up.

use hal::{FrameAllocator, PageTableOps, PtePerm, VmError};

use crate::memlayout::PGSIZE;

/// Owned ARMv8 root translation table (TTBR0_EL1 target).
pub struct PageTable {
    root_pa: usize,
}

impl PageTable {
    pub fn root_pa(&self) -> usize {
        self.root_pa
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
        _perm: PtePerm,
        _alloc: &dyn FrameAllocator,
    ) -> Result<(), VmError> {
        if va % PGSIZE != 0 || pa % PGSIZE != 0 || size % PGSIZE != 0 || size == 0 {
            return Err(VmError::Misaligned);
        }
        // TODO: walk + populate descriptors. Returning Ok keeps the
        // trait impl satisfied; any real use will crash later because
        // the table is empty.
        Ok(())
    }

    fn translate(&self, _va: usize) -> Option<(usize, PtePerm)> {
        None
    }

    fn unmap_page(&mut self, _va: usize) -> Option<usize> {
        // Skeleton — real impl pairs with the populate path.
        None
    }
}

/// TTBR0_EL1 value for a given root. Skeleton — real impl encodes
/// ASID + cnp + the root PA per ARMv8 TTBR encoding.
pub fn ttbr0_value(pt: &PageTable) -> usize {
    pt.root_pa()
}
