//! Frame de trap e contexto de execução para x86_64.
//!
//! O layout do Context (184 bytes) espelha exatamente a pilha montada
//! na entrada de exceções, interrupções e syscalls, e consumida por
//! `arch_enter_frame` via `iretq`.

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Context {
    // Registradores de propósito geral (salvos pelo software)
    pub r15: u64, // offset 0
    pub r14: u64, // offset 8
    pub r13: u64, // offset 16
    pub r12: u64, // offset 24
    pub r11: u64, // offset 32
    pub r10: u64, // offset 40
    pub r9:  u64, // offset 48
    pub r8:  u64, // offset 56
    pub rbp: u64, // offset 64
    pub rdi: u64, // offset 72  (arg0)
    pub rsi: u64, // offset 80  (arg1)
    pub rdx: u64, // offset 88  (arg2)
    pub rcx: u64, // offset 96
    pub rbx: u64, // offset 104
    pub rax: u64, // offset 112 (syscall num / retorno)

    // Metadados do kernel e TLS
    pub fs_base: u64,  // offset 120 (ponteiro TLS %fs:0)
    pub vector: u64,   // offset 128 (vetor de interrupção/exceção)
    pub err_code: u64, // offset 136 (código de erro do hardware/stub)

    // Quadro desempilhado pelo hardware via iretq
    pub rip: u64,    // offset 144
    pub cs: u64,     // offset 152
    pub rflags: u64, // offset 160
    pub rsp: u64,    // offset 168
    pub ss: u64,     // offset 176
}

const RFLAGS_IF: u64 = 0x200;
const RFLAGS_RESERVED: u64 = 0x002;
const RFLAGS_USER: u64 = RFLAGS_IF | RFLAGS_RESERVED;

const KERNEL_CS: u64 = 0x08;
const KERNEL_SS: u64 = 0x10;
const USER_SS: u64   = 0x18 | 3; // DPL 3
const USER_CS: u64   = 0x20 | 3; // DPL 3

impl Context {
    pub const fn zeroed() -> Self {
        Context {
            r15: 0, r14: 0, r13: 0, r12: 0, r11: 0, r10: 0, r9: 0, r8: 0,
            rbp: 0, rdi: 0, rsi: 0, rdx: 0, rcx: 0, rbx: 0, rax: 0,
            fs_base: 0, vector: 0, err_code: 0,
            rip: 0, cs: 0, rflags: 0, rsp: 0, ss: 0,
        }
    }

    /// Contexto inicial de um processo de usuário (Ring 3).
    pub fn new_user(entry: u64, sp: u64) -> Self {
        let mut c = Self::zeroed();
        c.rip = entry;
        c.rsp = sp;
        c.cs = USER_CS;
        c.ss = USER_SS;
        c.rflags = RFLAGS_USER;
        c
    }

    /// Contexto do laço ocioso da CPU `cpu` (Ring 0).
    pub fn new_idle(cpu: usize) -> Self {
        let mut c = Self::zeroed();
        c.rip = super::idle_loop_addr();
        c.rsp = crate::arch::kstack_top(cpu);
        c.cs = KERNEL_CS;
        c.ss = KERNEL_SS;
        c.rflags = RFLAGS_USER;
        c
    }

    #[inline] pub fn syscall_num(&self) -> u64 { self.rax }
    #[inline] pub fn arg(&self, i: usize) -> u64 {
        match i {
            0 => self.rdi,
            1 => self.rsi,
            2 => self.rdx,
            3 => self.r10,
            4 => self.r8,
            5 => self.r9,
            _ => 0,
        }
    }
    #[inline] pub fn set_ret(&mut self, v: u64) { self.rax = v; }
    #[inline] pub fn pc(&self) -> u64 { self.rip }
    /// Volta o PC para a instrução `syscall` (2 bytes, 0x0F 0x05)
    #[inline] pub fn rewind_syscall(&mut self) { self.rip -= 2; }
    /// Desfaz rewind_syscall (retorna em vez de refazer)
    #[inline] pub fn skip_syscall(&mut self) { self.rip += 2; }
    #[inline] pub fn set_sp(&mut self, v: u64) { self.rsp = v; }
    #[inline] pub fn set_tls(&mut self, v: u64) { self.fs_base = v; }

    #[inline] pub fn fp_take_dirty(&mut self) -> bool { true }
    #[inline] pub fn mark_fp_dirty(&mut self) {}
}

/// Estado FP/SSE salvo via FXRSTOR / FXSAVE (512 bytes alinhado a 16).
#[repr(C, align(16))]
#[derive(Clone, Copy)]
pub struct FpState {
    pub buf: [u8; 512],
}

impl FpState {
    pub const fn zeroed() -> Self {
        let mut s = FpState { buf: [0; 512] };
        // FCW = 0x037F (padrão x87: todas as máscaras de exceção ligadas)
        s.buf[0] = 0x7F;
        s.buf[1] = 0x03;
        // MXCSR = 0x1F80 (padrão SSE: todas as máscaras ligadas)
        s.buf[24] = 0x80;
        s.buf[25] = 0x1F;
        s
    }
}

pub unsafe fn fp_save(st: *mut FpState) {
    if (st as u64) & 0xF == 0 {
        unsafe {
            core::arch::asm!("fxsave [{}]", in(reg) st, options(nostack));
        }
    }
}

pub unsafe fn fp_restore(st: *const FpState) {
    if (st as u64) & 0xF == 0 {
        unsafe {
            core::arch::asm!("fxrstor [{}]", in(reg) st, options(nostack));
        }
    }
}

