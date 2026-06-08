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

    . = ALIGN(16);
    . = . + 0x80000;   /* 512 KiB stack do kernel (SP_EL1) */
    . = ALIGN(16);
    __stack_top = .;

    . = ALIGN(16);
    . = . + 0x10000;   /* 64 KiB stack de usuario (SP_EL0) */
    . = ALIGN(16);
    __user_stack_top = .;
}