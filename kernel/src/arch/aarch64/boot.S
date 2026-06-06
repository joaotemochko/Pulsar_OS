.section .text.boot
.global _start

_start:
    /* Lê o registrador MPIDR_EL1 para descobrir qual é o número do núcleo atual */
    mrs x1, mpidr_el1
    and x1, x1, #3
    cbz x1, 2f /* Se for o núcleo 0, pula para o rótulo '2' (iniciar o S.O.) */

1:  /* Se NÃO for o núcleo 0, entra em loop infinito dormindo (Wait For Event) */
    wfe
    b 1b

2:  /* Configuração exclusiva do Núcleo 0 */
    
    /* Configura o ponteiro da pilha (Stack Pointer) usando o endereço definido no linker */
    ldr x1, =__stack_top
    mov sp, x1

    /* Limpa os registradores x0, x1, x2 (boa prática antes de chamar código C/Rust) */
    mov x0, #0
    mov x1, #0
    mov x2, #0

    /* Pula para a nossa função principal escrita em Rust! */
    bl kernel_main

    /* Caso a função Rust retorne por algum motivo, trava o processador aqui */
    b 1b