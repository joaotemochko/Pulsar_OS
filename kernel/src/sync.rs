//! Sincronizacao SMP: travas finas por subsistema (sem Big Kernel Lock).
//!
//! O kernel roda com interrupcoes mascaradas, entao uma CPU nunca e
//! interrompida segurando uma trava (spinlocks simples bastam). Cada
//! subsistema tem a sua trava e elas so podem ser aninhadas nesta ordem
//! (de fora para dentro), o que torna impossivel um deadlock:
//!
//!   STAGE (syscall.rs)  ->  FS (fs.rs)  ->  FRAME
//!   SCHED (process.rs)  ->  INPUT, SERVICES, MMU  ->  FRAME
//!   MMU (aqui)          ->  FRAME (frame_allocator.rs)
//!   GPU, NET, CONSOLE   (folhas: nao pegam nenhuma outra trava)
//!
//! Assim duas CPUs podem, ao mesmo tempo, ler o disco, apresentar um quadro,
//! falar com a rede e trocar mensagens de IPC: so disputam o que de fato
//! compartilham.
use core::sync::atomic::{AtomicBool, Ordering};

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
                return;
            }
            while self.locked.load(Ordering::Relaxed) {
                core::hint::spin_loop();
            }
        }
    }

    #[inline]
    pub fn unlock(&self) {
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
