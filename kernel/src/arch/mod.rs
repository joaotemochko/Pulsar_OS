//! Camada de arquitetura. Todo o codigo generico do kernel (escalonador,
//! IPC, syscalls, drivers virtio, sistemas de arquivos) fala SO com esta
//! interface; cada backend implementa o mesmo contrato:
//!
//!   Context                  frame de trap salvo (registradores + PC + modo)
//!     ::new_user(entry, sp)  contexto inicial de um processo EL0/U-mode
//!     ::new_idle(cpu)        contexto do laco ocioso (wfi) da CPU
//!     .syscall_num() .arg(i) .set_ret(v) .pc()
//!   mmu::{PageFlags, map_page_in, create_user_space, free_user_space,
//!         switch_space, translate_user, kernel_root, flush_tlb, flush_asid}
//!   platform::*              mapa de memoria e MMIO da maquina QEMU virt
//!   cpu_id() / max_cpus()    identificacao da CPU corrente
//!   now() / timer_freq() / set_timer_deadline(t)
//!   send_ipi(cpu) / start_cpu(cpu)
//!   enter_frame(ctx) -> !    restaura um Context e retorna ao modo dele
//!   console_putc(b), rtc_seconds(), poweroff()

#[cfg(target_arch = "aarch64")]
#[path = "aarch64/mod.rs"]
mod imp;

#[cfg(target_arch = "riscv64")]
#[path = "riscv64/mod.rs"]
mod imp;

#[cfg(target_arch = "x86_64")]
#[path = "x86_64/mod.rs"]
mod imp;


pub use imp::*;

/// Topo da pilha de kernel da CPU `cpu` (pilhas estaticas por CPU).
pub fn kstack_top(cpu: usize) -> u64 {
    let base = (&raw const KSTACKS) as u64;
    base + ((cpu as u64) + 1) * KSTACK_SIZE as u64
}

/// Tamanho da pilha de kernel de cada CPU.
pub const KSTACK_SIZE: usize = 128 * 1024;

#[repr(C, align(4096))]
pub struct KStacks(pub [[u8; KSTACK_SIZE]; platform::MAX_CPUS]);

/// Pilhas de kernel por CPU. O assembly de boot calcula o SP inicial a
/// partir deste simbolo; o frame de trap sempre fica no topo da pilha.
#[unsafe(no_mangle)]
pub static mut KSTACKS: KStacks = KStacks([[0; KSTACK_SIZE]; platform::MAX_CPUS]);
