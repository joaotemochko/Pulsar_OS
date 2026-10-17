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
    } else {
        let what = match ec {
            0x20 | 0x21 => "Instruction Abort",
            0x24 | 0x25 => "Data Abort",
            0x00 => "instrucao invalida",
            0x07 => "acesso a FP/SIMD (softfloat)",
            _ => "excecao sincrona",
        };
        crate::trap::on_user_fault(frame, what, far);
    }
}
