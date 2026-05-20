/*
 * User-program linker script.
 *
 *  * `_start` forced at VMA 0 by collecting `.text.entry` first.
 *  * Two PT_LOAD segments so the kernel maps each with the right
 *    perms: text+rodata RX, data+bss RW. Without this, the ELF
 *    loader lumps everything into one RWX segment and the kernel
 *    happily writes user-supplied buffers into user code — a
 *    bug usertests' `copyout` catches by passing NULL as a read
 *    destination.
 */

OUTPUT_ARCH("riscv")
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
        *(.srodata .srodata.*)
    } :text

    /* Hard page-align the data segment regardless of section sizes
       so it never shares a frame with text/rodata (and so the kernel
       maps it as RW without including the code page). */
    . = ALIGN(0x1000);

    .data : {
        *(.data .data.*)
        *(.sdata .sdata.*)
    } :data

    .bss : ALIGN(8) {
        *(.sbss .sbss.*)
        *(.bss .bss.*)
        *(COMMON)
    } :data

    /DISCARD/ : { *(.eh_frame .eh_frame.*) *(.riscv.attributes) *(.comment) }
}
