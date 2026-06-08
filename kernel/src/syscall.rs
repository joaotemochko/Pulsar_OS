use crate::uart::Uart;
use core::fmt::Write;
use core::slice;
use core::str;

const SYS_WRITE: u64 = 1;

/// Classificador de exceções síncronas de EL0 (indice 8 da tabela de vetores).
/// Decide entre syscall (SVC) e fault, lendo o EC do ESR.
#[unsafe(no_mangle)]
pub extern "C" fn rust_el0_sync_handler(
    arg1: u64, arg2: u64, arg3: u64, syscall_num: u64, esr: u64,
) -> i64 {
    let ec = (esr >> 26) & 0x3F;
    if ec == 0x15 {
        // SVC: syscall de verdade
        rust_syscall_dispatch(arg1, arg2, arg3, syscall_num)
    } else {
        // Qualquer outra exceção sincrona de EL0 = fault do programa
        let mut serial = Uart;
        let far: u64;
        unsafe {
            core::arch::asm!("mrs {}, far_el1", out(reg) far,
                options(nomem, nostack, preserves_flags));
        }
        let _ = write!(serial,
            "\n[FAULT EL0] programa violou permissao! EC={:#x} FAR={:#x}\n", ec, far);
        let _ = write!(serial,
            "  (data abort em pagina R+X = tentativa de ESCRITA em codigo -> W^X funcionou!)\n");
        loop { unsafe { core::arch::asm!("wfe") }; }
    }
}

/// Dispatcher de syscalls propriamente dito.
#[unsafe(no_mangle)]
pub extern "C" fn rust_syscall_dispatch(arg1: u64, arg2: u64, _arg3: u64, num: u64) -> i64 {
    match num {
        SYS_WRITE => sys_write(arg1, arg2),
        _ => -1,
    }
}

fn sys_write(ptr: u64, len: u64) -> i64 {
    if ptr == 0 || len == 0 || len > 4096 {
        return -2;
    }
    let bytes = unsafe { slice::from_raw_parts(ptr as *const u8, len as usize) };
    match str::from_utf8(bytes) {
        Ok(s) => {
            Uart.write_string(s);
            len as i64
        }
        Err(_) => -3,
    }
}