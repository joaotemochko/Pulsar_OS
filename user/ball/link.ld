ENTRY(_start)
/* Layout de um app .pulse: cada segmento comeca numa pagina propria para
   que o loader aplique W^X (codigo R-X, constantes R--, dados RW-). */
PHDRS { text PT_LOAD FLAGS(5); rodata PT_LOAD FLAGS(4); data PT_LOAD FLAGS(6); }
SECTIONS
{
    . = 0x03000000;
    .text   : { *(.text._start) *(.text .text.*) } :text
    . = ALIGN(4096);
    .rodata : { *(.rodata .rodata.* .eh_frame*) } :rodata
    . = ALIGN(4096);
    .data   : { *(.data .data.*) } :data
    .bss    : { *(.bss .bss.*) *(COMMON) } :data
    /DISCARD/ : { *(.comment) }
}
