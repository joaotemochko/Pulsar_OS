/* Boot AArch64 do Pulsar OS.
 *
 * CPU 0: zera a BSS, monta a pilha de kernel 0 e salta para o Rust.
 * Demais CPUs: com -kernel o QEMU as deixa desligadas (PSCI); se alguma
 * chegar aqui mesmo assim, ela dorme. Elas sao acordadas pelo kernel via
 * PSCI CPU_ON e entram em _secondary_entry com x0 = indice da CPU.
 */
.equ KSTACK_SHIFT, 17          /* 128 KiB por CPU (arch::KSTACK_SIZE) */

.section .text.boot
.global _start
_start:
    mrs     x0, mpidr_el1
    and     x0, x0, #0xFF
    cbnz    x0, park

    ldr     x0, =__bss_start
    ldr     x1, =__bss_end
zero_bss:
    cmp     x0, x1
    b.hs    bss_done
    str     xzr, [x0], #8
    b       zero_bss
bss_done:

    /* pilha da CPU 0 = KSTACKS + 1 * KSTACK_SIZE */
    ldr     x0, =KSTACKS
    mov     x1, #1
    lsl     x1, x1, #KSTACK_SHIFT
    add     x0, x0, x1
    mov     sp, x0

    bl      arch_boot_main

park:
    wfe
    b       park

/* Entrada das CPUs secundarias (PSCI CPU_ON). x0 = indice da CPU.
 * A MMU ainda esta desligada: replicamos a configuracao da CPU 0 antes de
 * executar qualquer Rust (atomicos exclusivos exigem memoria Normal). */
.section .text
.global _secondary_entry
_secondary_entry:
    msr     tpidr_el1, x0
    ldr     x1, =KSTACKS
    add     x2, x0, #1
    lsl     x2, x2, #KSTACK_SHIFT
    add     x1, x1, x2
    mov     sp, x1

    ldr     x1, =MMU_MAIR
    ldr     x1, [x1]
    msr     mair_el1, x1
    ldr     x1, =MMU_TCR
    ldr     x1, [x1]
    msr     tcr_el1, x1
    ldr     x1, =MMU_TTBR0
    ldr     x1, [x1]
    msr     ttbr0_el1, x1
    isb
    tlbi    vmalle1
    dsb     nsh
    isb
    ldr     x1, =MMU_SCTLR
    ldr     x1, [x1]
    msr     sctlr_el1, x1
    isb
    ic      iallu
    dsb     nsh
    isb

    bl      arch_secondary_main
1:  wfe
    b       1b

/* Laco ocioso: roda em EL1h com IRQs habilitadas ate o proximo tick/IPI. */
.global arch_idle_loop
arch_idle_loop:
    wfi
    b       arch_idle_loop
