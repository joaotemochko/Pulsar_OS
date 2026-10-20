/* boot.S — Entrada 32-bit Multiboot 1 e transição para Long Mode 64-bit (x86_64).
 * O QEMU carrega o kernel em 0x100000 no modo protegido 32-bit com paginação desativada.
 */
.att_syntax prefix


.section .boot.multiboot, "a"
.align 4
.set MB_MAGIC, 0x1BADB002
.set MB_FLAGS, 0x00000003
.set MB_CHECKSUM, -(0x1BADB002 + 0x00000003)

.long MB_MAGIC
.long MB_FLAGS
.long MB_CHECKSUM


.section .boot.text, "ax"
.code32
.global _start
_start:
    cli
    cld
    mov %ebx, (boot_mb_info)

    /* Preenche as 4 tabelas de diretório de páginas (boot_pd0..boot_pd3) com blocos de 2MB.
     * Mapeia 4 GiB: 4 * 512 = 2048 entradas de 2 MiB cada.
     * Flags: 0x83 = Present (bit 0) | Read/Write (bit 1) | Page Size 2MB (bit 7)
     */
    mov $boot_pd, %edi
    mov $0x83, %eax         /* base física 0, flags 0x83 */
    mov $0, %edx
    mov $2048, %ecx         /* 2048 entradas de 2MB = 4 GiB */
1:
    mov %eax, (%edi)
    mov %edx, 4(%edi)
    add $0x200000, %eax     /* +2 MiB */
    adc $0, %edx
    add $8, %edi
    dec %ecx
    jnz 1b

    /* Carrega CR3 com a raiz das tabelas de boot (boot_pml4) */
    mov $boot_pml4, %eax
    mov %eax, %cr3

    /* Habilita PAE (Physical Address Extension) no CR4 (bit 5) */
    mov %cr4, %eax
    or $(1 << 5), %eax
    mov %eax, %cr4

    /* Habilita Long Mode (LME, bit 8) e No-Execute (NXE, bit 11) no MSR EFER (0xC0000080) */
    mov $0xC0000080, %ecx
    rdmsr
    or $((1 << 8) | (1 << 11)), %eax
    wrmsr

    /* Habilita Paginação (PG, bit 31) e Proteção (PE, bit 0) no CR0 */
    mov %cr0, %eax
    or $((1 << 31) | (1 << 0)), %eax
    mov %eax, %cr0

    /* Carrega a GDT de 64-bit */
    lgdt (boot_gdt64_ptr)

    /* Salto longo para entrar em 64-bit Long Mode (seletor 0x08 = código de kernel) */
    ljmp $0x08, $_start64

.code64
_start64:
    /* Segmentos de dados do kernel (0x10) */
    mov $0x10, %ax
    mov %ax, %ds
    mov %ax, %es
    mov %ax, %ss
    mov $0, %ax
    mov %ax, %fs
    mov %ax, %gs

    /* Recarrega a GDT no modo 64-bit */
    lgdt (boot_gdt64_ptr)

    /* Habilita SSE, FXSAVE/FXRSTOR e instruções de ponto flutuante */
    mov %cr0, %rax
    and $~(1 << 2), %rax   /* limpa EM (bit 2) */
    or $(1 << 1), %rax     /* liga MP (bit 1) */
    mov %rax, %cr0

    mov %cr4, %rax
    or $((1 << 9) | (1 << 10)), %rax /* OSFXSR (bit 9) | OSXMMEXCPT (bit 10) */
    mov %rax, %cr4
    fninit

    /* Habilita instruções SYSCALL/SYSRET no MSR EFER (bit 0 = SCE) */
    mov $0xC0000080, %ecx
    rdmsr
    or $1, %eax
    wrmsr

    /* Configura pilha da CPU 0: topo da primeira pilha em KSTACKS */
    mov $KSTACKS, %rax
    add $131072, %rax      /* KSTACK_SIZE = 128 KiB */
    mov %rax, %rsp

    /* Chama o ponto de entrada Rust */
    call arch_boot_main

    /* Se retornar, para o processador */
