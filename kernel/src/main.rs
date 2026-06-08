#![no_std]
#![no_main]

mod cpu;
mod exceptions;
mod mmu;
mod syscall;
mod uart;
mod user;

use core::arch::asm;
use core::fmt::Write;
use core::panic::PanicInfo;
use uart::Uart;

core::arch::global_asm!(include_str!("arch/aarch64/boot.S"));
core::arch::global_asm!(include_str!("arch/aarch64/vectors.S"));

unsafe extern "C" {
    static vector_table: u8;
}

#[unsafe(no_mangle)]
pub extern "C" fn kernel_main() -> ! {
    let mut serial = Uart;
    serial.write_string("\n====================================\n");
    serial.write_string("  Pulsar OS — sinal de vida (AArch64)\n");
    serial.write_string("====================================\n");

    let el = cpu::current_el();
    let _ = write!(serial, "Exception Level atual: EL{}\n", el);

    let vbar = unsafe { &vector_table as *const _ as u64 };
    cpu::set_vbar_el1(vbar);
    let _ = write!(serial, "VBAR_EL1 instalado em {:#x}\n", vbar);

    serial.write_string("Ligando MMU agora...\n");
    unsafe { mmu::init() };
    let _ = write!(serial, "MMU ativada: M = {}\n", mmu::is_enabled() as u32);

    // Copia o programa de usuario (assembly PIC) para a regiao EL0 mapeada
    serial.write_string("Copiando programa de usuario para regiao EL0...\n");
    unsafe {
        let start = &user::user_program_start as *const u8;
        let end = &user::user_program_end as *const u8;
        let len = end as usize - start as usize;
        core::ptr::copy_nonoverlapping(start, mmu::USER_CODE_VA as *mut u8, len);
        asm!("dsb ish", "ic iallu", "dsb ish", "isb", options(nostack, preserves_flags));
    }
    let _ = write!(serial, "Programa de usuario copiado.\n");

    serial.write_string("Saltando para EL0 (espaco de usuario)...\n");
    unsafe { cpu::enter_el0(mmu::USER_CODE_VA, mmu::USER_STACK_TOP) };
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    let serial = Uart;
    let _ = serial.write_string("\n!!! KERNEL PANIC !!!\n");
    loop {
        unsafe { asm!("wfe") };
    }
}