//! NS16550A (somente transmissao, por polling).
use core::ptr::{read_volatile, write_volatile};

const THR: u64 = 0;
const LSR: u64 = 5;
const LSR_THRE: u8 = 1 << 5;

#[inline]
pub fn putc(byte: u8) {
    let base = super::mmio(super::platform::UART_PA);
    unsafe {
        while read_volatile((base + LSR) as *const u8) & LSR_THRE == 0 {}
        write_volatile((base + THR) as *mut u8, byte);
    }
}
