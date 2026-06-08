use core::arch::asm;

pub const TIMER_IRQ: u32 = 30; // PPI do timer fisico EL1

/// Arma o timer para disparar daqui a `ticks_fraction` da frequencia.
/// Ex.: fraction=10 -> dispara a cada 1/10 de segundo.
pub fn arm(fraction: u64) {
    unsafe {
        let freq: u64;
        asm!("mrs {}, cntfrq_el0", out(reg) freq, options(nomem, nostack, preserves_flags));
        let interval = freq / fraction;
        // carrega o valor de contagem regressiva
        asm!("msr cntp_tval_el0, {}", in(reg) interval, options(nomem, nostack, preserves_flags));
        // habilita o timer (bit 0 = enable, bit 1 = mask; queremos enable e nao-mascarado)
        asm!("msr cntp_ctl_el0, {}", in(reg) 1u64, options(nomem, nostack, preserves_flags));
    }
}

/// Rearma o timer para o proximo tick (chamado no handler de IRQ).
pub fn rearm(fraction: u64) {
    arm(fraction);
}