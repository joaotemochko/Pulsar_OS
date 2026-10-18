//! Backend AArch64 (ARMv8-A, EL1/EL0) para a maquina QEMU `virt`.

pub mod context;
pub mod exceptions;
pub mod gic;
pub mod irq;
pub mod mmu;
pub mod timer;
pub mod uart;

pub use context::{Context, FpState, fp_save, fp_restore};

core::arch::global_asm!(include_str!("boot.S"));
core::arch::global_asm!(include_str!("vectors.S"));

pub const NAME: &str = "AArch64";
/// Identificador gravado no cabecalho .pulse (campo `arch`).
pub const ARCH_ID: u32 = 1;

/// Mapa de memoria da `virt`: RAM em 0x4000_0000, 4 GiB (rode com -m 4G).
pub mod platform {
    pub const MAX_CPUS: usize = 8;
    /// RAM que o kernel gerencia (QEMU precisa de pelo menos RAM_SIZE).
    pub const RAM_BASE: u64 = 0x4000_0000;
    pub const RAM_SIZE: u64 = 4 * 1024 * 1024 * 1024;
    pub const RAM_END: u64 = RAM_BASE + RAM_SIZE;
    /// Kernel + pilhas ocupam [0x4000_0000, HEAP_START).
    pub const KERNEL_BASE: u64 = 0x4000_0000;
    /// Janela do frame allocator: toda a RAM depois do kernel (a regiao
    /// grafica, dentro dela, fica reservada).
    pub const HEAP_START: u64 = 0x4100_0000;   // 16 MiB para o kernel (imagem + .bss)
    pub const HEAP_SIZE: u64 = RAM_END - HEAP_START;
    /// Regiao grafica (back buffer, wallpaper, superficies): fisico...
    pub const FB_PA: u64 = 0x4600_0000;
    /// ...e o endereco em que o userspace a enxerga (aqui VA == PA).
    pub const FB_UVA: u64 = 0x4600_0000;
    pub const FB_REGION_BYTES: u64 = 64 * 1024 * 1024;

    // MMIO (identity-mapped)
    pub const UART_BASE: u64 = 0x0900_0000;
    pub const RTC_BASE: u64 = 0x0901_0000; // PL031
    pub const GICD_BASE: u64 = 0x0800_0000;
    pub const GICC_BASE: u64 = 0x0801_0000;
    pub const VIRTIO_MMIO_BASE: u64 = 0x0a00_0000;
    pub const VIRTIO_MMIO_STRIDE: u64 = 0x200;
    pub const VIRTIO_MMIO_SLOTS: u64 = 32;
}

unsafe extern "C" {
    fn arch_enter_frame(ctx: *const Context, stack_top: u64) -> !;
    fn arch_idle_loop();
    fn _secondary_entry();
    static __kernel_end: u8;
}

/// Fim da imagem do kernel (inclui .bss e pilhas).
pub fn kernel_end() -> u64 {
    (&raw const __kernel_end) as u64
}

pub fn idle_loop_addr() -> u64 {
    arch_idle_loop as *const () as u64
}

/// CPU corrente (TPIDR_EL1 e gravado no boot de cada nucleo).
#[inline]
pub fn cpu_id() -> usize {
    let v: u64;
    unsafe { core::arch::asm!("mrs {}, tpidr_el1", out(reg) v, options(nomem, nostack, preserves_flags)) };
    v as usize
}

#[inline]
pub fn console_putc(b: u8) {
    uart::putc(b);
}

/// Inicializacao da CPU de boot (antes da MMU): vetor de excecoes.
pub fn early_init() {
    unsafe { core::arch::asm!("msr tpidr_el1, xzr", options(nomem, nostack, preserves_flags)) };
    exceptions::install_vectors();
    context::fp_enable();
}

/// Liga a MMU na CPU de boot com o mapa identidade do kernel.
pub fn mmu_init() {
    unsafe { mmu::init() };
}

/// Controlador de interrupcoes: distribuidor (uma vez) + interface da CPU.
pub fn irq_init_boot() {
    gic::init_distributor();
    gic::init_cpu();
}

/// Inicializacao por CPU secundaria, ja com MMU ligada (feito no asm).
pub fn secondary_init() {
    exceptions::install_vectors();
    context::fp_enable();
    gic::init_cpu();
}

#[inline]
pub fn now() -> u64 {
    timer::now()
}

#[inline]
pub fn timer_freq() -> u64 {
    timer::freq()
}

/// Programa o proximo IRQ de timer desta CPU para o instante absoluto `t`.
#[inline]
pub fn set_timer_deadline(t: u64) {
    timer::set_deadline(t)
}

/// Interrupcao entre processadores (SGI 1 do GICv2).
pub fn send_ipi(cpu: usize) {
    gic::send_sgi(cpu, gic::IPI_SGI);
}

/// Liga uma CPU secundaria via PSCI CPU_ON (conduite HVC na `virt`).
pub fn start_cpu(cpu: usize) -> bool {
    let entry = _secondary_entry as *const () as u64;
    let ret = psci_call(0xC400_0003, cpu as u64, entry, cpu as u64);
    ret == 0
}

fn psci_call(func: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    let ret: u64;
    unsafe {
        core::arch::asm!("hvc #0",
            inout("x0") func => ret, in("x1") a1, in("x2") a2, in("x3") a3,
            options(nostack));
    }
    ret as i64
}

/// Retorna para o contexto `ctx` (EL0 ou laco ocioso) na pilha desta CPU.
pub unsafe fn enter_frame(ctx: &Context) -> ! {
    // O Context pode estar na propria pilha de destino: copia-o antes
    // para um buffer estatico da CPU e deixa o assembly montar o frame.
    static mut STAGING: [Context; platform::MAX_CPUS] = [Context::zeroed(); platform::MAX_CPUS];
    let cpu = cpu_id();
    unsafe {
        let slot = &raw mut STAGING[cpu];
        *slot = *ctx;
        arch_enter_frame(slot, super::kstack_top(cpu))
    }
}

/// Segundos desde a epoca Unix (RTC PL031).
pub fn rtc_seconds() -> u64 {
    unsafe { core::ptr::read_volatile(platform::RTC_BASE as *const u32) as u64 }
}

/// Desliga a maquina (PSCI SYSTEM_OFF).
pub fn poweroff() -> ! {
    psci_call(0x8400_0008, 0, 0, 0);
    loop { wait_for_interrupt(); }
}

#[inline]
pub fn wait_for_interrupt() {
    unsafe { core::arch::asm!("wfi", options(nomem, nostack, preserves_flags)) };
}

/// Barreira completa (memoria normal + dispositivos).
#[inline]
pub fn mb() {
    unsafe { core::arch::asm!("dsb sy", options(nostack, preserves_flags)) };
}

/// Mascara IRQs na CPU corrente (usado no panic).
pub fn irq_disable() {
    unsafe { core::arch::asm!("msr daifset, #2", options(nomem, nostack, preserves_flags)) };
}

/// Entrada Rust da CPU de boot (chamada por boot.S).
#[unsafe(no_mangle)]
pub extern "C" fn arch_boot_main() -> ! {
    crate::kernel_main(0)
}

/// Entrada Rust das CPUs secundarias (MMU ja ligada pelo boot.S).
#[unsafe(no_mangle)]
pub extern "C" fn arch_secondary_main(cpu: u64) -> ! {
    crate::secondary_main(cpu as usize)
}
