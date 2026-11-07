# 42 — Split Kernel/User VA, Hardware ASIDs, 2 MiB Huge Pages & Fast User Copy

## Problem statement

Currently, every single system call, timer tick, or device interrupt pays a massive MMU and page-table penalty:
1. **Per-trap page-table swap & full/broadcast TLB flush:**
   - On AArch64 (`crates/hal-aarch64/asm/trampoline.S` lines 141–145, 195–199, 245–249 & `crates/hal-aarch64/src/pagetable.rs` lines 302–324), `TCR_EL1_VAL` sets `EPD1 = 1` (disabling `TTBR1_EL1`) and places both user and kernel mappings in `TTBR0_EL1` with `ASID = 0`. Every trap entry (`uservec_sync`, `uservec_irq`) and every trap exit (`trampoline_userret`) switches `ttbr0_el1` and executes `tlbi vmalle1is` — an Inner-Shareable broadcast TLB shootdown across **all CPU cores**!
   - On RISC-V (`crates/hal-riscv64/asm/trampoline.S` lines 111–112, 127–128 & `crates/hal-riscv64/src/pagetable.rs` line 284), `satp_value` encodes `ASID = 0` (`SATP_SV39 | (pt.root_pa() >> 12)`). Every trap entry and exit executes `csrw satp, ...; sfence.vma zero, zero`, flushing the entire local TLB twice per trap.
2. **Per-byte software page-table walk under `SpinLock` in `copyin`/`copyout`:**
   - `Proc::translate_user` and `Proc::translate_user_write` (`crates/kernel/src/proc.rs` lines 410–432) acquire `self.vm.lock()` (disabling interrupts) and walk all 3 levels of the software page table (`L2 -> L1 -> L0`).
   - Every I/O path in `crates/kernel/src/syscall.rs` (`inode_read` line 1581, `sys_pread` line 1742, `sys_pwrite` line 1787, `console_write` line 1408, `fb_read` line 1539, `tcp_read` line 2463/2472, `pipe_write` line 1907, `pipe_read` line 1997/2022) calls `translate_user` / `translate_user_write` **inside a `0..n` per-byte loop**! A 4 KiB `read()` locks `proc.vm`, toggles interrupts, and walks the 3-level page table **4,096 times**.
3. **4 KiB-only kernel direct mappings:**
   - `crates/kernel/src/vm.rs` (lines 72–79) maps the 128 MiB `KERNBASE..PHYSTOP` range using 32,768 individual 4 KiB leaf PTEs (consuming 64 page-table frames and thrashing the TLB on kernel memory accesses).

## Discovery & research checklist

1. **Split User/Kernel Virtual Address Space & Zero-Switch Traps:**
   - **AArch64 (`crates/hal-aarch64/`):**
     - Enable `TTBR1_EL1` for kernel space (`EPD1 = 0`, `T1SZ = 16`, `TG1 = 4 KiB`) or keep the kernel page table in `TTBR1_EL1` while `TTBR0_EL1` holds the current process's user page table with a 16-bit hardware ASID in `TTBR0_EL1[63:48]`.
     - Even simpler transition before full high-half kernel relocation: share the kernel L1/L2 entries (with `UXN = 1`, `AP = EL1-only`, `nG = 0` global) in user page tables and assign each process a unique 16-bit ASID in `TTBR0_EL1[63:48]`.
     - Remove `msr ttbr0_el1` and `tlbi vmalle1is` from `uservec_sync`, `uservec_irq`, and `trampoline_userret` in `crates/hal-aarch64/asm/trampoline.S`! Only update `ttbr0_el1` when switching between different user processes, without any `tlbi` unless an ASID generation rolls over or `munmap`/COW invalidates a specific VA (`tlbi vaae1is` / `tlbi aside1is`).
   - **RISC-V (`crates/hal-riscv64/`):**
     - Map kernel direct-map (`KERNBASE..PHYSTOP`) and MMIO regions as global (`PTE_G = 1 << 5`, `PTE_U = 0`) by copying the kernel root page table's L2 PTE entries into every user root page table at creation time (`UserVm::new`).
     - Probe supported ASID bits at boot (write `0xffff` to `satp[59:44]` and read back) and allocate per-process `(generation, asid)` pairs.
     - In `crates/hal-riscv64/asm/trampoline.S`, eliminate the `csrw satp` and `sfence.vma zero, zero` on trap entry and exit when staying in the same process; only write `satp` (without `sfence.vma`) when `executor` switches to a different process, and only `sfence.vma` on ASID rollover or explicit page unmap/protect.
2. **2 MiB Huge-Page (Megapage / Block) Kernel Direct Map (`pagetable.rs`, `vm.rs`):**
   - In `crates/hal-riscv64/src/pagetable.rs`, add `map_2m` (installing an L1 leaf PTE with `PTE_V | PTE_R | PTE_W | PTE_X | PTE_G`).
   - In `crates/hal-aarch64/src/pagetable.rs`, add `map_2m` (installing an L2 block descriptor with bits `[1:0] = 0b01`, `AF`, `SH_INNER`, `ATTR_IDX_NORMAL`).
   - Update `crates/kernel/src/vm.rs` (`map_range`) to use 2 MiB mappings for aligned 2 MiB subranges of `KERNBASE..PHYSTOP` (and `etext..PHYSTOP`), cutting 32,768 4 KiB PTEs down to 64 L1/L2 entries and saving ~63 physical page-table frames.
3. **Page-Chunked & Hardware-MMU `copyin` / `copyout` / `copyinstr` (`proc.rs`, `syscall.rs`):**
   - Introduce `Proc::copy_from_user(&self, dst: &mut [u8], src_va: usize) -> Result<(), ()>` and `Proc::copy_to_user(&self, dst_va: usize, src: &[u8]) -> Result<(), ()>`.
   - **Stage 1 (Page-chunked software walk):** Lock `self.vm.lock()` once, walk once per 4 KiB page boundary (`chunk = (PGSIZE - (va & (PGSIZE - 1))).min(remaining)`), and `core::ptr::copy_nonoverlapping` the entire page slice at once. This immediately reduces lock acquisitions and page-table walks by **4,096x** for 4 KiB I/O and replaces all per-byte `translate_user` / `translate_user_write` loops across `crates/kernel/src/syscall.rs`.
   - **Stage 2 (Hardware-MMU direct copy with fault fixup):** Once user mappings remain active during kernel execution (via split VA / shared kernel L2 PTEs), enable `sstatus.SUM = 1` (RISC-V) or clear `PSTATE.PAN` (AArch64) inside a small assembly `raw_copy_user` routine whose load/store PC range is registered in an exception fixup table checked by `kerneltrap`. This completely removes `self.vm.lock()` and software page-table walks from `copyin`/`copyout`!

## Verification

- `usertests` (including `copyin`, `copyout`, `copyinstr1`, `copyinstr2`, `copyinstr3`, `argptest`, `sbrkbugs`, `mmap`) passes on both `riscv64` and `aarch64` under `-smp 4`.
- `bench` null-syscall latency (`getpid`), pipe throughput, and 4 KiB file `read`/`write` throughput improve dramatically (verify `tlbi vmalle1is` / `sfence.vma zero, zero` count on syscalls drops to 0).
