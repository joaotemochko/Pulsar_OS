.section .text

.macro SAVE_CONTEXT
    sub     sp,  sp,  #288
    stp     x0,  x1,  [sp, #16 * 0]
    stp     x2,  x3,  [sp, #16 * 1]
    stp     x4,  x5,  [sp, #16 * 2]
    stp     x6,  x7,  [sp, #16 * 3]
    stp     x8,  x9,  [sp, #16 * 4]
    stp     x10, x11, [sp, #16 * 5]
    stp     x12, x13, [sp, #16 * 6]
    stp     x14, x15, [sp, #16 * 7]
    stp     x16, x17, [sp, #16 * 8]
    stp     x18, x19, [sp, #16 * 9]
    stp     x20, x21, [sp, #16 * 10]
    stp     x22, x23, [sp, #16 * 11]
    stp     x24, x25, [sp, #16 * 12]
    stp     x26, x27, [sp, #16 * 13]
    stp     x28, x29, [sp, #16 * 14]
    str     x30,      [sp, #16 * 15]
    mrs     x9,  elr_el1
    mrs     x10, sp_el0
    mrs     x11, spsr_el1
    stp     x9,  x10, [sp, #16 * 16]
    str     x11,      [sp, #16 * 17]
.endm

.macro RESTORE_CONTEXT
    ldp     x9,  x10, [sp, #16 * 16]
    ldr     x11,      [sp, #16 * 17]
    msr     elr_el1,  x9
    msr     sp_el0,   x10
    msr     spsr_el1, x11
    ldp     x0,  x1,  [sp, #16 * 0]
    ldp     x2,  x3,  [sp, #16 * 1]
    ldp     x4,  x5,  [sp, #16 * 2]
    ldp     x6,  x7,  [sp, #16 * 3]
    ldp     x8,  x9,  [sp, #16 * 4]
    ldp     x10, x11, [sp, #16 * 5]
    ldp     x12, x13, [sp, #16 * 6]
    ldp     x14, x15, [sp, #16 * 7]
    ldp     x16, x17, [sp, #16 * 8]
    ldp     x18, x19, [sp, #16 * 9]
    ldp     x20, x21, [sp, #16 * 10]
    ldp     x22, x23, [sp, #16 * 11]
    ldp     x24, x25, [sp, #16 * 12]
    ldp     x26, x27, [sp, #16 * 13]
    ldp     x28, x29, [sp, #16 * 14]
    ldr     x30,      [sp, #16 * 15]
    add     sp,  sp,  #288
.endm

/* Cada entrada da tabela e CURTA: apenas um salto. Assim cabe nos 128 bytes
   por entrada e o alinhamento .balign 0x80 nunca e estourado. O codigo pesado
   (SAVE/RESTORE) vive nas rotinas fora da tabela, abaixo. */

.macro VEC_UNHANDLED idx
.balign 0x80
    mov     x0, #\idx
    b       unhandled_entry
.endm

.macro VEC_EL0_SYNC
.balign 0x80
    b       el0_sync_entry
.endm

.macro VEC_IRQ
.balign 0x80
    b       irq_entry
.endm

.balign 0x800
.global vector_table
vector_table:
    VEC_UNHANDLED 0    /* 0x000  EL atual SP0,  Sincrona */
    VEC_UNHANDLED 1    /* 0x080  EL atual SP0,  IRQ      */
    VEC_UNHANDLED 2    /* 0x100  EL atual SP0,  FIQ      */
    VEC_UNHANDLED 3    /* 0x180  EL atual SP0,  SError   */
    VEC_UNHANDLED 4    /* 0x200  EL atual SPx,  Sincrona */
    VEC_IRQ            /* 0x280  EL atual SPx,  IRQ      <- timer enquanto kernel roda */
    VEC_UNHANDLED 6    /* 0x300  EL atual SPx,  FIQ      */
    VEC_UNHANDLED 7    /* 0x380  EL atual SPx,  SError   */
    VEC_EL0_SYNC       /* 0x400  EL inferior A64, Sincrona <- syscall/fault de EL0 */
    VEC_IRQ            /* 0x480  EL inferior A64, IRQ      <- timer enquanto EL0 roda */
    VEC_UNHANDLED 10   /* 0x500  EL inferior A64, FIQ     */
    VEC_UNHANDLED 11   /* 0x580  EL inferior A64, SError  */
    VEC_UNHANDLED 12   /* 0x600  EL inferior A32, Sincrona */
    VEC_UNHANDLED 13   /* 0x680  EL inferior A32, IRQ     */
    VEC_UNHANDLED 14   /* 0x700  EL inferior A32, FIQ     */
    VEC_UNHANDLED 15   /* 0x780  EL inferior A32, SError  */

/* ---- Rotinas fora da tabela (sem limite de 128 bytes) ---- */

/* Exceção nao tratada: x0 ja contem o indice. Reporta e trava controlado. */
unhandled_entry:
    SAVE_CONTEXT
    /* x0 = indice (setado na entrada da tabela, preservado ate aqui) */
    mrs     x1, esr_el1
    mrs     x2, elr_el1
    bl      rust_exception_dispatch
1:  wfe
    b       1b

/* Syscall / fault de EL0. */
el0_sync_entry:
    SAVE_CONTEXT
    mov     x0, sp
    bl      rust_el0_sync_handler
    RESTORE_CONTEXT
    eret

/* IRQ (timer). */
irq_entry:
    SAVE_CONTEXT
    mov     x0, sp
    bl      rust_irq_handler
    RESTORE_CONTEXT
    eret