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
/// sstatus.FS: estado da unidade de FP (0 Off, 1 Initial, 2 Clean, 3 Dirty).
const SSTATUS_FS: u64 = 3 << 13;
const FS_INITIAL: u64 = 1 << 13;
const FS_CLEAN: u64 = 2 << 13;

impl Context {
    pub const fn zeroed() -> Self {
        Context { x: [0; 32], sepc: 0, sstatus: 0 }
    }

    /// Processo de usuario: U-mode, interrupcoes ligadas apos o sret.
    pub fn new_user(entry: u64, sp: u64) -> Self {
        let mut c = Self::zeroed();
        c.sepc = entry;
        c.x[2] = sp;
        c.sstatus = SSTATUS_SPIE | SSTATUS_SUM | FS_INITIAL;
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
    /// Volta o PC para o `ecall` (sempre 4 bytes): a syscall roda de novo
    /// quando o processo voltar (esperas de E/S).
    #[inline] pub fn rewind_syscall(&mut self) { self.sepc -= 4 }
    /// Desfaz rewind_syscall (a espera foi interrompida: retorna em vez de refazer).
    #[inline] pub fn skip_syscall(&mut self) { self.sepc += 4 }
    #[inline] pub fn set_sp(&mut self, v: u64) { self.x[2] = v }
    #[inline] pub fn set_tls(&mut self, v: u64) { self.x[4] = v }

    /// O processo mexeu no FP desde a ultima vez (FS = Dirty)? Se sim,
    /// marca como Clean: o chamador vai salvar os registradores agora.
    #[inline]
    pub fn fp_take_dirty(&mut self) -> bool {
        if self.sstatus & SSTATUS_FS == SSTATUS_FS {
            self.sstatus = (self.sstatus & !SSTATUS_FS) | FS_CLEAN;
            true
        } else {
            false
        }
    }

    /// FP restaurado de fora (rt_sigreturn): marca Dirty para a proxima
    /// troca salvar o valor novo.
    #[inline]
    pub fn mark_fp_dirty(&mut self) { self.sstatus |= SSTATUS_FS; }
}

/// Registradores F/D de um processo: f0..f31 e fcsr. O kernel (rv64imac)
/// nunca os usa; eles so trocam de dono em process::dispatch.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FpState {
    pub f: [u64; 32],
    pub fcsr: u64,
}

impl FpState {
    pub const fn zeroed() -> Self {
        FpState { f: [0; 32], fcsr: 0 }
    }
}

macro_rules! fp_regs {
    ($op:literal) => {
        concat!(
            $op, " f0, 0({p})\n",    $op, " f1, 8({p})\n",    $op, " f2, 16({p})\n",   $op, " f3, 24({p})\n",
            $op, " f4, 32({p})\n",   $op, " f5, 40({p})\n",   $op, " f6, 48({p})\n",   $op, " f7, 56({p})\n",
            $op, " f8, 64({p})\n",   $op, " f9, 72({p})\n",   $op, " f10, 80({p})\n",  $op, " f11, 88({p})\n",
            $op, " f12, 96({p})\n",  $op, " f13, 104({p})\n", $op, " f14, 112({p})\n", $op, " f15, 120({p})\n",
            $op, " f16, 128({p})\n", $op, " f17, 136({p})\n", $op, " f18, 144({p})\n", $op, " f19, 152({p})\n",
            $op, " f20, 160({p})\n", $op, " f21, 168({p})\n", $op, " f22, 176({p})\n", $op, " f23, 184({p})\n",
            $op, " f24, 192({p})\n", $op, " f25, 200({p})\n", $op, " f26, 208({p})\n", $op, " f27, 216({p})\n",
            $op, " f28, 224({p})\n", $op, " f29, 232({p})\n", $op, " f30, 240({p})\n", $op, " f31, 248({p})\n",
        )
    };
}

/// Salva os registradores de FP da hart em `st`. Liga o FS do proprio
/// kernel antes (senao fsd e instrucao ilegal); o sstatus do processo
/// volta do frame no sret.
pub unsafe fn fp_save(st: *mut FpState) {
    unsafe {
        core::arch::asm!(
            ".option push",
            ".option arch, +d",
            "csrs sstatus, {fs}",
            fp_regs!("fsd"),
            "frcsr {t}",
            "sd {t}, 256({p})",
            ".option pop",
            p = in(reg) st,
            fs = in(reg) FS_INITIAL,
            t = out(reg) _,
            options(nostack),
        );
    }
}

/// Carrega `st` nos registradores de FP da hart.
pub unsafe fn fp_restore(st: *const FpState) {
    unsafe {
        core::arch::asm!(
            ".option push",
            ".option arch, +d",
            "csrs sstatus, {fs}",
            fp_regs!("fld"),
            "ld {t}, 256({p})",
            "fscsr {t}",
            ".option pop",
            p = in(reg) st,
            fs = in(reg) FS_INITIAL,
            t = out(reg) _,
            options(nostack),
        );
    }
}
