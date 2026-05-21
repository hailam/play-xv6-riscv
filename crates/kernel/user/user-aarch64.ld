/*
 * AArch64 user-program linker script. Mirror of `user.ld` for
 * riscv64. Two PT_LOAD segments — text RX, data RW page-aligned.
 */

OUTPUT_ARCH("aarch64")
ENTRY(_start)

PHDRS {
    text PT_LOAD FLAGS(5);  /* R | X */
    data PT_LOAD FLAGS(6);  /* R | W */
}

SECTIONS
{
    . = 0;

    .text : {
        *(.text.entry)
        *(.text .text.*)
    } :text

    .rodata : ALIGN(8) {
        *(.rodata .rodata.*)
    } :text

    . = ALIGN(0x1000);

    .data : {
        *(.data .data.*)
    } :data

    .bss : ALIGN(8) {
        *(.bss .bss.*)
        *(COMMON)
    } :data

    /DISCARD/ : { *(.eh_frame .eh_frame.*) *(.note .note.*) *(.ARM.attributes) *(.comment) }
}
