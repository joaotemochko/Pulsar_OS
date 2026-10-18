//! Console serial generico (o driver real vive no backend da arquitetura).
use crate::sync::SpinLock;
use core::fmt;

/// Evita que mensagens de CPUs diferentes se misturem no meio da linha.
static CONSOLE: SpinLock = SpinLock::new();

pub struct Uart;

impl Uart {
    pub fn write_byte(&self, byte: u8) {
        crate::arch::console_putc(byte);
    }

    pub fn write_string(&self, s: &str) {
        let _g = CONSOLE.guard();
        for b in s.bytes() {
            if b == b'\n' { self.write_byte(b'\r'); }
            self.write_byte(b);
        }
    }
}

/// Bytes crus (descritores 1/2 do console): sem exigir UTF-8.
pub fn write_bytes(bytes: &[u8]) {
    let _g = CONSOLE.guard();
    for &b in bytes {
        if b == b'\n' { crate::arch::console_putc(b'\r'); }
        crate::arch::console_putc(b);
    }
}

impl fmt::Write for Uart {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write_string(s);
        Ok(())
    }
}

/// Escritor sem trava, para o caminho de panico/excecao (a CPU pode ter
/// caido justamente segurando a trava do console).
pub struct RawUart;

impl fmt::Write for RawUart {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for b in s.bytes() {
            if b == b'\n' { crate::arch::console_putc(b'\r'); }
            crate::arch::console_putc(b);
        }
        Ok(())
    }
}
