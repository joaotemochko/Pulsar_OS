//! Tratamento generico de traps. Os backends decodificam a causa e chamam
//! estas funcoes; cada subsistema pega a sua propria trava (ver sync.rs).
//! Na volta de toda trap, sinais pendentes da thread corrente sao entregues
//! (signal::deliver), seja ela a que entrou ou outra escalonada no caminho.
use crate::arch::Context;
use crate::process;
use crate::signal;
use crate::uart::Uart;
use core::fmt::Write;

pub fn on_syscall(frame: *mut Context) {
    if unsafe { process::exit_if_killed(frame) } {
        return;
    }
    if process::current_abi() == process::ABI_LINUX {
        crate::linux::dispatch(frame);
    } else {
        crate::syscall::dispatch(frame);
    }
    signal::deliver(frame);
}

/// Falha do programa. Processos Linux com tratador para o sinal
/// correspondente (`sig`, codigo `code`) o recebem; os demais morrem.
pub fn on_user_fault_sig(frame: *mut Context, what: &str, addr: u64, sig: u32, code: i32) {
    if signal::fault_signal(sig, code, addr) {
        signal::deliver(frame);
        return;
    }
    let mut serial = Uart;
    let pc = unsafe { (*frame).pc() };
    let pid = process::current_pid();
    let _ = write!(serial,
        "\n[FAULT] pid {} ({}) — {} em {:#x} (pc={:#x}, cpu {}). Processo encerrado.\n",
        pid, process::current_name(), what, addr, pc, crate::arch::cpu_id());
    process::note_crash(pid);
    process::set_status(sig, true);
    unsafe { process::exit_group(frame) };   // a falha de uma thread derruba o processo
}

#[cfg_attr(target_arch = "riscv64", allow(dead_code))]
pub fn on_user_fault(frame: *mut Context, what: &str, addr: u64) {
    on_user_fault_sig(frame, what, addr, signal::SIGSEGV, 1);
}

/// Page fault de usuario: paginas preguicosas do mmap sao criadas aqui;
/// o resto e SIGSEGV (MAPERR sem pagina, ACCERR com pagina sem permissao).
pub fn on_page_fault(frame: *mut Context, what: &str, addr: u64, access: crate::vm::Access) {
    if crate::vm::fault(addr, access) {
        signal::deliver(frame);
        return;
    }
    let mapped = process::current_space().is_some_and(|(_, root, _)| crate::arch::mmu::is_mapped(root, addr & !4095));
    on_user_fault_sig(frame, what, addr, signal::SIGSEGV, if mapped { 2 } else { 1 });
}

pub fn on_timer(frame: *mut Context) {
    if unsafe { process::exit_if_killed(frame) } {
        return;
    }
    unsafe { process::preempt(frame) };
    signal::deliver(frame);
}

pub fn on_ipi(frame: *mut Context) {
    if unsafe { process::exit_if_killed(frame) } {
        return;
    }
    unsafe { process::on_ipi(frame) };
    signal::deliver(frame);
}
