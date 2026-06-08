.section .text

.macro SAVE_CONTEXT
    sub     sp,  sp,  #256
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
.endm

.macro RESTORE_CONTEXT
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
    add     sp,  sp,  #256
.endm

/* Exceção nao tratada: salva contexto, reporta via Rust (indice, ESR, ELR),
   e trava de forma controlada. NAO retorna — um fault sincrono nao tratado
   re-dispararia na mesma instrucao se fizesse eret. */
.macro VENTRY idx
.balign 0x80
    SAVE_CONTEXT
    mov     x0, #\idx
    mrs     x1, esr_el1
    mrs     x2, elr_el1
    bl      rust_exception_dispatch
1:  wfe
    b       1b
.endm

/* Syscall (svc de EL0): salva contexto, despacha, grava retorno em x0, retorna. */
.macro VENTRY_SYSCALL
.balign 0x80
    SAVE_CONTEXT
    mov     x3, x8                 /* 4o arg = numero da syscall */
    bl      rust_syscall_dispatch
    str     x0, [sp, #16 * 0]      /* grava retorno no slot salvo de x0 */
    RESTORE_CONTEXT
    eret
.endm

.balign 0x800
.global vector_table
vector_table:
    VENTRY 0           /* 0x000  EL atual SP0,  Sincrona */
    VENTRY 1           /* 0x080  EL atual SP0,  IRQ      */
    VENTRY 2           /* 0x100  EL atual SP0,  FIQ      */
    VENTRY 3           /* 0x180  EL atual SP0,  SError   */
    VENTRY 4           /* 0x200  EL atual SPx,  Sincrona */
    VENTRY 5           /* 0x280  EL atual SPx,  IRQ      */
    VENTRY 6           /* 0x300  EL atual SPx,  FIQ      */
    VENTRY 7           /* 0x380  EL atual SPx,  SError   */
    VENTRY_SYSCALL     /* 0x400  EL inferior A64, Sincrona <- svc de EL0 */
    VENTRY 9           /* 0x480  EL inferior A64, IRQ     */
    VENTRY 10          /* 0x500  EL inferior A64, FIQ     */
    VENTRY 11          /* 0x580  EL inferior A64, SError  */
    VENTRY 12          /* 0x600  EL inferior A32, Sincrona */
    VENTRY 13          /* 0x680  EL inferior A32, IRQ     */
    VENTRY 14          /* 0x700  EL inferior A32, FIQ     */
    VENTRY 15          /* 0x780  EL inferior A32, SError  */