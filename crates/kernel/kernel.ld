/*
 * Kernel linker script for QEMU virt machine (riscv64).
 */

OUTPUT_ARCH("riscv")
ENTRY(_entry)

SECTIONS
{
    . = 0x80000000;

    .text : {
        KEEP(*(.text.entry))
        *(.text .text.*)
        . = ALIGN(0x1000);
        PROVIDE(_trampoline = .);
        *(.trampsec)
        . = ALIGN(0x1000);
        ASSERT(. - _trampoline == 0x1000, "trampoline must be exactly one page");
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

    /DISCARD/ : { *(.eh_frame .eh_frame.*) }
}
