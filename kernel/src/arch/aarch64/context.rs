/// Espelha o layout do SAVE_CONTEXT (288 bytes). TPIDR_EL0 (ponteiro de
/// TLS do usuario) e salvo junto: cada thread tem o seu.
/// x0..x30 = 31 regs (offsets 0..248). x30 esta em [240]; offset 248 fica
/// VAGO (o assembly pula para 256). Por isso o _gap aqui.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Context {
    pub x: [u64; 31],   // offsets 0..248 (x0..x30)
    pub _gap: u64,      // offset 248 — nao usado pelo assembly (alinhamento)
    pub elr: u64,       // offset 256  (16*16)
    pub sp_el0: u64,    // offset 264
    pub spsr: u64,      // offset 272
    pub tpidr: u64,     // offset 280 (TPIDR_EL0; frame de 288 bytes)
}

/// SPSR: EL1h com DAIF limpos (IRQs habilitadas) — laco ocioso.
const SPSR_EL1H: u64 = 0b0101;
/// SPSR: EL0t com DAIF limpos.
const SPSR_EL0T: u64 = 0;

impl Context {
    pub const fn zeroed() -> Self {
        Context { x: [0; 31], _gap: 0, elr: 0, sp_el0: 0, spsr: 0, tpidr: 0 }
    }

    /// Contexto inicial de um processo de usuario.
    pub fn new_user(entry: u64, sp: u64) -> Self {
        let mut c = Self::zeroed();
        c.elr = entry;
        c.sp_el0 = sp;
        c.spsr = SPSR_EL0T;
        c
    }

    /// Contexto do laco ocioso da CPU `cpu`. A pilha usada e a propria
    /// pilha de kernel (SP_EL1), que volta ao topo a cada eret.
    pub fn new_idle(_cpu: usize) -> Self {
        let mut c = Self::zeroed();
        c.elr = super::idle_loop_addr();
        c.spsr = SPSR_EL1H;
        c
    }

    #[inline] pub fn syscall_num(&self) -> u64 { self.x[8] }
    #[inline] pub fn arg(&self, i: usize) -> u64 { self.x[i] }
    #[inline] pub fn set_ret(&mut self, v: u64) { self.x[0] = v }
    #[inline] pub fn pc(&self) -> u64 { self.elr }
    /// Volta o PC para a instrucao `svc`: a syscall roda de novo quando o
    /// processo voltar (esperas de E/S). x0..x5 ainda tem os argumentos.
    #[inline] pub fn rewind_syscall(&mut self) { self.elr -= 4 }
    /// Desfaz rewind_syscall (a espera foi interrompida: retorna em vez de refazer).
    #[inline] pub fn skip_syscall(&mut self) { self.elr += 4 }
    #[inline] pub fn set_sp(&mut self, v: u64) { self.sp_el0 = v }
    #[inline] pub fn set_tls(&mut self, v: u64) { self.tpidr = v }

    /// O ARM nao marca uso de FP por processo: sempre salva.
    #[inline] pub fn fp_take_dirty(&mut self) -> bool { true }
    /// FP restaurado de fora (rt_sigreturn): no ARM sempre se salva.
    #[inline] pub fn mark_fp_dirty(&mut self) {}
}

/// Registradores de FP/SIMD de um processo: v0..v31 (128 bits), FPCR, FPSR.
/// O kernel e softfloat e nunca os usa, entao eles so mudam de dono na
/// troca de processo (process::dispatch).
#[repr(C, align(16))]
#[derive(Clone, Copy)]
pub struct FpState {
    pub v: [u128; 32],
    pub fpcr: u64,
    pub fpsr: u64,
}

impl FpState {
    pub const fn zeroed() -> Self {
        FpState { v: [0; 32], fpcr: 0, fpsr: 0 }
    }
}

macro_rules! fp_pairs {
    ($op:literal) => {
        concat!(
            $op, " q0,  q1,  [{p}, #0]\n",   $op, " q2,  q3,  [{p}, #32]\n",
            $op, " q4,  q5,  [{p}, #64]\n",  $op, " q6,  q7,  [{p}, #96]\n",
            $op, " q8,  q9,  [{p}, #128]\n", $op, " q10, q11, [{p}, #160]\n",
            $op, " q12, q13, [{p}, #192]\n", $op, " q14, q15, [{p}, #224]\n",
            $op, " q16, q17, [{p}, #256]\n", $op, " q18, q19, [{p}, #288]\n",
            $op, " q20, q21, [{p}, #320]\n", $op, " q22, q23, [{p}, #352]\n",
            $op, " q24, q25, [{p}, #384]\n", $op, " q26, q27, [{p}, #416]\n",
            $op, " q28, q29, [{p}, #448]\n", $op, " q30, q31, [{p}, #480]\n",
        )
    };
}

/// Salva os registradores de FP da CPU em `st`.
pub unsafe fn fp_save(st: *mut FpState) {
    unsafe {
        core::arch::asm!(
            ".arch_extension fp",
            ".arch_extension simd",
            fp_pairs!("stp"),
            "mrs {t}, fpcr",
            "str {t}, [{p}, #512]",
            "mrs {t}, fpsr",
            "str {t}, [{p}, #520]",
            p = in(reg) st,
            t = out(reg) _,
            options(nostack, preserves_flags),
        );
    }
}

/// Carrega `st` nos registradores de FP da CPU.
pub unsafe fn fp_restore(st: *const FpState) {
    unsafe {
        core::arch::asm!(
            ".arch_extension fp",
            ".arch_extension simd",
            fp_pairs!("ldp"),
            "ldr {t}, [{p}, #512]",
            "msr fpcr, {t}",
            "ldr {t}, [{p}, #520]",
            "msr fpsr, {t}",
            p = in(reg) st,
            t = out(reg) _,
            options(nostack, preserves_flags),
        );
    }
}

/// Libera FP/SIMD em EL0 e EL1 (CPACR_EL1.FPEN = 0b11) nesta CPU.
pub fn fp_enable() {
    unsafe {
        core::arch::asm!(
            "mrs {t}, cpacr_el1",
            "orr {t}, {t}, #(3 << 20)",
            "msr cpacr_el1, {t}",
            "isb",
            t = out(reg) _,
            options(nomem, nostack, preserves_flags),
        );
    }
}
