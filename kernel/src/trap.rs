//! Tratamento generico de traps. Os backends decodificam a causa e chamam
//! estas funcoes; cada subsistema pega a sua propria trava (ver sync.rs).
use crate::arch::Context;
use crate::process;
use crate::uart::Uart;
use core::fmt::Write;

pub fn on_syscall(frame: *mut Context) {
    if unsafe { process::exit_if_killed(frame) } {
        return;
    }
    crate::syscall::dispatch(frame);
}

pub fn on_user_fault(frame: *mut Context, what: &str, addr: u64) {
    let mut serial = Uart;
    let pc = unsafe { (*frame).pc() };
    let pid = process::current_pid();
    let _ = write!(serial,
        "\n[FAULT] pid {} ({}) — {} em {:#x} (pc={:#x}, cpu {}). Processo encerrado.\n",
        pid, process::current_name(), what, addr, pc, crate::arch::cpu_id());
    process::note_crash(pid);
    unsafe { process::exit_group(frame) };   // a falha de uma thread derruba o processo
}

pub fn on_timer(frame: *mut Context) {
    if unsafe { process::exit_if_killed(frame) } {
        return;
    }
    unsafe { process::preempt(frame) };
}

pub fn on_ipi(frame: *mut Context) {
    if unsafe { process::exit_if_killed(frame) } {
        return;
    }
    unsafe { process::on_ipi(frame) };
}
