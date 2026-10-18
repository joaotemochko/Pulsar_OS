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
        // Excecao dentro do kernel: bug.
        let mut serial = crate::uart::RawUart;
        use core::fmt::Write;
        let _ = write!(serial,
            "\n[EXCECAO NO KERNEL] hart={} scause={:#x} stval={:#x} sepc={:#x}\n",
            super::cpu_id(), scause, stval, unsafe { (*frame).sepc });
        // vira um oops: o processo atendido morre, o sistema segue
        panic!("excecao no kernel (scause {:#x}) em {:#x}, endereco {:#x}", scause, unsafe { (*frame).sepc }, stval);
    }
    if scause == EXC_ECALL_U {
        unsafe { (*frame).sepc += 4 };
        crate::trap::on_syscall(frame);
    } else if scause == 12 || scause == 13 || scause == 15 {
        use crate::vm::Access;
        let (what, access) = match scause {
            12 => ("Instruction Page Fault", Access::Exec),
            13 => ("Load Page Fault", Access::Read),
            _ => ("Store Page Fault", Access::Write),
        };
        crate::trap::on_page_fault(frame, what, stval, access);
    } else {
        use crate::signal::{SIGBUS, SIGILL, SIGSEGV, SIGTRAP};
        let pc = unsafe { (*frame).sepc };
        let (what, sig, code, addr) = match scause {
            0 => ("instrucao desalinhada", SIGBUS, 1, stval),
            4 | 6 => ("acesso desalinhado", SIGBUS, 1, stval),
            1 => ("Instruction Access Fault", SIGSEGV, 2, stval),
            5 => ("Load Access Fault", SIGSEGV, 2, stval),
            7 => ("Store Access Fault", SIGSEGV, 2, stval),
            3 => ("breakpoint", SIGTRAP, 1, pc),
            2 => ("instrucao ilegal", SIGILL, 1, pc),
            _ => ("excecao sincrona", SIGILL, 1, pc),
        };
        crate::trap::on_user_fault_sig(frame, what, addr, sig, code);
    }
}
