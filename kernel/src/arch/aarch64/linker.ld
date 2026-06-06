ENTRY(_start)

SECTIONS
{
    . = 0x80000;

    .text :
    {
        KEEP(*(.text.boot))
        *(.text .text.*)
    }

    .rodata : { *(.rodata .rodata.*) }
    .data : { *(.data .data.*) }

    .bss (NOLOAD) :
    {
        . = ALIGN(16);
        __bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        __bss_end = .;
    }
    . = ALIGN(8);
    . = . + 0x80000; /* 512KB de stack */
    __stack_top = .;
}