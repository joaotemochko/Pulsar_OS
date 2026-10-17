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
}

impl Context {
    pub const fn zeroed() -> Self {
        Context { x: [0; 31], _gap: 0, elr: 0, sp_el0: 0, spsr: 0 }
    }
}