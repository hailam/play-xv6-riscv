/*
 * Kernel linker script for QEMU virt machine (aarch64).
 *
 * Image base = 0x4008_0000 (DRAM at 0x4000_0000 + Linux-style
 * 1 MiB offset; QEMU's direct-kernel boot honours ELF p_paddr).
 */

OUTPUT_ARCH("aarch64")
ENTRY(_entry)

SECTIONS
{
    . = 0x40080000;

    .text : {
        KEEP(*(.text.entry))
        *(.text .text.*)
        . = ALIGN(0x1000);
        PROVIDE(_trampoline = .);
        *(.trampsec)
        . = ALIGN(0x1000);
        PROVIDE(_etext = .);
    }

    .rodata : ALIGN(0x1000) {
        *(.rodata .rodata.*)
        *(.srodata .srodata.*)
        . = ALIGN(0x1000);
    }

    .data : ALIGN(0x1000) {
        *(.data .data.*)
        *(.sdata .sdata.*)
    }

    PROVIDE(_edata = .);

    .bss : ALIGN(0x1000) {
        PROVIDE(_bss_start = .);
        *(.sbss .sbss.*)
        *(.bss .bss.*)
        *(COMMON)
        PROVIDE(_bss_end = .);
    }

    . = ALIGN(0x1000);
    PROVIDE(_end = .);

    /DISCARD/ : { *(.eh_frame .eh_frame.*) *(.note .note.*) *(.ARM.attributes) }
}
