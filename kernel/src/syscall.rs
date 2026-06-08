use crate::uart::Uart;
use crate::context::Context;
use crate::process;
use core::fmt::Write;
use core::slice;
use core::str;

const SYS_WRITE: u64 = 1;
const SYS_YIELD: u64 = 2;

#[unsafe(no_mangle)]
pub extern "C" fn rust_el0_sync_handler(frame: *mut Context) {
    let ctx = unsafe { &mut *frame };

    let esr_val: u64;
    unsafe {
        core::arch::asm!("mrs {}, esr_el1", out(reg) esr_val,
            options(nomem, nostack, preserves_flags));
    }
    let ec = (esr_val >> 26) & 0x3F;

    if ec != 0x15 {
        // Qualquer exceção sincrona de EL0 que NAO seja SVC = fault do programa.
        // Reporta, mata o processo faltoso e troca para o proximo.
        let mut serial = Uart;
        let far: u64;
        unsafe {
            core::arch::asm!("mrs {}, far_el1", out(reg) far,
                options(nomem, nostack, preserves_flags));
        }
        let _ = write!(serial, "\n[FAULT EL0] EC={:#x} FAR={:#x} — matando processo.\n", ec, far);

        let (dead, has_next) = unsafe { process::exit_current(frame) };
        let _ = write!(serial, "[kernel] processo {} terminado.\n", dead);

        if !has_next {
            let _ = write!(serial, "[kernel] nenhum processo restante. Sistema ocioso.\n");
            loop { unsafe { core::arch::asm!("wfe") }; }
        }
        // ha proximo: exit_current ja colocou o contexto dele no frame;
        // o RESTORE_CONTEXT/eret do vetor vai retomar esse proximo processo.
        return;
    }

    // SVC: syscall de verdade. Numero em x8, args em x0..x2 (lidos do frame).
    let num = ctx.x[8];
    let a1 = ctx.x[0];
    let a2 = ctx.x[1];

    match num {
        SYS_WRITE => {
            ctx.x[0] = sys_write(a1, a2) as u64;
        }
        SYS_YIELD => {
            unsafe { process::schedule(frame); }
        }
        _ => {
            ctx.x[0] = (-1i64) as u64;
        }
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