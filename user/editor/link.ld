ENTRY(_start)
SECTIONS
{
    . = 0x0B000000;              /* base do trespass */
    .text   : { *(.text .text.*) }
    .rodata : ALIGN(4096) { *(.rodata .rodata.*) }
    .data   : ALIGN(4096) { *(.data .data.*) }
    .bss    : { *(.bss .bss.*) }
}
