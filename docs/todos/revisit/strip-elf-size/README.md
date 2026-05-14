# revisit: ELF size optimization

**Why deferred:** sh.elf is 1.2 KB after `objcopy --strip-all`. Fits
comfortably in one page. Good enough.

**What would trigger revisit:**
- A user binary growing past 4 KB just from ELF overhead (before code)
- Multiple PT_LOAD segments in the same binary creating wasteful padding

**What to try:**
- `objcopy --remove-section=.shstrtab` (removes section header string table; PT_LOAD doesn't need it)
- Drop section headers entirely — the kernel only reads program headers
- Custom linker script that emits a minimal ELF with one PT_LOAD only

Easy follow-up if it becomes a problem.
