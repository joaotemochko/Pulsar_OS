//! MMU AArch64: tabelas de 4 niveis (granulo 4 KiB, VA de 48 bits) em
//! TTBR0, com ASIDs de 8 bits. O kernel e mapeado em identidade e
//! compartilhado por todos os espacos de usuario.
use core::arch::asm;
use aarch64_cpu::registers::*;
use tock_registers::interfaces::{Readable, Writeable};
use crate::frame_allocator;
use super::platform;

#[repr(C, align(4096))]
struct Table { e: [u64; 512] }

// Raiz (L0) do kernel. Os demais niveis vem do frame allocator.
static mut L0: Table = Table { e: [0; 512] };

const TABLE: u64 = 0b11;   // descritor de tabela (aponta p/ proximo nivel)
const PAGE:  u64 = 0b11;   // descritor de pagina (L3)
const VALID_MASK: u64 = 0b11;
const ADDR_MASK: u64 = 0x0000_FFFF_FFFF_F000;

/// Registradores de MMU publicados para as CPUs secundarias (boot.S).
#[unsafe(no_mangle)] pub static mut MMU_MAIR: u64 = 0;
#[unsafe(no_mangle)] pub static mut MMU_TCR: u64 = 0;
#[unsafe(no_mangle)] pub static mut MMU_TTBR0: u64 = 0;
#[unsafe(no_mangle)] pub static mut MMU_SCTLR: u64 = 0;

#[derive(Clone, Copy)]
pub struct PageFlags {
    pub bits: u64,
}

impl PageFlags {
    const AF:    u64 = 1 << 10;
    const SH_IS: u64 = 0b11 << 8;
    const IDX_DEV: u64 = 0 << 2;
    const IDX_NRM: u64 = 1 << 2;
    const AP_KERNEL: u64 = 0b00 << 6;  // RW so EL1
    const AP_USER:   u64 = 0b01 << 6;  // RW EL0+EL1
    const AP_USER_RO: u64 = 0b11 << 6; // RO EL0+EL1
    const PXN: u64 = 1 << 53;
    const UXN: u64 = 1 << 54;
    /// not-Global: traducao presa ao ASID corrente.
    const NG: u64 = 1 << 11;

    /// Codigo/dados do kernel: Normal, RW so EL1, executavel em EL1.
    pub fn kernel_rwx() -> Self {
        Self { bits: Self::IDX_NRM | Self::AP_KERNEL | Self::AF | Self::SH_IS }
    }
    /// Periferico: Device, RW so EL1, nunca executavel.
    pub fn device() -> Self {
        Self { bits: Self::IDX_DEV | Self::AP_KERNEL | Self::AF | Self::PXN | Self::UXN }
    }
    /// Dados do kernel (framebuffer): Normal, RW so EL1, nao-executavel.
    pub fn kernel_data() -> Self {
        Self { bits: Self::IDX_NRM | Self::AP_KERNEL | Self::AF | Self::SH_IS | Self::PXN | Self::UXN }
    }
    /// Codigo de usuario: RO + executavel em EL0 (W^X).
    pub fn user_code() -> Self {
        Self { bits: Self::IDX_NRM | Self::AP_USER_RO | Self::AF | Self::SH_IS | Self::PXN | Self::NG }
    }
    /// Dados/pilha de usuario: RW, nunca executavel.
    pub fn user_data() -> Self {
        Self { bits: Self::IDX_NRM | Self::AP_USER | Self::AF | Self::SH_IS | Self::PXN | Self::UXN | Self::NG }
    }
    /// Regiao grafica compartilhada com o userspace (global: vive na
    /// subarvore do kernel, igual para todos os processos).
    pub fn user_shared() -> Self {
        Self { bits: Self::IDX_NRM | Self::AP_USER | Self::AF | Self::SH_IS | Self::PXN | Self::UXN }
    }
}

