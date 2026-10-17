/* Boot RISC-V (S-mode). O OpenSBI escolhe UMA hart de boot (loteria) e a
 * entrega aqui com a0 = hartid, a1 = DTB e a MMU desligada. As demais
 * harts ficam paradas no OpenSBI ate o kernel chamar SBI HSM hart_start. */
.equ KSTACK_SHIFT, 17      # 128 KiB por hart (arch::KSTACK_SIZE)
.equ MAX_HARTS, 8

.section .text.boot
.global _start
_start:
    li      t0, MAX_HARTS
    bgeu    a0, t0, park

    la      t0, __bss_start
    la      t1, __bss_end
1:  bgeu    t0, t1, 2f
    sd      zero, 0(t0)
    addi    t0, t0, 8
    j       1b
2:
    # sp = KSTACKS + (hartid + 1) * KSTACK_SIZE
    la      sp, KSTACKS
    addi    t0, a0, 1
    slli    t0, t0, KSTACK_SHIFT
    add     sp, sp, t0
    call    arch_boot_main

park:
    wfi
    j       park

.section .text
# Entrada das harts secundarias (SBI HSM): a0 = hartid, a1 = opaco.
.global _secondary_entry
_secondary_entry:
    li      t0, MAX_HARTS
    bgeu    a0, t0, park
    la      sp, KSTACKS
    addi    t0, a0, 1
    slli    t0, t0, KSTACK_SHIFT
    add     sp, sp, t0
    call    arch_secondary_main
    j       park

# Laco ocioso: S-mode com SIE ligado ate o proximo timer/IPI.
.global arch_idle_loop
arch_idle_loop:
    wfi
    j       arch_idle_loop
