use core::arch::asm;

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

/// Configura o estado de retorno e faz ERET para `entry` rodando em EL0,
/// usando `user_sp` como SP_EL0. Nao retorna (saimos deste fluxo via EL0).
pub unsafe fn enter_el0(entry: u64, user_sp: u64) -> ! {
    let spsr: u64 = 0x3C0; // M[3:0]=0000 (EL0t) + DAIF mascarado
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