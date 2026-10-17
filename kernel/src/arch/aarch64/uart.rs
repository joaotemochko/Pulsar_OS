//! PL011 UART (somente transmissao, por polling).
use core::ptr::{read_volatile, write_volatile};
use super::platform::UART_BASE;

const DR: u64 = 0x00;
const FR: u64 = 0x18;
const FR_TXFF: u32 = 1 << 5;

#[inline]
pub fn putc(byte: u8) {
    unsafe {
        while read_volatile((UART_BASE + FR) as *const u32) & FR_TXFF != 0 {}
        write_volatile((UART_BASE + DR) as *mut u32, byte as u32);
    }
}
