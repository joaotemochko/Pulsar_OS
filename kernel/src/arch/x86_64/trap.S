/* trap.S — Tratamento de interrupções, exceções e syscalls em x86_64 */
.att_syntax prefix
.code64

.section .text

.macro TRAP_NOERR vec
.global trap_entry_\vec
trap_entry_\vec:
    pushq $0                /* dummy error code */
    pushq $\vec             /* vector number */
    jmp common_trap_entry
.endm

.macro TRAP_ERR vec
.global trap_entry_\vec
trap_entry_\vec:
    pushq $\vec             /* vector number (error code already pushed by CPU) */
    jmp common_trap_entry
.endm

/* Exceções 0..31 */
TRAP_NOERR 0   /* #DE Divide Error */
TRAP_NOERR 1   /* #DB Debug */
TRAP_NOERR 2   /* NMI */
TRAP_NOERR 3   /* #BP Breakpoint */
TRAP_NOERR 4   /* #OF Overflow */
TRAP_NOERR 5   /* #BR Bound Range Exceeded */
TRAP_NOERR 6   /* #UD Invalid Opcode */
TRAP_NOERR 7   /* #NM Device Not Available */
TRAP_ERR   8   /* #DF Double Fault */
TRAP_NOERR 9   /* Coprocessor Segment Overrun */
TRAP_ERR   10  /* #TS Invalid TSS */
TRAP_ERR   11  /* #NP Segment Not Present */
TRAP_ERR   12  /* #SS Stack Fault */
TRAP_ERR   13  /* #GP General Protection Fault */
TRAP_ERR   14  /* #PF Page Fault */
TRAP_NOERR 15  /* Reserved */
TRAP_NOERR 16  /* #MF x87 FPU Error */
TRAP_ERR   17  /* #AC Alignment Check */
TRAP_NOERR 18  /* #MC Machine Check */
TRAP_NOERR 19  /* #XM SIMD Floating-Point Exception */
TRAP_NOERR 20  /* #VE Virtualization Exception */
TRAP_ERR   21  /* #CP Control Protection Exception */
TRAP_NOERR 22
TRAP_NOERR 23
TRAP_NOERR 24
TRAP_NOERR 25
TRAP_NOERR 26
TRAP_NOERR 27
TRAP_NOERR 28
TRAP_ERR   29  /* #VC VMM Communication */
TRAP_ERR   30  /* #SX Security Exception */
TRAP_NOERR 31

/* IRQs 32..47 */
TRAP_NOERR 32  /* IRQ 0: Timer (PIT) */
TRAP_NOERR 33  /* IRQ 1: Teclado */
TRAP_NOERR 34
TRAP_NOERR 35
TRAP_NOERR 36
TRAP_NOERR 37
TRAP_NOERR 38
TRAP_NOERR 39
TRAP_NOERR 40
TRAP_NOERR 41
TRAP_NOERR 42
TRAP_NOERR 43
TRAP_NOERR 44
TRAP_NOERR 45
TRAP_NOERR 46
TRAP_NOERR 47
TRAP_NOERR 48  /* IPI */
TRAP_NOERR 255 /* Spurious APIC */

common_trap_entry:
    sub $8, %rsp            /* espaço para fs_base (offset 120) */

    /* Salva 15 registradores gerais (offset 112 down to 0) */
    push %rax
    push %rbx
    push %rcx
    push %rdx
    push %rsi
    push %rdi
    push %rbp
    push %r8
    push %r9
    push %r10
    push %r11
    push %r12
    push %r13
    push %r14
    push %r15

    /* Lê FS_BASE e salva no offset 120(%rsp) sem corromper registradores do usuário */
    mov $0xC0000100, %ecx   /* IA32_FS_BASE */
    rdmsr
    shl $32, %rdx
    or %rdx, %rax
    mov %rax, 120(%rsp)

    /* Restaura rax, rcx, rdx clobbered por rdmsr */
    mov 112(%rsp), %rax
    mov 96(%rsp), %rcx
    mov 88(%rsp), %rdx

    /* Argumento %rdi = ponteiro para Context (*mut Context) */
    sub $8, %rsp            /* alinha rsp a 16 bytes para SysV ABI */
    lea 8(%rsp), %rdi
    call x86_trap_dispatch
    add $8, %rsp

    /* Restaura FS_BASE caso tenha sido modificado (ex: arch_prctl) */
    mov 120(%rsp), %rax
    test %rax, %rax
    jz 1f
    mov $0xC0000100, %ecx
    mov %rax, %rdx
    shr $32, %rdx
    wrmsr
1:
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

    /* Descarta fs_base, vector, err_code */
    add $24, %rsp

    /* Retorna de interrupção / exceção */
    iretq

/* Entrada da instrução SYSCALL */
.global x86_syscall_entry
x86_syscall_entry:
    swapgs
    mov %rsp, %gs:0         /* salva user rsp em PerCpu.user_rsp */
    mov %gs:8, %rsp         /* carrega PerCpu.kstack_top */

    /* Constrói o quadro iretq na pilha de kernel:
     * ss, rsp, rflags, cs, rip
     */
    pushq $0x1B             /* User SS (0x18 | 3) */
    pushq %gs:0             /* User RSP */
    pushq %r11              /* User RFLAGS (salvo pelo hardware em r11) */
    pushq $0x23             /* User CS (0x20 | 3) */
    pushq %rcx              /* User RIP (salvo pelo hardware em rcx) */

    pushq $0                /* dummy err_code (offset 136) */
    pushq $0x80             /* vector syscall (offset 128) */
    sub $8, %rsp            /* espaço para fs_base (offset 120) */

    swapgs                  /* restaura GS do usuário */

    /* Salva os 15 registradores gerais (offset 112 down to 0) */
    push %rax
    push %rbx
    push %rcx
    push %rdx
    push %rsi
    push %rdi
    push %rbp
    push %r8
    push %r9
    push %r10
    push %r11
    push %r12
    push %r13
    push %r14
    push %r15

    /* Lê e salva FS_BASE no offset 120 */
    mov $0xC0000100, %ecx
    rdmsr
    shl $32, %rdx
    or %rdx, %rax
    mov %rax, 120(%rsp)

    /* Restaura rax, rcx, rdx clobbered por rdmsr */
    mov 112(%rsp), %rax
    mov 96(%rsp), %rcx
    mov 88(%rsp), %rdx

    /* %rdi = *mut Context */
    sub $8, %rsp            /* alinha rsp a 16 bytes para SysV ABI */
    lea 8(%rsp), %rdi
    call x86_syscall_dispatch
    add $8, %rsp

    /* Restaura FS_BASE se alterado */
    mov 120(%rsp), %rax
    test %rax, %rax
    jz 2f
    mov $0xC0000100, %ecx
    mov %rax, %rdx
    shr $32, %rdx
    wrmsr
2:
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

    add $24, %rsp
    iretq
