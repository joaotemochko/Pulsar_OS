ENTRY(_start)
SECTIONS
{
    . = 0x03000000;              /* base do ball (nao colide com o shell) */
    .text   : { *(.text .text.*) }
    .rodata : ALIGN(4096) { *(.rodata .rodata.*) }
    .data   : ALIGN(4096) { *(.data .data.*) }
    .bss    : { *(.bss .bss.*) }
}
