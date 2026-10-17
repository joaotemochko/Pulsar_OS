//! Despacho de traps RISC-V (chamado por trap.S).
use super::Context;

const INTERRUPT: u64 = 1 << 63;
const IRQ_S_SOFT: u64 = 1;
const IRQ_S_TIMER: u64 = 5;
const EXC_ECALL_U: u64 = 8;
const SIP_SSIP: u64 = 1 << 1;

#[unsafe(no_mangle)]
pub extern "C" fn rust_trap(frame: *mut Context) {
    let scause: u64;
    let stval: u64;
    unsafe {
        core::arch::asm!("csrr {}, scause", out(reg) scause, options(nomem, nostack));
        core::arch::asm!("csrr {}, stval", out(reg) stval, options(nomem, nostack));
    }
    let from_user = unsafe { (*frame).sstatus } & (1 << 8) == 0;

    if scause & INTERRUPT != 0 {
        let code = scause & !INTERRUPT;
        match code {
            IRQ_S_TIMER => crate::trap::on_timer(frame),
            IRQ_S_SOFT => {
                unsafe { core::arch::asm!("csrc sip, {}", in(reg) SIP_SSIP, options(nomem, nostack)) };
                crate::trap::on_ipi(frame);
            }
            _ => {}
        }
        return;
    }

    if !from_user {
        // Excecao dentro do kernel: bug. Reporta e para esta hart.
        let mut serial = crate::uart::RawUart;
        use core::fmt::Write;
        let _ = write!(serial,
            "\n[EXCECAO NO KERNEL] hart={} scause={:#x} stval={:#x} sepc={:#x}\n",
            super::cpu_id(), scause, stval, unsafe { (*frame).sepc });
        loop { super::wait_for_interrupt(); }
    }
    if scause == EXC_ECALL_U {
        unsafe { (*frame).sepc += 4 };
        crate::trap::on_syscall(frame);
    } else {
        let what = match scause {
            0 => "instrucao desalinhada",
            1 => "Instruction Access Fault",
            2 => "instrucao ilegal",
            5 => "Load Access Fault",
            7 => "Store Access Fault",
            12 => "Instruction Page Fault",
            13 => "Load Page Fault",
            15 => "Store Page Fault",
            _ => "excecao sincrona",
        };
        crate::trap::on_user_fault(frame, what, stval);
    }
}
