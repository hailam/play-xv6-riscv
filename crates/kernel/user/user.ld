/*
 * User-program linker script. Forces `_start` to be at VMA 0 by
 * collecting `.text.entry` (where ulib.S puts `_start`) before any
 * other `.text` sections.
 */

OUTPUT_ARCH("riscv")
ENTRY(_start)

SECTIONS
{
    . = 0;

    .text : {
        *(.text.entry)
        *(.text .text.*)
    }

    .rodata : ALIGN(8) {
        *(.rodata .rodata.*)
        *(.srodata .srodata.*)
    }

    .data : ALIGN(8) {
        *(.data .data.*)
        *(.sdata .sdata.*)
    }

    .bss : ALIGN(8) {
        *(.sbss .sbss.*)
        *(.bss .bss.*)
        *(COMMON)
    }

    /DISCARD/ : { *(.eh_frame .eh_frame.*) *(.riscv.attributes) *(.comment) }
}
