use core::fmt;
use core::ptr::{read_volatile, write_volatile};

const UART0_BASE: usize = 0x0900_0000;
const UART0_DR: *mut u32 = UART0_BASE as *mut u32;              // Data Register
const UART0_FR: *const u32 = (UART0_BASE + 0x18) as *const u32; // Flag Register
const FR_TXFF: u32 = 1 << 5;                                    // Transmit FIFO Full

pub struct Uart;

impl Uart {
    pub fn write_byte(&self, byte: u8) {
        unsafe {
            while read_volatile(UART0_FR) & FR_TXFF != 0 {} // espera a FIFO esvaziar
            write_volatile(UART0_DR, byte as u32);
        }
    }

    pub fn write_string(&self, s: &str) {
        for b in s.bytes() {
            if b == b'\n' { self.write_byte(b'\r'); }
            self.write_byte(b);
        }
    }
}

impl fmt::Write for Uart {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write_string(s);
        Ok(())
    }
}