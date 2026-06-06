use core::fmt;
use core::ptr::write_volatile;

// Endereço base do registrador de dados da UART0 (PL011)
const UART0_DR: *mut u32 = 0x3F20_1000 as *mut u32;

pub struct Uart;

impl Uart {
    /// Envia um único byte bruto para a interface serial
    pub fn write_byte(&self, byte: u8) {
        unsafe {
            // Escreve o byte diretamente no endereço mapeado da UART
            write_volatile(UART0_DR, byte as u32);
        }
    }

    /// Envia uma sequência de texto (string) caractere por caractere
    pub fn write_string(&self, s: &str) {
        for byte in s.bytes() {
            self.write_byte(byte);
        }
    }
}

impl fmt::Write for Uart {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write_string(s);
        Ok(())
    }
}