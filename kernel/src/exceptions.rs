use crate::uart::Uart;
use core::fmt::Write;

#[unsafe(no_mangle)]
pub extern "C" fn rust_exception_dispatch(index: u64, esr: u64, elr: u64) {
    let mut serial = Uart;
    let ec = (esr >> 26) & 0x3F;
    let _ = write!(
        serial,
        "\n[EXCECAO NAO TRATADA] indice={} EC={:#x} ESR={:#x} ELR={:#x}\n",
        index, ec, esr, elr
    );
}