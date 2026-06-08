#![no_std]
#![no_main]

mod cpu;
mod exceptions;
mod frame_allocator;
mod mmu;
mod syscall;
mod uart;
mod user;
mod loader;
mod pulse;

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

    serial.write_string("Inicializando frame allocator...\n");
    frame_allocator::init();
    let _ = write!(serial, "Frames livres: {}\n", frame_allocator::free_count());

    serial.write_string("Ligando MMU agora...\n");
    unsafe { mmu::init() };
    let _ = write!(serial, "MMU ativada: M = {}\n", mmu::is_enabled() as u32);
    let _ = write!(serial, "Frames livres apos montar tabelas: {}\n", frame_allocator::free_count());

    serial.write_string("Carregando programa .pulse...\n");
    let entry = unsafe {
        loader::load_pulse(&user::pulse_file_start as *const u8)
    };

    match entry {
        Some(ep) => {
            let _ = write!(serial, "Saltando para EL0 no entry {:#x}...\n", ep);
            unsafe { cpu::enter_el0(ep, mmu::USER_STACK_TOP) };
        }
        None => {
            serial.write_string("[loader] FALHOU ao carregar .pulse\n");
            loop { unsafe { asm!("wfe") }; }
        }
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    let serial = Uart;
    let _ = serial.write_string("\n!!! KERNEL PANIC !!!\n");
    loop {
        unsafe { asm!("wfe") };
    }
}