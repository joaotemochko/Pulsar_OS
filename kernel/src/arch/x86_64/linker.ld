ENTRY(_start)

SECTIONS {
    . = 0x100000;

    .boot : {
        KEEP(*(.boot.multiboot))
        *(.boot.text)
        *(.boot.data)
        . = ALIGN(4096);
    }

    .text : ALIGN(4096) {
        *(.text .text.*)
    }

    .rodata : ALIGN(4096) {
        *(.rodata .rodata.*)
    }

    .data : ALIGN(4096) {
        *(.data .data.*)
    }

    .bss : ALIGN(4096) {
        __bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        . = ALIGN(4096);
        __bss_end = .;
    }

    . = ALIGN(4096);
    __kernel_end = .;
}

