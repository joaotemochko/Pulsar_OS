ENTRY(_start)
SECTIONS
{
    . = 0x01000000;              /* base do shell */
    .text   : { *(.text .text.*) }
    .rodata : ALIGN(4096) { *(.rodata .rodata.*) }
    .data   : ALIGN(4096) { *(.data .data.*) }
    .bss    : { *(.bss .bss.*) }
}