1:  cli
    hlt
    jmp 1b

.global arch_idle_loop
arch_idle_loop:
1:  sti
    hlt
    jmp 1b

/* Trampolim AP (Application Processor) para inicialização SMP via INIT-SIPI.
 * Copiado para 0x8000 na memória física; o SIPI com vetor 0x08 inicia aqui em 16-bit real mode.
 */
.global ap_trampoline_start
.global ap_trampoline_end

ap_trampoline_start:
    .code16
    cli
    cld
    xorw %ax, %ax
    movw %ax, %ds
    movw %ax, %es
    movw %ax, %ss
    movw $0x7C00, %sp
    lgdt (0x8000 + (tramp_gdt_ptr - ap_trampoline_start))
    movl %cr0, %eax
    orl $1, %eax
    movl %eax, %cr0
    ljmpl $0x08, $(0x8000 + (tramp_pm32 - ap_trampoline_start))

.align 8
tramp_gdt_ptr:
    .word (tramp_gdt_end - tramp_gdt - 1)
    .long (0x8000 + (tramp_gdt - ap_trampoline_start))

.align 8
tramp_gdt:
    .quad 0x0000000000000000
    .quad 0x00CF9A000000FFFF
    .quad 0x00CF92000000FFFF
tramp_gdt_end:

.code32
tramp_pm32:
    mov $0x10, %ax
    mov %ax, %ds
    mov %ax, %es
    mov %ax, %ss

    /* Habilita PAE no CR4 (bit 5) */
    mov %cr4, %eax
    or $(1 << 5), %eax
    mov %eax, %cr4

    /* Carrega CR3 com boot_pml4 */
    mov $boot_pml4, %eax
    mov %eax, %cr3

    /* Habilita Long Mode (LME) e NXE no MSR EFER (0xC0000080) */
    mov $0xC0000080, %ecx
    rdmsr
    or $((1 << 8) | (1 << 11)), %eax
    wrmsr

    /* Habilita Paginação (PG) e Proteção (PE) no CR0 */
    mov %cr0, %eax
    or $((1 << 31) | (1 << 0)), %eax
    mov %eax, %cr0

    /* Carrega GDT 64-bit e salta para Long Mode */
    lgdt (boot_gdt64_ptr)
    ljmp $0x08, $_secondary_start64
ap_trampoline_end:

.code64
.global _secondary_start64
_secondary_start64:
    /* Segmentos de dados 64-bit */
    mov $0x10, %ax
    mov %ax, %ds
    mov %ax, %es
    mov %ax, %ss
    mov $0, %ax
    mov %ax, %fs
    mov %ax, %gs

    /* Recarrega GDT no modo 64-bit */
    lgdt (boot_gdt64_ptr)

    /* Habilita SSE e instruções de ponto flutuante */
    mov %cr0, %rax
    and $~(1 << 2), %rax
    or $(1 << 1), %rax
    mov %rax, %cr0

    mov %cr4, %rax
    or $((1 << 9) | (1 << 10)), %rax
    mov %rax, %cr4
    fninit

    /* Habilita SCE no EFER */
    mov $0xC0000080, %ecx
    rdmsr
    or $1, %eax
    wrmsr

    /* Muda para a tabela de páginas final do kernel se já ativa */
    mov x86_kernel_root(%rip), %rax
    test %rax, %rax
    jz 1f
    mov %rax, %cr3
1:
    /* Lê o cpu_id escrito pelo BSP na mailbox em 0x7FF0 */
    mov 0x7FF0, %edi
    movslq %edi, %rdi

    /* Escreve 1 na flag de confirmação em 0x7FF4 */
    movl $1, 0x7FF4
    mfence

    /* Configura pilha da CPU: KSTACKS + (cpu + 1) * 131072 */
    mov %rdi, %rax
    inc %rax
    shl $17, %rax
    add $KSTACKS, %rax
    mov %rax, %rsp

    /* Chama o ponto de entrada da CPU secundária */
    call arch_secondary_main