#[inline]
fn indices(va: u64) -> (usize, usize, usize, usize) {
    (
        ((va >> 39) & 0x1FF) as usize,
        ((va >> 30) & 0x1FF) as usize,
        ((va >> 21) & 0x1FF) as usize,
        ((va >> 12) & 0x1FF) as usize,
    )
}

unsafe fn next_table(table_pa: u64, idx: usize) -> Option<u64> {
    unsafe {
        let entry_ptr = (table_pa as *mut u64).add(idx);
        let entry = entry_ptr.read_volatile();
        if entry & VALID_MASK == TABLE {
            Some(entry & ADDR_MASK)
        } else {
            let new_pa = frame_allocator::alloc_frame()?;
            core::ptr::write_bytes(new_pa as *mut u8, 0, 4096);
            entry_ptr.write_volatile(new_pa | TABLE);
            Some(new_pa)
        }
    }
}

pub fn kernel_root() -> u64 {
    (&raw const L0) as u64
}

/// Mapeia uma pagina de 4KB no espaco cuja raiz e `root_pa`.
pub unsafe fn map_page_in(root_pa: u64, va: u64, pa: u64, flags: PageFlags) -> bool {
    let _g = crate::sync::MMU.guard();
    let (i0, i1, i2, i3) = indices(va);
    unsafe {
        let Some(l1) = next_table(root_pa, i0) else { return false };
        let Some(l2) = next_table(l1, i1) else { return false };
        let Some(l3) = next_table(l2, i2) else { return false };
        (l3 as *mut u64).add(i3).write_volatile((pa & ADDR_MASK) | PAGE | flags.bits);
    }
    true
}

const BLOCK: u64 = 0b01;   // descritor de bloco (L2: 2 MiB)
const SZ_2M: u64 = 2 * 1024 * 1024;

/// Mapeia um bloco de 2 MiB (identidade) no espaco do kernel.
unsafe fn map_block_2m(va: u64, flags: PageFlags) -> bool {
    let (i0, i1, i2, _) = indices(va);
    unsafe {
        let Some(l1) = next_table(kernel_root(), i0) else { return false };
        let Some(l2) = next_table(l1, i1) else { return false };
        (l2 as *mut u64).add(i2).write_volatile((va & ADDR_MASK) | BLOCK | flags.bits);
    }
    true
}

unsafe fn map_range_identity(start: u64, size: u64, flags: PageFlags) {
    let mut off = 0u64;
    while off < size {
        unsafe { map_page_in(kernel_root(), start + off, start + off, flags); }
        off += 4096;
    }
}

