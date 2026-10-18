#![no_std]
#![no_main]

mod arch;
mod blk;
mod exec;
mod ext2;
mod fb;
mod file;
mod frame_allocator;
mod fs;
mod gpu;
mod input;
mod linux;
mod loader;
mod nbfs;
mod net;
mod pfs;
mod process;
mod pulse;
mod rng;
mod shm;
mod signal;
mod sync;
mod syscall;
mod time;
mod trap;
mod uart;
mod unix;
mod virtio;
mod vm;

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

    // O init supervisiona os servicos (fsd, WM...) e os reinicia se cairem.
    // Sem ele (discos antigos), o kernel inicia fsd e o shell diretamente.
    if loader::spawn_from_fs("init.pulse").is_some() {
        serial.write_string("init iniciado (servicos em /etc/services)\n");
    } else {
        serial.write_string("Iniciando fsd.pulse e shell.pulse...\n");
        loader::spawn_from_fs("fsd.pulse");
        if loader::spawn_from_fs("shell.pulse").is_none() {
            serial.write_string("[kernel] shell.pulse nao carregou. Parando.\n");
            halt();
        }
    }

    autorun();

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

/// /etc/autorun (opcional, para testes automatizados): cada linha e um
/// programa com argumentos, executado no boot com stdout/stderr no console
/// serial. Linhas vazias e comecando com '#' sao ignoradas.
fn autorun() {
    let Ok((buf, frames, size)) = file::read_whole(ext2::ROOT_INO, b"/etc/autorun", 16 * 1024) else { return };
    let text = unsafe { core::slice::from_raw_parts(buf as *const u8, size as usize) };
    for line in text.split(|&c| c == b'\n') {
        let line = line.trim_ascii();
        if line.is_empty() || line[0] == b'#' { continue; }
        // argv: palavras separadas por espaco, terminadas em NUL
        let mut argv = [0u8; 512];
        let (mut n, mut argc) = (0, 0);
        for w in line.split(|&c| c == b' ').filter(|w| !w.is_empty()) {
            if n + w.len() + 1 > argv.len() { break; }
            argv[n..n + w.len()].copy_from_slice(w);
            n += w.len() + 1;
            argc += 1;
        }
        let prog = line.split(|&c| c == b' ').next().unwrap_or(b"");
        let args = exec::Args { argv: &argv[..n], argc, envp: b"HOME=/home/user\0PATH=/bin\0XDG_RUNTIME_DIR=/run\0WAYLAND_DISPLAY=wayland-0\0XCOMPOSEFILE=/usr/share/pulsar/Compose\0LANG=C.UTF-8\0", envc: 6 };
        let _ = exec::spawn(ext2::ROOT_INO, prog, &args);
    }
    frame_allocator::free_contig(buf, frames);
}

fn halt() -> ! {
    loop { arch::wait_for_interrupt(); }
}

/// Falhas do kernel aninhadas por CPU (uma falha durante a recuperacao
/// para a maquina).
static OOPS: [core::sync::atomic::AtomicU32; arch::platform::MAX_CPUS] =
    [const { core::sync::atomic::AtomicU32::new(0) }; arch::platform::MAX_CPUS];

/// Uma falha do kernel (panic, ou excecao em modo kernel, que vira panic)
/// nao derruba o sistema: e um "oops". A CPU solta as travas que segurava,
/// o processo que estava sendo atendido morre (status de SIGSEGV) e a CPU
/// volta ao escalonador numa pilha limpa. Os servicos (fsd, WM...) sao
/// processos supervisionados pelo init, que os reinicia se cairem. So uma
/// falha antes do escalonador existir, ou durante a propria recuperacao,
/// para a maquina.
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    use core::sync::atomic::Ordering;
    arch::irq_disable();
    let c = arch::cpu_id();
    let mut serial = uart::RawUart;
    let _ = write!(serial, "\n!!! OOPS DO KERNEL (cpu {}, processo {} '{}') !!!\n",
        c, process::current_pid() as isize, process::current_name());
    if let Some(loc) = info.location() {
        let _ = write!(serial, "em {}:{}\n", loc.file(), loc.line());
    }
    let _ = write!(serial, "{}\n", info.message());
    if OOPS[c].fetch_add(1, Ordering::SeqCst) > 0 || !process::cpu_online() {
        let _ = write!(serial, "[oops] falha sem recuperacao possivel (boot ou recuperacao em curso). CPU parada.\n");
        halt();
    }
    let held = sync::force_release_all();
    let (ctx, killed) = unsafe { process::oops_recover() };
    match killed {
        Some(pid) => { let _ = write!(serial, "[oops] {} trava(s) soltas; processo {} encerrado; o sistema segue.\n", held, pid); }
        None => { let _ = write!(serial, "[oops] {} trava(s) soltas; o sistema segue.\n", held); }
    }
    OOPS[c].store(0, Ordering::SeqCst);
    unsafe { arch::enter_frame(&ctx) }
}
