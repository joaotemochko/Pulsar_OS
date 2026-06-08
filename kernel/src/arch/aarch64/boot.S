.section .text.boot
.global _start

_start:
    /* Só o núcleo 0 segue; os outros dormem */
    mrs     x0, mpidr_el1
    and     x0, x0, #0xFF
    cbnz    x0, halt

    /* Zera a seção BSS (de __bss_start até __bss_end, de 8 em 8 bytes) */
    ldr     x0, =__bss_start
    ldr     x1, =__bss_end
zero_bss:
    cmp     x0, x1
    b.hs    bss_done
    str     xzr, [x0], #8
    b       zero_bss
bss_done:

    /* Configura o stack pointer */
    ldr     x0, =__stack_top
    mov     sp, x0

    bl      kernel_main

halt:
    wfe
    b       halt