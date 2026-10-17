// crt0.s — startup do Pulsar para apps C. Zera bss não é necessário aqui
// pois o loader já zera mem>file. Apenas chama main e sai.
.section .text._start
.global _start
_start:
    bl main
    // sys_exit
    mov x8, #3
    svc #0
1:  b 1b
