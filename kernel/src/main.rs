#![no_std]
#![no_main]

mod arch;
mod blk;
mod fb;
mod frame_allocator;
mod fs;
mod gpu;
mod input;
mod loader;
mod nbfs;
mod net;
mod pfs;
mod process;
mod pulse;
mod rng;
mod sync;
mod syscall;
mod time;
mod trap;
mod uart;
mod virtio;

use core::fmt::Write;
use core::panic::PanicInfo;
use uart::Uart;

/// Entrada da CPU de boot (chamada pelo backend da arquitetura).
pub fn kernel_main(boot_cpu: usize) -> ! {
    arch::early_init();
    let mut serial = Uart;
    serial.write_string("\n====================================\n");
    let _ = write!(serial, "  Pulsar OS — {} (SMP, travas finas)\n", arch::NAME);
    serial.write_string("====================================\n");
    let _ = write!(serial, "CPU de boot: {}\n", boot_cpu);

    let kend = arch::kernel_end();
    let _ = write!(serial, "Kernel: {:#x}..{:#x}\n", arch::platform::KERNEL_BASE, kend);
    if kend > arch::platform::HEAP_START {
        serial.write_string("[kernel] imagem do kernel invade o heap! Parando.\n");
        halt();
    }

    frame_allocator::init();
    let _ = write!(serial, "RAM: {} MiB gerenciados a partir de {:#x}\n",
        arch::platform::RAM_SIZE / (1024 * 1024), arch::platform::RAM_BASE);
    let _ = write!(serial, "Frames livres: {} ({} MiB)\n",
        frame_allocator::free_count(), frame_allocator::free_count() * 4 / 1024);

    serial.write_string("Ligando MMU...\n");
    arch::mmu_init();
    let _ = write!(serial, "MMU ativada: {}\n", arch::mmu::is_enabled());

    serial.write_string("Inicializando virtio-gpu...\n");
    match gpu::init() {
        Some((w, h)) => {
            fb::init(w, h);
            fb::draw_splash();
            gpu::present();
            let _ = write!(serial, "[gpu] {}x{}, cursor por hardware\n", w, h);
        }
        None => serial.write_string("[gpu] nao encontrado — seguindo sem video\n"),
    }

    arch::irq_init_boot();

    serial.write_string("Inicializando virtio-blk...\n");
    if blk::init().is_none() {
        serial.write_string("[kernel] sem disco — nada a executar. Parando.\n");
        halt();
    }
    if fs::mount().is_none() {
        serial.write_string("[kernel] sistema de arquivos invalido. Parando.\n");
        halt();
    }

    rng::init();

    let n_input = input::init();
    let _ = write!(serial, "[input] {} dispositivo(s) de entrada\n", n_input);

    if net::init() {
        net::resolve_gateway();
        if net::udp_send(&net::GW_IP, 9, 40000, b"Pulsar OS online") {
            serial.write_string("[net] datagrama UDP de teste enviado ao gateway\n");
        }
    }

    // fsd primeiro (registra SVC_FS), depois o shell grafico.
    serial.write_string("Iniciando fsd.pulse e shell.pulse...\n");
    loader::spawn_from_fs("fsd.pulse");
    if loader::spawn_from_fs("shell.pulse").is_none() {
        serial.write_string("[kernel] shell.pulse nao carregou. Parando.\n");
        halt();
    }

    // Acorda os demais nucleos: cada um entra no escalonador por conta propria.
    let mut started = 0;
    for cpu in 0..arch::platform::MAX_CPUS {
        if cpu != boot_cpu && arch::start_cpu(cpu) {
            started += 1;
        }
    }
    let _ = write!(serial, "[smp] {} CPU(s) secundaria(s) acordada(s)\n", started);

    let ctx = unsafe { process::enter_scheduler(boot_cpu) };
    unsafe { arch::enter_frame(&ctx) }
}

/// Entrada das CPUs secundarias (MMU ja ligada pelo backend).
pub fn secondary_main(cpu: usize) -> ! {
    arch::secondary_init();
    let _ = write!(Uart, "[smp] CPU {} online\n", cpu);
    let ctx = unsafe { process::enter_scheduler(cpu) };
    unsafe { arch::enter_frame(&ctx) }
}

fn halt() -> ! {
    loop { arch::wait_for_interrupt(); }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    arch::irq_disable();
    let mut serial = uart::RawUart;
    let _ = write!(serial, "\n!!! KERNEL PANIC (cpu {}) !!!\n", arch::cpu_id());
    if let Some(loc) = info.location() {
        let _ = write!(serial, "em {}:{}\n", loc.file(), loc.line());
    }
    let _ = write!(serial, "{}\n", info.message());
    halt()
}
