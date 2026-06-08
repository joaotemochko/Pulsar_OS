#![no_std]
#![no_main]

mod context;
mod cpu;
mod exceptions;
mod frame_allocator;
mod gic;
mod irq;
mod loader;
mod mmu;
mod process;
mod pulse;
mod syscall;
mod timer;
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

    serial.write_string("Inicializando frame allocator...\n");
    frame_allocator::init();
    let _ = write!(serial, "Frames livres: {}\n", frame_allocator::free_count());

    serial.write_string("Ligando MMU agora...\n");
    unsafe { mmu::init() };
    let _ = write!(serial, "MMU ativada: M = {}\n", mmu::is_enabled() as u32);

    // GIC + timer (preempcao)
    serial.write_string("Inicializando GIC + timer...\n");
    gic::init();
    gic::enable_irq(timer::TIMER_IRQ);
    timer::arm(10);
    unsafe { asm!("msr daifclr, #2", options(nomem, nostack, preserves_flags)); }

    // Cria dois processos que NAO cooperam (loops infinitos sem yield)
    serial.write_string("Carregando processos A e B...\n");
    let entry_a = unsafe { loader::load_pulse(&user::pulse_a_start as *const u8) }.unwrap();
    let _pid_a = process::create(entry_a, 0x4010_2000);
    let entry_b = unsafe { loader::load_pulse(&user::pulse_b_start as *const u8) }.unwrap();
    let _pid_b = process::create(entry_b, 0x4011_2000);

    serial.write_string("Preempcao ativa. Os processos NAO dao yield — o timer forca a troca:\n");
    let first = unsafe { process::first_context() };
    unsafe { cpu::start_first(&first) };
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    let serial = Uart;
    let _ = serial.write_string("\n!!! KERNEL PANIC !!!\n");
    loop {
        unsafe { asm!("wfe") };
    }
}