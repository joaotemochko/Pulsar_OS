//! Relogio monotonico do sistema (contador de hardware da arquitetura).
use crate::arch;

/// Quantum de preempcao.
pub const QUANTUM_MS: u64 = 10;

pub fn now_ms() -> u64 {
    let f = arch::timer_freq();
    let t = arch::now();
    (t / f) * 1000 + (t % f) * 1000 / f
}

pub fn ms_to_ticks(ms: u64) -> u64 {
    ms * arch::timer_freq() / 1000
}

pub fn ticks_to_ms(t: u64) -> u64 {
    t * 1000 / arch::timer_freq()
}
