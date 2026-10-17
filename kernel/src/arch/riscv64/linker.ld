OUTPUT_ARCH(riscv)
ENTRY(_start)

SECTIONS
{
    . = 0x80200000;

    .text :
    {
        KEEP(*(.text.boot))
        *(.text .text.*)
    }

    .rodata : ALIGN(16) { *(.rodata .rodata.* .srodata .srodata.*) }
    .data   : ALIGN(16) { *(.data .data.* .sdata .sdata.*) }

    .bss (NOLOAD) :
    {
        . = ALIGN(16);
        __bss_start = .;
        *(.sbss .sbss.* .bss .bss.*)
        *(COMMON)
        . = ALIGN(16);
        __bss_end = .;
    }

    . = ALIGN(4096);
    __kernel_end = .;

    /DISCARD/ : { *(.eh_frame .eh_frame_hdr) }
}
