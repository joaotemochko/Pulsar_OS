#![no_std]
#![no_main]

mod blk;
mod context;
mod cpu;
mod exceptions;
mod fb;
mod frame_allocator;
mod gic;
mod gpu;
mod input;
mod irq;
mod loader;
mod mmu;
mod pfs;
mod nbfs;
mod fs;
mod process;
mod pulse;
mod syscall;
mod timer;
mod uart;
mod virtio;
mod net;

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

    // GPU (virtio-gpu 2D): framebuffer + padrao de teste
    serial.write_string("Inicializando virtio-gpu...\n");
    match gpu::init() {
        Some((w, h)) => {
            fb::init(w, h);
            fb::draw_test_pattern();
            gpu::present();
            let _ = write!(serial, "[gpu] frame apresentado ({}x{})\n", w, h);
        }
        None => serial.write_string("[gpu] nao encontrado — seguindo sem video\n"),
    }

    // GIC + timer prontos, mas IRQs ainda MASCARADAS: o boot nao pode
    // ser preemptado no meio (leitura de disco, carga do shell).
    serial.write_string("Inicializando GIC + timer...\n");
    gic::init();
    gic::enable_irq(timer::TIMER_IRQ);

    // Armazenamento + sistema de arquivos
    serial.write_string("Inicializando virtio-blk...\n");
    if blk::init().is_none() {
        serial.write_string("[kernel] sem disco — nada a executar. Parando.\n");
        loop { unsafe { asm!("wfe") }; }
    }
    if fs::mount().is_none() {
        serial.write_string("[kernel] PulsarFS invalido. Parando.\n");
        loop { unsafe { asm!("wfe") }; }
    }

    // Input (teclado + tablet)
    let n_input = input::init();
    let _ = write!(serial, "[input] {} dispositivo(s) de entrada\n", n_input);

    // Rede (virtio-net). Nao-fatal: o SO funciona sem rede.
    if net::init() {
        // tenta resolver o gateway via ARP (prova que RX/TX funcionam)
        net::resolve_gateway();
        // envia um datagrama UDP de teste ao gateway (porta 9)
        if net::udp_send(&net::GW_IP, 9, 40000, b"Pulsar OS online") {
            let _ = write!(serial, "[net] datagrama UDP de teste enviado ao gateway\n");
        }
    }

    // Carrega o shell grafico do disco
    // Inicia o filesystem daemon PRIMEIRO (pid 0) para registrar SVC_FS,
    // depois o shell (pid 1). O shell espera o fsd via lookup antes de usar.
    serial.write_string("Iniciando filesystem daemon (fsd.pulse)...\n");
    loader::spawn_from_fs("fsd.pulse");
    serial.write_string("Carregando shell.pulse do disco...\n");
    if loader::spawn_from_fs("shell.pulse").is_none() {
        loop { unsafe { asm!("wfe") }; }
    }

    // So agora habilita preempcao e entra em EL0
    timer::arm(100); // 100 Hz: cursor fluido
    unsafe { asm!("msr daifclr, #2", options(nomem, nostack, preserves_flags)); }

    serial.write_string("Entrando no shell (EL0)...\n");
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