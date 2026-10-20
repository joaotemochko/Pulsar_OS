//! Backend RISC-V RV64 (S-mode sob OpenSBI, Sv39) para a QEMU `virt`.
//!
//! O OpenSBI (M-mode) entra no kernel em 0x8020_0000 com a0 = hartid e
//! a1 = DTB. Timer, IPIs, partida de harts e desligamento usam chamadas
//! SBI. O MMIO fica numa janela alta (gigapagina global) para nunca
//! colidir com os enderecos fixos dos programas de usuario.

pub mod context;
pub mod mmu;
pub mod sbi;
pub mod trap;
pub mod uart;

pub use context::{Context, FpState, fp_save, fp_restore};

core::arch::global_asm!(include_str!("boot.S"));
core::arch::global_asm!(include_str!("trap.S"));

pub const NAME: &str = "RISC-V RV64";
pub const ARCH_ID: u32 = 2;

pub mod platform {
    pub const MAX_CPUS: usize = 8;
    /// RAM que o kernel gerencia (rode com -m 4G ou mais).
    pub const RAM_BASE: u64 = 0x8000_0000;
    pub const RAM_SIZE: u64 = 4 * 1024 * 1024 * 1024;
    pub const RAM_END: u64 = RAM_BASE + RAM_SIZE;
    /// O kernel e carregado aqui pelo OpenSBI (abaixo fica o firmware).
    pub const KERNEL_BASE: u64 = 0x8020_0000;
    /// Janela do frame allocator: toda a RAM depois do kernel (a regiao
    /// grafica, dentro dela, fica reservada).
    pub const HEAP_START: u64 = 0x8120_0000;   // 16 MiB para o kernel (imagem + .bss)
    pub const HEAP_SIZE: u64 = RAM_END - HEAP_START;
    pub const FB_PA: u64 = 0x8600_0000;
    /// Endereco da regiao grafica no espaco de usuario (igual ao AArch64,
    /// assim o mesmo binario de userspace funciona nas duas arquiteturas).
    pub const FB_UVA: u64 = 0x4600_0000;
    pub const FB_REGION_BYTES: u64 = 176 * 1024 * 1024;

    /// Base virtual da janela de MMIO (VA alto, sinal estendido do bit 38).
    pub const MMIO_VA: u64 = 0xFFFF_FFC0_0000_0000;
    pub const UART_PA: u64 = 0x1000_0000;  // NS16550A
    pub const RTC_PA: u64 = 0x0010_1000;   // goldfish-rtc
    pub const TEST_PA: u64 = 0x0010_0000;  // sifive,test (poweroff)
    pub const VIRTIO_MMIO_BASE: u64 = MMIO_VA + 0x1000_1000;
    pub const VIRTIO_MMIO_STRIDE: u64 = 0x1000;
    pub const VIRTIO_MMIO_SLOTS: u64 = 8;
    /// Frequencia do `time` na virt (timebase-frequency do DTB).
    pub const TIMEBASE_HZ: u64 = 10_000_000;
}

unsafe extern "C" {
    fn arch_enter_frame(ctx: *const Context, stack_top: u64) -> !;
    fn arch_idle_loop();
    fn _secondary_entry();
    fn trap_vector();
    static __kernel_end: u8;
}

/// Offset aplicado aos enderecos fisicos de MMIO: 0 antes da paginacao,
/// MMIO_VA depois.
static mut MMIO_OFFSET: u64 = 0;

#[inline]
pub fn mmio(pa: u64) -> u64 {
    unsafe { pa + MMIO_OFFSET }
}

pub fn kernel_end() -> u64 {
    (&raw const __kernel_end) as u64
}

pub fn idle_loop_addr() -> u64 {
    arch_idle_loop as *const () as u64
}

/// A CPU e deduzida do SP: cada hart roda sempre na sua pilha de kernel.
/// (O registrador tp pertence ao usuario e nao e confiavel no kernel.)
#[inline]
pub fn cpu_id() -> usize {
    let sp: u64;
    unsafe { core::arch::asm!("mv {}, sp", out(reg) sp, options(nomem, nostack, preserves_flags)) };
    let base = (&raw const super::KSTACKS) as u64;
    ((sp - base - 1) / super::KSTACK_SIZE as u64) as usize
}

#[inline]
pub fn console_putc(b: u8) {
    uart::putc(b);
}

const SSTATUS_SUM: u64 = 1 << 18;
const SIE_SSIE: u64 = 1 << 1;
const SIE_STIE: u64 = 1 << 5;

fn csr_setup_hart() {
    unsafe {
        core::arch::asm!(
            "csrw stvec, {vec}",
            "csrw sscratch, zero",
            "csrs sstatus, {sum}",
            "csrs sie, {ie}",
            vec = in(reg) trap_vector as *const () as usize,
            sum = in(reg) SSTATUS_SUM,
            ie = in(reg) SIE_SSIE | SIE_STIE,
            options(nostack));
    }
}

pub fn early_init() {
    csr_setup_hart();
}

pub fn mmu_init() {
    unsafe {
        mmu::init();
        MMIO_OFFSET = platform::MMIO_VA;
    }
}

/// Sem controlador externo: so timer (STIP) e IPI (SSIP), ja habilitados.
pub fn irq_init_boot() {}

pub fn secondary_init() {
    csr_setup_hart();
}

#[inline]
pub fn now() -> u64 {
    let t: u64;
    unsafe { core::arch::asm!("rdtime {}", out(reg) t, options(nomem, nostack, preserves_flags)) };
    t
}

#[inline]
pub fn timer_freq() -> u64 {
    platform::TIMEBASE_HZ
}

#[inline]
pub fn set_timer_deadline(t: u64) {
    sbi::set_timer(t);
}

pub fn send_ipi(cpu: usize) {
    sbi::send_ipi(1u64 << cpu, 0);
}

/// Parte um hart via SBI HSM; ele entra em _secondary_entry(a0=hartid).
pub fn start_cpu(cpu: usize) -> bool {
    sbi::hart_start(cpu as u64, _secondary_entry as *const () as u64, 0) == 0
}

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

/// goldfish-rtc: nanossegundos desde a epoca (TIME_LOW deve ser lido 1o).
pub fn rtc_seconds() -> u64 {
    unsafe {
        let base = mmio(platform::RTC_PA);
        let lo = core::ptr::read_volatile(base as *const u32) as u64;
        let hi = core::ptr::read_volatile((base + 4) as *const u32) as u64;
        ((hi << 32) | lo) / 1_000_000_000
    }
}

pub fn poweroff() -> ! {
    sbi::system_reset(0, 0);
    unsafe { core::ptr::write_volatile(mmio(platform::TEST_PA) as *mut u32, 0x5555) };
    loop { wait_for_interrupt(); }
}

#[inline]
pub fn wait_for_interrupt() {
    unsafe { core::arch::asm!("wfi", options(nomem, nostack, preserves_flags)) };
}

#[inline]
pub fn mb() {
    unsafe { core::arch::asm!("fence iorw, iorw", options(nostack, preserves_flags)) };
}

pub fn irq_disable() {
    unsafe { core::arch::asm!("csrci sstatus, 2", options(nomem, nostack)) };
}

/// Entrada Rust da hart de boot (chamada por boot.S).
#[unsafe(no_mangle)]
pub extern "C" fn arch_boot_main(hartid: u64, _dtb: u64) -> ! {
    crate::kernel_main(hartid as usize)
}

#[unsafe(no_mangle)]
pub extern "C" fn arch_secondary_main(hartid: u64) -> ! {
    unsafe { mmu::enable_on_this_hart() };
    crate::secondary_main(hartid as usize)
}
