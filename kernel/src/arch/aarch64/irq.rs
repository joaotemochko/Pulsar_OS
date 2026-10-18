//! Pontos de entrada Rust chamados pelos vetores (vectors.S).
use super::{gic, timer, Context};

/// IRQ (timer ou IPI). Vem de EL0 ou do laco ocioso em EL1.
#[unsafe(no_mangle)]
pub extern "C" fn rust_irq_handler(frame: *mut Context) {
    let iar = gic::ack();
    let irq = iar & 0x3FF;
    if irq == timer::TIMER_IRQ {
        crate::trap::on_timer(frame);
    } else if irq < 16 {
        crate::trap::on_ipi(frame);
    }
    if irq < 1020 {
        gic::eoi(iar);
    }
}

/// Excecao sincrona de EL0: syscall (SVC) ou fault do programa.
#[unsafe(no_mangle)]
pub extern "C" fn rust_el0_sync_handler(frame: *mut Context) {
    let esr: u64;
    let far: u64;
    unsafe {
        core::arch::asm!("mrs {}, esr_el1", out(reg) esr, options(nomem, nostack, preserves_flags));
        core::arch::asm!("mrs {}, far_el1", out(reg) far, options(nomem, nostack, preserves_flags));
    }
    let ec = (esr >> 26) & 0x3F;
    if ec == 0x15 {
        // SVC: ELR ja aponta para a instrucao seguinte.
        crate::trap::on_syscall(frame);
    } else if ec == 0x20 || ec == 0x24 {
        // abort de EL0: falta de traducao (0b0001xx) ou de permissao
        // (0b0011xx) pode ser uma pagina preguicosa do mmap
        use crate::vm::Access;
        let fsc = esr & 0x3C;
        let (what, access) = if ec == 0x20 {
            ("Instruction Abort", Access::Exec)
        } else if esr & (1 << 6) != 0 {
            ("Data Abort", Access::Write)
        } else {
            ("Data Abort", Access::Read)
        };
        if fsc == 0x04 || fsc == 0x0C {
            crate::trap::on_page_fault(frame, what, far, access);
        } else if esr & 0x3F == 0x21 {
            crate::trap::on_user_fault_sig(frame, "acesso desalinhado", far, crate::signal::SIGBUS, 1);
        } else {
            crate::trap::on_user_fault(frame, what, far);
        }
    } else {
        // sinal e si_code do Linux para cada classe de excecao
        let pc = unsafe { (*frame).pc() };
        let (what, sig, code, addr) = match ec {
            0x22 | 0x26 => ("pc/sp desalinhado", crate::signal::SIGBUS, 1, far),
            0x3C => ("breakpoint", crate::signal::SIGTRAP, 1, pc),
            0x2C => ("excecao de ponto flutuante", crate::signal::SIGFPE, 0, pc),
            0x00 => ("instrucao invalida", crate::signal::SIGILL, 1, pc),
            _ => ("excecao sincrona", crate::signal::SIGILL, 1, pc),
        };
        crate::trap::on_user_fault_sig(frame, what, addr, sig, code);
    }
}