/// Monta o mapa do kernel e liga a MMU (CPU de boot).
pub unsafe fn init() {
    unsafe {
        map_page_in(kernel_root(), platform::UART_BASE, platform::UART_BASE, PageFlags::device());
        map_page_in(kernel_root(), platform::RTC_BASE, platform::RTC_BASE, PageFlags::device());
        map_page_in(kernel_root(), platform::GICD_BASE, platform::GICD_BASE, PageFlags::device());
        map_page_in(kernel_root(), platform::GICC_BASE, platform::GICC_BASE, PageFlags::device());
        map_range_identity(platform::VIRTIO_MMIO_BASE,
            platform::VIRTIO_MMIO_SLOTS * platform::VIRTIO_MMIO_STRIDE, PageFlags::device());
        // regiao grafica: Normal, so EL1 por enquanto (FB_MAP libera p/ EL0)
        map_range_identity(platform::FB_PA, platform::FB_REGION_BYTES, PageFlags::kernel_data());
        // RAM: blocos de 2 MiB (4 GiB em paginas de 4 KiB custariam 8 MiB de
        // tabelas). O comeco (kernel) e executavel; o resto (frames: tabelas,
        // dados, codigo de apps visto pelo kernel) nunca e executado em EL1.
        let mut a = platform::RAM_BASE;
        while a < platform::RAM_END {
            let in_fb = a >= platform::FB_PA && a < platform::FB_PA + platform::FB_REGION_BYTES;
            if !in_fb {
                let flags = if a < platform::HEAP_START { PageFlags::kernel_rwx() } else { PageFlags::kernel_data() };
                map_block_2m(a, flags);
            }
            a += SZ_2M;
        }
    }

    MAIR_EL1.write(
        MAIR_EL1::Attr0_Device::nonGathering_nonReordering_noEarlyWriteAck
        + MAIR_EL1::Attr1_Normal_Inner::WriteBack_NonTransient_ReadWriteAlloc
        + MAIR_EL1::Attr1_Normal_Outer::WriteBack_NonTransient_ReadWriteAlloc,
    );
    TCR_EL1.write(
        TCR_EL1::T0SZ.val(16)
        + TCR_EL1::IRGN0::WriteBack_ReadAlloc_WriteAlloc_Cacheable
        + TCR_EL1::ORGN0::WriteBack_ReadAlloc_WriteAlloc_Cacheable
        + TCR_EL1::SH0::Inner
        + TCR_EL1::TG0::KiB_4
        + TCR_EL1::EPD1::DisableTTBR1Walks
        + TCR_EL1::IPS::Bits_40,
    );
    TTBR0_EL1.set(kernel_root());

    unsafe { asm!("dsb ish", "tlbi vmalle1", "dsb ish", "isb", options(nostack, preserves_flags)); }

    let mut sctlr = SCTLR_EL1.get();
    sctlr &= !(1u64 << 19);                 // WXN off (W^X e feito por pagina)
    sctlr |= (1 << 0) | (1 << 2) | (1 << 12); // M, C, I
    unsafe {
        MMU_MAIR = MAIR_EL1.get();
        MMU_TCR = TCR_EL1.get();
        MMU_TTBR0 = kernel_root();
        MMU_SCTLR = sctlr;
    }
    SCTLR_EL1.set(sctlr);

    unsafe { asm!("isb", "ic iallu", "tlbi vmalle1", "dsb nsh", "isb", options(nostack, preserves_flags)); }
}

pub fn is_enabled() -> bool {
    SCTLR_EL1.is_set(SCTLR_EL1::M)
}

/// Invalida o TLB em todos os nucleos (inner shareable).
pub fn flush_tlb() {
    unsafe { asm!("dsb ish", "tlbi vmalle1is", "dsb ish", "isb", options(nostack, preserves_flags)); }
}

unsafe fn table_entry(table_pa: u64, idx: usize) -> u64 {
    unsafe { (table_pa as *const u64).add(idx).read_volatile() }
}

/// Cria um espaco de enderecos de usuario novo:
///
///   L0 (privada) ─[0]→ L1 (privada)
///                        ├─[0] L2 do GB0 (privada): entradas de MMIO do
///                        │      kernel copiadas (L3 compartilhadas);
///                        │      regiao de usuario comeca vazia
///                        └─[1] subarvore do GB1 do kernel (COMPARTILHADA:
///                               kernel, heap, regiao grafica)
pub fn create_user_space() -> Option<u64> {
    let _g = crate::sync::MMU.guard();
    let l0 = frame_allocator::alloc_frame()?;
    let l1 = frame_allocator::alloc_frame()?;
    let l2_gb0 = frame_allocator::alloc_frame()?;
    unsafe {
        core::ptr::write_bytes(l0 as *mut u8, 0, 4096);
        core::ptr::write_bytes(l1 as *mut u8, 0, 4096);
        core::ptr::write_bytes(l2_gb0 as *mut u8, 0, 4096);

        let k_l1 = table_entry(kernel_root(), 0) & ADDR_MASK;
        let k_l2_gb0 = table_entry(k_l1, 0) & ADDR_MASK;

        // GB1..: RAM do kernel e regiao grafica (subarvores compartilhadas)
        for i1 in 1..512 {
            let e = table_entry(k_l1, i1);
            if e & VALID_MASK != 0 {
                (l1 as *mut u64).add(i1).write_volatile(e);
            }
        }
        (l1 as *mut u64).add(0).write_volatile(l2_gb0 | TABLE);
        for idx in 0..512 {
            let e = table_entry(k_l2_gb0, idx);
            if e & VALID_MASK != 0 {
                (l2_gb0 as *mut u64).add(idx).write_volatile(e);
            }
        }
        (l0 as *mut u64).add(0).write_volatile(l1 | TABLE);
    }
    Some(l0)
}

