//! Generic Timer (timer fisico EL1, PPI 30). Cada CPU tem o seu.
use core::arch::asm;

pub const TIMER_IRQ: u32 = 30;

#[inline]
pub fn now() -> u64 {
    let v: u64;
    unsafe { asm!("isb", "mrs {}, cntpct_el0", out(reg) v, options(nomem, nostack, preserves_flags)) };
    v
}

#[inline]
pub fn freq() -> u64 {
    let v: u64;
    unsafe { asm!("mrs {}, cntfrq_el0", out(reg) v, options(nomem, nostack, preserves_flags)) };
    v
}

/// Dispara o IRQ do timer desta CPU no instante absoluto `t`.
pub fn set_deadline(t: u64) {
    unsafe {
        asm!("msr cntp_cval_el0, {}", in(reg) t, options(nomem, nostack, preserves_flags));
        asm!("msr cntp_ctl_el0, {}", in(reg) 1u64, options(nomem, nostack, preserves_flags));
        asm!("isb", options(nomem, nostack, preserves_flags));
    }
}
