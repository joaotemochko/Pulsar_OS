use core::fmt::Write;

unsafe extern "C" {
    static vector_table: u8;
}

/// Instala a tabela de vetores em VBAR_EL1 desta CPU.
pub fn install_vectors() {
    let vbar = unsafe { &vector_table as *const _ as u64 };
    unsafe {
        core::arch::asm!("msr vbar_el1, {}", "isb", in(reg) vbar, options(nostack, preserves_flags));
    }
}

/// Excecao que o kernel nao sabe tratar (bug em EL1): reporta e trata como
/// oops (ver o panic handler).
#[unsafe(no_mangle)]
pub extern "C" fn rust_exception_dispatch(index: u64, esr: u64, elr: u64) {
    let mut serial = crate::uart::RawUart;
    let far: u64;
    unsafe { core::arch::asm!("mrs {}, far_el1", out(reg) far, options(nomem, nostack, preserves_flags)) };
    let ec = (esr >> 26) & 0x3F;
    let _ = write!(
        serial,
        "\n[EXCECAO NO KERNEL] cpu={} indice={} EC={:#x} ESR={:#x} ELR={:#x} FAR={:#x}\n",
        super::cpu_id(), index, ec, esr, elr, far
    );
    // vira um oops: o processo atendido morre, o sistema segue
    panic!("excecao no kernel (EC {:#x}) em {:#x}, endereco {:#x}", ec, elr, far);
}
