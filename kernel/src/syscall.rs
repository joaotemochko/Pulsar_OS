use crate::uart::Uart;
use core::slice;
use core::str;

const SYS_WRITE: u64 = 1;

#[unsafe(no_mangle)]
pub extern "C" fn rust_syscall_dispatch(arg1: u64, arg2: u64, _arg3: u64, num: u64) -> i64 {
    match num {
        SYS_WRITE => sys_write(arg1, arg2),
        _ => -1, // syscall desconhecida
    }
}

/// SYS_WRITE(ptr, len): kernel imprime a string em nome do processo de EL0.
/// Retorna o numero de bytes escritos, ou negativo em erro.
fn sys_write(ptr: u64, len: u64) -> i64 {
    if ptr == 0 || len == 0 || len > 4096 {
        return -2; // argumentos invalidos
    }
    let bytes = unsafe { slice::from_raw_parts(ptr as *const u8, len as usize) };
    match str::from_utf8(bytes) {
        Ok(s) => {
            Uart.write_string(s);
            len as i64
        }
        Err(_) => -3, // utf-8 invalido
    }
}