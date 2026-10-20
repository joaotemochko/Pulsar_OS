//! Backend x86_64 (Intel 64 / AMD64) para a máquina QEMU `microvm`.

pub mod apic;
pub mod context;
pub mod mmu;
pub mod timer;
pub mod trap;
pub mod uart;

pub use context::{Context, FpState, fp_save, fp_restore};

core::arch::global_asm!(include_str!("boot.S"));
core::arch::global_asm!(include_str!("trap.S"));

pub const NAME: &str = "x86_64";
pub const ARCH_ID: u32 = 3;

pub mod platform {
    pub const MAX_CPUS: usize = 8;
    pub const RAM_BASE: u64 = 0x0000_0000;
    pub const RAM_SIZE: u64 = 3 * 1024 * 1024 * 1024; // 3 GiB gerenciados pelo kernel
    pub const RAM_END: u64 = RAM_BASE + RAM_SIZE;
    pub const KERNEL_BASE: u64 = 0x0010_0000;         // 1 MiB (carregado pelo Multiboot)
    pub const HEAP_START: u64 = 0x5400_0000;          // 1344 MiB (acima da região gráfica)
    pub const HEAP_SIZE: u64 = RAM_END - HEAP_START;

    // Região gráfica compartilhada (PA == VA no espaço do usuário)
    pub const FB_PA: u64 = 0x4600_0000;
    pub const FB_UVA: u64 = 0x4600_0000;
    pub const FB_REGION_BYTES: u64 = 176 * 1024 * 1024;

    // Slots Virtio-MMIO da máquina microvm do QEMU
    pub const VIRTIO_MMIO_BASE: u64 = 0xfeb0_0000;
    pub const VIRTIO_MMIO_STRIDE: u64 = 0x200;
    pub const VIRTIO_MMIO_SLOTS: u64 = 24;
}

unsafe extern "C" {
    fn arch_enter_frame(ctx: *const Context, stack_top: u64) -> !;
    fn arch_idle_loop();
    static __kernel_end: u8;
    static boot_mb_info: u64;
}

pub fn kernel_end() -> u64 {
    (&raw const __kernel_end) as u64
}

pub fn idle_loop_addr() -> u64 {
    arch_idle_loop as *const () as u64
}

#[inline]
pub fn cpu_id() -> usize {
    let sp: u64;
    unsafe { core::arch::asm!("mov {}, rsp", out(reg) sp, options(nomem, nostack, preserves_flags)); }
    let base = (&raw const super::KSTACKS) as u64;

    if sp >= base && sp < base + (super::KSTACK_SIZE * platform::MAX_CPUS) as u64 {
        ((sp - base) / super::KSTACK_SIZE as u64) as usize
    } else {
        0
    }
}

#[inline]
pub fn console_putc(b: u8) {
    uart::putc(b);
}

pub fn early_init() {
    trap::init_gdt_tss(0);
    trap::init_idt();
    trap::init_syscall(0);
    trap::init_pic();

    let mb = unsafe { boot_mb_info };
    let mut u = crate::uart::Uart;
    use core::fmt::Write;
    let _ = write!(u, "Multiboot info: 0x{:x}\n", mb);
    if mb != 0 && mb < 0x1_0000_0000 {
        let flags = unsafe { *(mb as *const u32) };
        let _ = write!(u, "MB flags: 0x{:x}\n", flags);
        if flags & (1 << 2) != 0 {
            let cmdline = unsafe { *((mb + 16) as *const u32) } as u64;
            let _ = write!(u, "Cmdline at 0x{:x}: ", cmdline);
            if cmdline != 0 && cmdline < 0x1_0000_0000 {
                let mut p = cmdline as *const u8;
                while unsafe { *p } != 0 {
                    console_putc(unsafe { *p });
                    p = unsafe { p.add(1) };
                }
                console_putc(b'\n');
            }
        }
    }
}

pub fn mmu_init() {
    unsafe { mmu::init(); }
}

pub fn irq_init_boot() {
    apic::copy_trampoline();
    apic::init_lapic();
    apic::calibrate_timer();
    trap::mask_pic();
    apic::init_lapic_timer();
}

pub fn secondary_init() {
    let cpu = cpu_id();
    trap::init_gdt_tss(cpu);
    trap::init_syscall(cpu);
    trap::init_idt_secondary();
    apic::init_lapic();
    apic::init_lapic_timer();
}

#[inline]
pub fn now() -> u64 {
    timer::now()
}

#[inline]
pub fn timer_freq() -> u64 {
    timer::freq()
}

#[inline]
pub fn set_timer_deadline(t: u64) {
    timer::set_deadline(t);
}

pub fn send_ipi(cpu: usize) {
    apic::send_ipi(cpu as u32, 48);
}

pub fn start_cpu(cpu: usize) -> bool {
    apic::start_ap(cpu as u32)
}

pub unsafe fn enter_frame(ctx: &Context) -> ! {
    static mut STAGING: [Context; platform::MAX_CPUS] = [Context::zeroed(); platform::MAX_CPUS];
    let cpu = cpu_id();
    unsafe {
        let slot = &raw mut STAGING[cpu];
        *slot = *ctx;
        arch_enter_frame(slot, super::kstack_top(cpu))
    }
}

pub fn rtc_seconds() -> u64 {
    unsafe {
        let bcd = |reg: u8| -> u64 {
            uart::outb(0x70, reg);
            let val = uart::inb(0x71);
            (((val >> 4) * 10) + (val & 0xF)) as u64
        };
        let sec = bcd(0x00);
        let min = bcd(0x02);
        let hour = bcd(0x04);
        let day = bcd(0x07);
        let month = bcd(0x08);
        let year_val = bcd(0x09);
        let century = {
            let c = bcd(0x32);
            if (19..=21).contains(&c) { c } else { 20 }
        };
        let year = century * 100 + year_val;

        let days_before_year = |y: u64| -> u64 {
            let y1 = y - 1;
            y1 * 365 + y1 / 4 - y1 / 100 + y1 / 400
        };
        let leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
        let mut m_days = [0u64, 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
        if leap { m_days[2] = 29; }
        let mut sum_m = 0;
        let m_idx = (month as usize).min(12);
        for i in 1..m_idx {
            sum_m += m_days[i];
        }
        let tot_days = days_before_year(year).saturating_sub(days_before_year(1970))
            + sum_m + (day.saturating_sub(1));
        tot_days * 86400 + hour * 3600 + min * 60 + sec
    }
}

pub fn poweroff() -> ! {
    unsafe {
        core::arch::asm!("out dx, ax", in("dx") 0x604u16, in("ax") 0x2000u16, options(nomem, nostack));
    }
    loop { wait_for_interrupt(); }
}


#[inline]
pub fn wait_for_interrupt() {
    unsafe { core::arch::asm!("hlt", options(nomem, nostack, preserves_flags)); }
}

#[inline]
pub fn mb() {
    unsafe { core::arch::asm!("mfence", options(nostack, preserves_flags)); }
}

pub fn irq_disable() {
    unsafe { core::arch::asm!("cli", options(nomem, nostack)); }
}

#[unsafe(no_mangle)]
pub extern "C" fn arch_boot_main() -> ! {
    crate::kernel_main(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn arch_secondary_main(cpu: u64) -> ! {
    crate::secondary_main(cpu as usize)
}
