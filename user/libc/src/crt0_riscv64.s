# crt0 — startup do Pulsar para apps C (RISC-V). O loader ja zera a .bss
# e monta a pilha; aqui so chamamos main e saimos.
.section .text._start
.global _start
_start:
    call    main
    li      a7, 3          # SYS_EXIT
    ecall
1:  j       1b