/// Libera tabelas e paginas privadas de um espaco de usuario. Entradas
/// compartilhadas com o kernel (MMIO, GB1) sao reconhecidas por serem
/// identicas as do kernel e preservadas. O chamador ja trocou de espaco.
pub fn free_user_space(l0: u64) {
    let _g = crate::sync::MMU.guard();
    unsafe {
        let k_l1 = table_entry(kernel_root(), 0) & ADDR_MASK;
        let k_l2_gb0 = table_entry(k_l1, 0) & ADDR_MASK;
        for i0 in 0..512 {
            let e0 = table_entry(l0, i0);
            if e0 & VALID_MASK != TABLE { continue; }
            let l1 = e0 & ADDR_MASK;
            for i1 in 0..512 {
                let e1 = table_entry(l1, i1);
                if e1 & VALID_MASK != TABLE { continue; }
                if i0 == 0 && e1 == table_entry(k_l1, i1) { continue; } // GB1 do kernel
                let l2 = e1 & ADDR_MASK;
                for i2 in 0..512 {
                    let e2 = table_entry(l2, i2);
                    if e2 & VALID_MASK != TABLE { continue; }
                    if i0 == 0 && i1 == 0 && e2 == table_entry(k_l2_gb0, i2) { continue; } // MMIO
                    let l3 = e2 & ADDR_MASK;
                    for i3 in 0..512 {
                        let e3 = table_entry(l3, i3);
                        if e3 & VALID_MASK == PAGE {
                            frame_allocator::free_frame(e3 & ADDR_MASK);
                        }
                    }
                    frame_allocator::free_frame(l3);
                }
                frame_allocator::free_frame(l2);
            }
            frame_allocator::free_frame(l1);
        }
        frame_allocator::free_frame(l0);
    }
}

/// Troca o espaco de enderecos ativo: TTBR0 = raiz | ASID.
pub fn switch_space(l0_pa: u64, asid: u16) {
    unsafe {
        asm!("msr ttbr0_el1, {v}", "isb",
            v = in(reg) (l0_pa | ((asid as u64) << 48)),
            options(nostack, preserves_flags));
    }
}

/// Invalida as entradas de TLB de um ASID em todos os nucleos.
pub fn flush_asid(asid: u16) {
    unsafe {
        asm!("dsb ish", "tlbi aside1is, {v}", "dsb ish", "isb",
            v = in(reg) ((asid as u64) << 48),
            options(nostack, preserves_flags));
    }
}

/// Traduz `va` de USUARIO para PA; None se ausente, se nao acessivel em
/// EL0 ou (com `write`) se somente-leitura. Usado para validar ponteiros
/// recebidos em syscalls.
pub fn translate_user(root_pa: u64, va: u64, write: bool) -> Option<u64> {
    let (i0, i1, i2, i3) = indices(va);
    unsafe {
        let e0 = table_entry(root_pa, i0);
        if e0 & VALID_MASK != TABLE { return None; }
        let e1 = table_entry(e0 & ADDR_MASK, i1);
        if e1 & VALID_MASK != TABLE { return None; }
        let e2 = table_entry(e1 & ADDR_MASK, i2);
        if e2 & VALID_MASK != TABLE { return None; }
        let e3 = table_entry(e2 & ADDR_MASK, i3);
        if e3 & VALID_MASK != PAGE { return None; }
        let ap = (e3 >> 6) & 0b11;
        if ap & 0b01 == 0 { return None; }        // AP[1]=0: so EL1
        if write && ap & 0b10 != 0 { return None; } // AP[2]=1: somente leitura
        Some((e3 & ADDR_MASK) | (va & 0xFFF))
    }
}