2:  cli
    hlt
    jmp 2b

.code64

/* arch_enter_frame(ctx: *const Context, stack_top: u64) -> !
 *
 * Transfere a execução para o contexto especificado na pilha indicada.
 * Desempilha os registradores do Context e executa iretq.
 */
.global arch_enter_frame
arch_enter_frame:
    /* %rdi = ctx, %rsi = stack_top */
    /* Muda para o topo da pilha de destino */
    mov %rsi, %rsp

    /* Copia os 23 campos de Context (184 bytes) para a pilha de destino */
    sub $184, %rsp
    mov %rdi, %rsi          /* origem ctx */
    mov %rsp, %rdi          /* destino na pilha (%rsp) */
    mov $23, %rcx
    rep movsq

    /* Restaura FS_BASE se estiver em Ring 3 */
    mov 120(%rsp), %rax     /* fs_base está no offset 120 */
    test %rax, %rax
    jz 2f
    mov $0xC0000100, %ecx   /* IA32_FS_BASE */
    mov %rax, %rdx
    shr $32, %rdx
    wrmsr
2:
    /* Garante que DS e ES sejam configurados para modo de usuário se CS for de usuário */
    mov 152(%rsp), %rax     /* cs está no offset 152 */
    test $3, %al
    jz 3f
    mov $0x1B, %cx          /* User Data selector */
    mov %cx, %ds
    mov %cx, %es
3:
    /* Desempilha registradores gerais */
    pop %r15
    pop %r14
    pop %r13
    pop %r12
    pop %r11
    pop %r10
    pop %r9
    pop %r8
    pop %rbp
    pop %rdi
    pop %rsi
    pop %rdx
    pop %rcx
    pop %rbx
    pop %rax

    /* Pula fs_base (8B), vector (8B), err_code (8B) = 24 bytes */
    add $24, %rsp

    /* Agora o topo da pilha tem: rip, cs, rflags, rsp, ss
     * Executa iretq para retornar a Ring 3 (ou Ring 0 para idle loop)
     */
    iretq

.section .boot.data, "aw", @progbits
.align 4096
boot_pml4:
    .quad boot_pdpt + 0x03   /* Entradas 0..511: entrada 0 aponta para boot_pdpt */
    .fill 511, 8, 0

.align 4096
boot_pdpt:
    .quad boot_pd + 0 * 4096 + 0x03 /* 0..1 GiB */
    .quad boot_pd + 1 * 4096 + 0x03 /* 1..2 GiB */
    .quad boot_pd + 2 * 4096 + 0x03 /* 2..3 GiB */
    .quad boot_pd + 3 * 4096 + 0x03 /* 3..4 GiB */
    .fill 508, 8, 0

.align 4096
boot_pd:
    .fill 2048, 8, 0        /* 4 tabelas de 512 entradas preenchidas no _start */

.align 16
.global boot_gdt64
boot_gdt64:
    .quad 0x0000000000000000 /* 0x00: Null */
    .quad 0x00AF9A000000FFFF /* 0x08: Kernel Code 64-bit (DPL 0, L=1) */
    .quad 0x00CF92000000FFFF /* 0x10: Kernel Data 64-bit (DPL 0) */
    .quad 0x00CFF2000000FFFF /* 0x18: User Data 64-bit   (DPL 3) */
    .quad 0x00AFFA000000FFFF /* 0x20: User Code 64-bit   (DPL 3, L=1) */
    /* 0x28 .. 0xA8: 8 descritores TSS (16 bytes cada) para as 8 CPUs */
    .fill 16, 8, 0
boot_gdt64_end:

.global boot_gdt64_ptr
boot_gdt64_ptr:
    .word boot_gdt64_end - boot_gdt64 - 1
    .quad boot_gdt64

.global boot_mb_info
boot_mb_info:
    .quad 0
