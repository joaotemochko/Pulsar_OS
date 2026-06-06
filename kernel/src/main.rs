#![no_std]
#![no_main]

// Declara o módulo uart que acabamos de criar
mod uart;

use core::fmt::Write;
use core::panic::PanicInfo;
use uart::Uart;

core::arch::global_asm!(include_str!("arch/aarch64/boot.S"));

#[no_mangle]
pub extern "C" fn kernel_main() -> ! {
    // Instancia o nosso driver UART
    let mut serial = Uart;

    // Escreve uma mensagem simples usando o método nativo do driver
    serial.write_string("\n====================================\n");
    serial.write_string(" Olá, Universo! Pulsar Kernel ativo. \n");
    serial.write_string("====================================\n\n");

    // Demonstração do poder da trait fmt::Write combinada com macros
    // O compilador do Rust monta toda a formatação estaticamente em tempo de compilação!
    let _ = write!(serial, "Arquitetura: AArch64 (ARM 64-bits)\n");
    let _ = write!(serial, "Modo do Kernel: Microkernel Privilegiado\n");

    // Loop de espera segura
    loop {
        unsafe { core::arch::asm!("wfe") };
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    // Caso o kernel entre em colapso futuramente, avisa pela UART
    let mut serial = Uart;
    let _ = serial.write_string("\n!!! KERNEL PANIC !!!\n");
    
    loop {
        unsafe { core::arch::asm!("wfe") };
    }
}