use core::arch::asm;
use crate::context::Context;

pub fn current_el() -> u64 {
    let el: u64;
    unsafe {
        asm!("mrs {}, CurrentEL", out(reg) el, options(nomem, nostack, preserves_flags));
    }
    (el >> 2) & 0b11
}

pub fn set_vbar_el1(addr: u64) {
    unsafe {
        asm!("msr vbar_el1, {}", "isb", in(reg) addr, options(nostack, preserves_flags));
    }
}

/// Salta para EL0 num entry/stack (usado no modo single-process antigo).
pub unsafe fn enter_el0(entry: u64, user_sp: u64) -> ! {
    let spsr: u64 = 0x3C0;
    unsafe {
        asm!(
            "msr sp_el0,   {sp}",
            "msr spsr_el1, {spsr}",
            "msr elr_el1,  {entry}",
            "isb",
            "eret",
            sp    = in(reg) user_sp,
            spsr  = in(reg) spsr,
            entry = in(reg) entry,
            options(noreturn)
        );
    }
}

/// Parte o primeiro processo a partir de um Context salvo.
pub unsafe fn start_first(ctx: &Context) -> ! {
    unsafe {
        asm!(
            "msr sp_el0,   {sp}",
            "msr elr_el1,  {elr}",
            "msr spsr_el1, {spsr}",
            "mov x0, {x0}",
            "isb",
            "eret",
            sp   = in(reg) ctx.sp_el0,
            elr  = in(reg) ctx.elr,
            spsr = in(reg) ctx.spsr,
            x0   = in(reg) ctx.x[0],
            options(noreturn)
        );
    }
}