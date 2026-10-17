ENTRY(_start)

SECTIONS
{
    . = 0x40000000;

    .text :
    {
        KEEP(*(.text.boot))
        *(.text .text.*)
    }

    .rodata : { *(.rodata .rodata.*) }
    .data   : { *(.data .data.*) }

    .bss (NOLOAD) :
    {
        . = ALIGN(16);
        __bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        . = ALIGN(16);
        __bss_end = .;
    }

    /* As pilhas de kernel por CPU (KSTACKS) vivem na .bss. */
    . = ALIGN(4096);
    __kernel_end = .;
}
