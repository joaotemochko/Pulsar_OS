//! Sincronizacao SMP: travas finas por subsistema (sem Big Kernel Lock).
//!
//! O kernel roda com interrupcoes mascaradas, entao uma CPU nunca e
//! interrompida segurando uma trava (spinlocks simples bastam). Cada
//! subsistema tem a sua trava e elas so podem ser aninhadas nesta ordem
//! (de fora para dentro), o que torna impossivel um deadlock:
//!
//!   STAGE (syscall.rs)  ->  FS (fs.rs)  ->  FRAME
//!   SCHED (process.rs)  ->  INPUT, SERVICES, VM, MMU  ->  FRAME
//!   VM (vm.rs)          ->  SHM, MMU, FRAME
//!   FS                  ->  SHM (shm.rs)  ->  FRAME
//!   SCHED               ->  FS (fecha os descritores de quem termina)
//!   MMU (aqui)          ->  FRAME (frame_allocator.rs)
//!   GPU, NET, CONSOLE   (folhas: nao pegam nenhuma outra trava)
//!
//! Assim duas CPUs podem, ao mesmo tempo, ler o disco, apresentar um quadro,
//! falar com a rede e trocar mensagens de IPC: so disputam o que de fato
//! compartilham.
use core::sync::atomic::{AtomicBool, Ordering};
use crate::arch::platform::MAX_CPUS;

/// Travas presas por cada CPU (para a recuperacao de falhas do kernel:
/// quem cai segurando uma trava a solta antes de seguir — ver oops em
/// main.rs).
const MAX_HELD: usize = 16;
static mut HELD: [[usize; MAX_HELD]; MAX_CPUS] = [[0; MAX_HELD]; MAX_CPUS];
static mut NHELD: [usize; MAX_CPUS] = [0; MAX_CPUS];

/// Solta todas as travas que esta CPU segura (da mais recente para a mais
/// antiga). Retorna quantas eram. So para a recuperacao de falhas.
pub fn force_release_all() -> usize {
    let c = crate::arch::cpu_id();
    unsafe {
        let n = NHELD[c];
        for i in (0..n.min(MAX_HELD)).rev() {
            let l = HELD[c][i] as *const SpinLock;
            (*l).locked.store(false, Ordering::Release);
        }
        NHELD[c] = 0;
        n
    }
}

pub struct SpinLock {
    locked: AtomicBool,
}

impl SpinLock {
    pub const fn new() -> Self {
        SpinLock { locked: AtomicBool::new(false) }
    }

    #[inline]
    pub fn lock(&self) {
        loop {
            if self
                .locked
                .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
            {
                let c = crate::arch::cpu_id();
                unsafe {
                    let n = NHELD[c];
                    if n < MAX_HELD { HELD[c][n] = self as *const _ as usize; }
                    NHELD[c] = n + 1;
                }
                return;
            }
            while self.locked.load(Ordering::Relaxed) {
                core::hint::spin_loop();
            }
        }
    }

    #[inline]
    pub fn unlock(&self) {
        let c = crate::arch::cpu_id();
        unsafe {
            // tira a entrada mais recente desta trava (normalmente a ultima)
            let n = NHELD[c];
            let me = self as *const _ as usize;
            let mut i = n.min(MAX_HELD);
            while i > 0 {
                i -= 1;
                if HELD[c][i] == me {
                    HELD[c].copy_within(i + 1..n.min(MAX_HELD), i);
                    break;
                }
            }
            NHELD[c] = n.saturating_sub(1);
        }
        self.locked.store(false, Ordering::Release);
    }

    /// Trava e devolve um guarda que destrava ao sair do escopo.
    #[inline]
    pub fn guard(&self) -> Guard<'_> {
        self.lock();
        Guard(self)
    }
}

pub struct Guard<'a>(&'a SpinLock);

impl Drop for Guard<'_> {
    #[inline]
    fn drop(&mut self) {
        self.0.unlock();
    }
}

/// Tabelas de paginas (as do kernel sao compartilhadas por todos os espacos).
pub static MMU: SpinLock = SpinLock::new();
