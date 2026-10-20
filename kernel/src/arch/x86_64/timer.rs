//! Timer para x86_64: RDTSC para carimbo de tempo monotônico de alta precisão
//! e PIT (Intel 8254) para interrupções periódicas a 100 Hz no IRQ 0.

use super::uart::outb;

pub const TIMER_FREQ: u64 = 2_000_000_000; // 2 GHz referência

#[inline]
pub fn now() -> u64 {
    let lo: u32;
    let hi: u32;
    unsafe {
        core::arch::asm!(
            "rdtsc",
            out("eax") lo,
            out("edx") hi,
            options(nomem, nostack, preserves_flags)
        );
    }
    ((hi as u64) << 32) | (lo as u64)
}

#[inline]
pub fn freq() -> u64 {
    TIMER_FREQ
}

/// Configura o PIT a 100 Hz (divisor 11932).
pub fn init_pit() {
    unsafe {
        // Canal 0, modo de onda quadrada (modo 3), leitura/escrita lobyte/hibyte
        outb(0x43, 0x36);
        let divisor: u16 = 11932;
        outb(0x40, (divisor & 0xFF) as u8);
        outb(0x40, (divisor >> 8) as u8);
    }
}

pub fn set_deadline(_t: u64) {
    // No PIT periódico, o tick de 100 Hz roda continuamente no IRQ 0
}

