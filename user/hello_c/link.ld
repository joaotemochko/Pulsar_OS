ENTRY(_start)
SECTIONS
{
    . = 0x10000000;                       /* base bem longe do MMIO */
    .text   : { *(.text._start) *(.text .text.*) }
    .rodata : ALIGN(4096) { *(.rodata .rodata.*) }
    .data   : ALIGN(4096) { *(.data .data.*) }
    .bss    : ALIGN(16)   { *(.bss .bss.*) *(COMMON) }
    . = ALIGN(4096);
}
