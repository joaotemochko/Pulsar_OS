/// Frame de trap RISC-V (272 bytes, ver trap.S): x0..x31, sepc, sstatus.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Context {
    pub x: [u64; 32],
    pub sepc: u64,
    pub sstatus: u64,
}

const SSTATUS_SPP: u64 = 1 << 8;
const SSTATUS_SPIE: u64 = 1 << 5;
const SSTATUS_SUM: u64 = 1 << 18;

impl Context {
    pub const fn zeroed() -> Self {
        Context { x: [0; 32], sepc: 0, sstatus: 0 }
    }

    /// Processo de usuario: U-mode, interrupcoes ligadas apos o sret.
    pub fn new_user(entry: u64, sp: u64) -> Self {
        let mut c = Self::zeroed();
        c.sepc = entry;
        c.x[2] = sp;
        c.sstatus = SSTATUS_SPIE | SSTATUS_SUM;
        c
    }

    /// Laco ocioso: S-mode, pilha = topo da pilha de kernel da hart.
    pub fn new_idle(cpu: usize) -> Self {
        let mut c = Self::zeroed();
        c.sepc = super::idle_loop_addr();
        c.x[2] = crate::arch::kstack_top(cpu);
        c.sstatus = SSTATUS_SPP | SSTATUS_SPIE | SSTATUS_SUM;
        c
    }

    // ABI: numero em a7, argumentos em a0..a2, retorno em a0.
    #[inline] pub fn syscall_num(&self) -> u64 { self.x[17] }
    #[inline] pub fn arg(&self, i: usize) -> u64 { self.x[10 + i] }
    #[inline] pub fn set_ret(&mut self, v: u64) { self.x[10] = v }
    #[inline] pub fn pc(&self) -> u64 { self.sepc }
}
