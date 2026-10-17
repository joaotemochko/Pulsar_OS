/// Espelha o layout do SAVE_CONTEXT (288 bytes).
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
    pub _pad: u64,      // offset 280 (frame de 288 bytes)
}

/// SPSR: EL1h com DAIF limpos (IRQs habilitadas) — laco ocioso.
const SPSR_EL1H: u64 = 0b0101;
/// SPSR: EL0t com DAIF limpos.
const SPSR_EL0T: u64 = 0;

impl Context {
    pub const fn zeroed() -> Self {
        Context { x: [0; 31], _gap: 0, elr: 0, sp_el0: 0, spsr: 0, _pad: 0 }
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
}
