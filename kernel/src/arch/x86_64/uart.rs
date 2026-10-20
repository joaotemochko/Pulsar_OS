//! Driver de console serial por polling (COM1 em 0x3F8).
//! Na máquina microvm / PC do QEMU, a porta serial ISA COM1 já vem
//! pré-configurada pelo firmware / emulador.

const PORT_COM1: u16 = 0x3F8;
const PORT_LSR: u16  = PORT_COM1 + 5;
const LSR_THRE: u8   = 1 << 5; // Transmitter Holding Register Empty

#[inline]
pub unsafe fn inb(port: u16) -> u8 {
    let ret: u8;
    unsafe {
        core::arch::asm!("in al, dx", in("dx") port, out("al") ret, options(nomem, nostack, preserves_flags));
    }
    ret
}

#[inline]
pub unsafe fn outb(port: u16, val: u8) {
    unsafe {
        core::arch::asm!("out dx, al", in("dx") port, in("al") val, options(nomem, nostack, preserves_flags));
    }
}



#[inline]
pub fn putc(byte: u8) {
    unsafe {
        // Aguarda transmissor estar vazio
        while inb(PORT_LSR) & LSR_THRE == 0 {}
        outb(PORT_COM1, byte);
    }
}
