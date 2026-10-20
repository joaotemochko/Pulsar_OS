// crt0_x86_64.s — startup do Pulsar para apps C em x86_64.
// O loader já zera mem>file e configura a pilha.
.section .text._start
.global _start
_start:
    // Alinha pilha a 16 bytes e chama main
    and $-16, %rsp
    call main

    // sys_exit(status): rax = 3 (SYS_EXIT), rdi = status retornado por main (em rax)
    mov %rax, %rdi
    mov $3, %rax
    syscall

1:  hlt
    jmp 1b

