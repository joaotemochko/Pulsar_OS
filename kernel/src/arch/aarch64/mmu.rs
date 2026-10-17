use core::arch::asm;
use aarch64_cpu::registers::*;
use tock_registers::interfaces::{Readable, Writeable};
use crate::frame_allocator;

#[repr(C, align(4096))]
struct Table { e: [u64; 512] }

// So a L0 raiz continua estatica — e o ponto de entrada do walk.
// Todas as outras tabelas (L1/L2/L3) sao alocadas dinamicamente.
static mut L0: Table = Table { e: [0; 512] };

const TABLE: u64 = 0b11;   // descritor de tabela (aponta p/ proximo nivel)
const PAGE:  u64 = 0b11;   // descritor de pagina (L3)
const VALID_MASK: u64 = 0b11;

// Flags de pagina, agrupados numa struct para clareza no loader.
#[derive(Clone, Copy)]
pub struct PageFlags {
    pub bits: u64,
}

impl PageFlags {
    const AF:    u64 = 1 << 10;
    const SH_IS: u64 = 0b11 << 8;
    const IDX_DEV: u64 = 0 << 2;
    const IDX_NRM: u64 = 1 << 2;
    const AP_KERNEL: u64 = 0b00 << 6;  // RW so EL1 -> executavel em EL1
    const AP_USER:   u64 = 0b01 << 6;  // RW EL0+EL1 -> executavel em EL0
    const AP_USER_RO: u64 = 0b11 << 6;  // RO EL0+EL1 (nao-gravavel)
    const PXN: u64 = 1 << 53;
    const UXN: u64 = 1 << 54;
    /// not-Global: a traducao e valida SO para o ASID corrente. Sem este
    /// bit, entradas de TLB de paginas de usuario ignorariam o ASID e
    /// vazariam entre processos, anulando o isolamento.
    const NG: u64 = 1 << 11;

    /// Codigo/dados do kernel: Normal, RW so EL1, executavel em EL1.
    pub fn kernel_rwx() -> Self {
        Self { bits: Self::IDX_NRM | Self::AP_KERNEL | Self::AF | Self::SH_IS }
    }

    /// Periferico (UART/GIC): Device, RW so EL1, nunca executavel.
    pub fn device() -> Self {
        Self { bits: Self::IDX_DEV | Self::AP_KERNEL | Self::AF | Self::PXN | Self::UXN }
    }

    /// Dados do kernel (ex.: framebuffer): Normal, RW so EL1, nao-executavel.
    pub fn kernel_data() -> Self {
        Self { bits: Self::IDX_NRM | Self::AP_KERNEL | Self::AF | Self::SH_IS | Self::PXN | Self::UXN }
    }

    /// Codigo de usuario: Normal, RW EL0+EL1, executavel em EL0, nao-exec em EL1.
    pub fn user_code() -> Self {
        Self { bits: Self::IDX_NRM | Self::AP_USER_RO | Self::AF | Self::SH_IS | Self::PXN | Self::NG }
    }

    /// Dados/stack de usuario: Normal, RW EL0+EL1, nao-executavel.
    pub fn user_data() -> Self {
        Self { bits: Self::IDX_NRM | Self::AP_USER | Self::AF | Self::SH_IS | Self::PXN | Self::UXN | Self::NG }
    }
}

/// Indices de cada nivel para um dado VA (4KB granule, VA 48 bits).
#[inline]
fn indices(va: u64) -> (usize, usize, usize, usize) {
    (
        ((va >> 39) & 0x1FF) as usize,
        ((va >> 30) & 0x1FF) as usize,
        ((va >> 21) & 0x1FF) as usize,
        ((va >> 12) & 0x1FF) as usize,
    )
}

/// Garante que a entrada `idx` da tabela em `table_pa` aponta para uma
/// tabela do proximo nivel; aloca uma nova (zerada) se necessario.
/// Retorna o endereco fisico da tabela do proximo nivel.
unsafe fn next_table(table_pa: u64, idx: usize) -> u64 {
    unsafe {
        let entry_ptr = (table_pa as *mut u64).add(idx);
        let entry = entry_ptr.read_volatile();
        if entry & VALID_MASK == TABLE {
            entry & 0x0000_FFFF_FFFF_F000
        } else {
            let new_pa = frame_allocator::alloc_frame().expect("sem frames para page table");
            core::ptr::write_bytes(new_pa as *mut u8, 0, 4096);
            entry_ptr.write_volatile(new_pa | TABLE);
            new_pa
        }
    }
}

/// Raiz (L0) do espaco de enderecos do KERNEL.
pub fn kernel_root() -> u64 {
    (&raw const L0) as u64
}

/// Mapeia uma pagina de 4KB no espaco cuja raiz L0 e `root_pa`.
/// Constroi os niveis intermediarios sob demanda via frame allocator.
pub unsafe fn map_page_in(root_pa: u64, va: u64, pa: u64, flags: PageFlags) {
    let (i0, i1, i2, i3) = indices(va);
    unsafe {
        let l1_pa = next_table(root_pa, i0);
        let l2_pa = next_table(l1_pa, i1);
        let l3_pa = next_table(l2_pa, i2);
        let l3_entry = (l3_pa as *mut u64).add(i3);
        l3_entry.write_volatile((pa & 0x0000_FFFF_FFFF_F000) | PAGE | flags.bits);
    }
}

/// Mapeia uma pagina no espaco do KERNEL (usado no boot).
pub unsafe fn map_page(va: u64, pa: u64, flags: PageFlags) {
    unsafe { map_page_in(kernel_root(), va, pa, flags) }
}

/// Mapeia um intervalo [va, va+size) identity (VA==PA) com os flags dados.
pub unsafe fn map_range_identity(start: u64, size: u64, flags: PageFlags) {
    let mut off = 0u64;
    while off < size {
        unsafe { map_page(start + off, start + off, flags); }
        off += 4096;
    }
}

pub unsafe fn init() {
    unsafe {
        // UART (PL011) em 0x09000000
        map_page(0x0900_0000, 0x0900_0000, PageFlags::device());

        // GICv2: distribuidor (GICD) e interface de CPU (GICC)
        map_page(0x0800_0000, 0x0800_0000, PageFlags::device()); // GICD
        map_page(0x0801_0000, 0x0801_0000, PageFlags::device()); // GICC

        // virtio-mmio: 32 slots de 0x200 a partir de 0x0a000000 (16KB)
        map_range_identity(0x0a00_0000, 0x4000, PageFlags::device());

        // Framebuffer: RAM alta, fora da janela do frame allocator
        map_range_identity(crate::fb::FB_BASE, crate::fb::FB_MAX_BYTES, PageFlags::kernel_data());

        // Kernel + stacks: identity em 0x40000000, 2MB, kernel_rwx.
        map_range_identity(0x4000_0000, 0x20_0000, PageFlags::kernel_rwx());

        // Janela COMPLETA do frame allocator (64MB) — identity, para o
        // kernel acessar page tables, buffers de disco e stacks alocadas
        // com a MMU ligada. (Antes so 16MB eram mapeados: frames alem de
        // 0x4120_0000 faultavam.)
        map_range_identity(0x4020_0000, 0x400_0000, PageFlags::kernel_rwx());
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

    TTBR0_EL1.set((&raw const L0) as u64);

    unsafe { asm!("dsb ish", "tlbi vmalle1", "dsb ish", "isb", options(nostack, preserves_flags)); }

    let mut sctlr = SCTLR_EL1.get();
    sctlr &= !(1u64 << 19);
    sctlr |= (1 << 0) | (1 << 2) | (1 << 12);
    SCTLR_EL1.set(sctlr);

    unsafe { asm!("isb", "ic iallu", "tlbi vmalle1", "dsb nsh", "isb", options(nostack, preserves_flags)); }
}

pub fn is_enabled() -> bool {
    SCTLR_EL1.is_set(SCTLR_EL1::M)
}
/// Invalida o TLB inteiro (apos remapear paginas ja usadas).
pub fn flush_tlb() {
    unsafe { asm!("dsb ish", "tlbi vmalle1", "dsb ish", "isb", options(nostack, preserves_flags)); }
}

/// Le a entrada `idx` da tabela em `table_pa` (endereco fisico do alvo).
unsafe fn table_entry(table_pa: u64, idx: usize) -> u64 {
    unsafe { (table_pa as *const u64).add(idx).read_volatile() }
}

/// Cria um espaco de enderecos de USUARIO novo, compartilhando as
/// subarvores do kernel:
///
///   L0 (privada)
///    └─[0] L1 (privada)
///          ├─[0] L2 do GB0 (privada) ── entradas de dispositivo (L3
///          │       compartilhadas do kernel: GIC, UART, virtio);
///          │       regiao de usuario (indices 8..49) comeca VAZIA
///          └─[1] subarvore do GB1 do kernel (COMPARTILHADA: kernel,
///                  heap/page tables, framebuffer)
///
/// Paginas do kernel sao globais (nG=0): uma unica entrada de TLB serve
/// todos os ASIDs. Paginas de usuario sao nG=1: presas ao ASID do dono.
pub fn create_user_space() -> Option<u64> {
    let l0 = frame_allocator::alloc_frame()?;
    let l1 = frame_allocator::alloc_frame()?;
    let l2_gb0 = frame_allocator::alloc_frame()?;
    unsafe {
        core::ptr::write_bytes(l0 as *mut u8, 0, 4096);
        core::ptr::write_bytes(l1 as *mut u8, 0, 4096);
        core::ptr::write_bytes(l2_gb0 as *mut u8, 0, 4096);

        // Subarvores do kernel
        let k_l1 = table_entry(kernel_root(), 0) & 0x0000_FFFF_FFFF_F000;
        let k_l2_gb0 = table_entry(k_l1, 0) & 0x0000_FFFF_FFFF_F000;

        // L1 privada: GB1 (kernel RAM/fb) compartilhado; GB0 privado
        (l1 as *mut u64).add(1).write_volatile(table_entry(k_l1, 1));
        (l1 as *mut u64).add(0).write_volatile(l2_gb0 | TABLE);

        // L2 do GB0 privada: copia as entradas de dispositivo do kernel
        // (subarvores L3 compartilhadas); resto fica vazio (usuario)
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

/// Troca o espaco de enderecos ativo: TTBR0 = raiz | ASID.
/// Sem flush de TLB — as entradas sao etiquetadas por ASID.
pub fn switch_space(l0_pa: u64, asid: u16) {
    unsafe {
        asm!(
            "msr ttbr0_el1, {v}",
            "isb",
            v = in(reg) (l0_pa | ((asid as u64) << 48)),
            options(nostack, preserves_flags)
        );
    }
}

/// Invalida as entradas de TLB de um ASID especifico (reuso de ASID).
pub fn flush_asid(asid: u16) {
    unsafe {
        asm!(
            "dsb ish",
            "tlbi aside1is, {v}",
            "dsb ish",
            "isb",
            v = in(reg) ((asid as u64) << 48),
            options(nostack, preserves_flags)
        );
    }
}

/// Traduz um VA para PA percorrendo as tabelas cuja raiz e `root_pa`.
/// Retorna None se qualquer nivel estiver ausente. So paginas de 4KB.
pub fn translate(root_pa: u64, va: u64) -> Option<u64> {
    let (i0, i1, i2, i3) = indices(va);
    unsafe {
        let e0 = table_entry(root_pa, i0);
        if e0 & VALID_MASK != TABLE { return None; }
        let l1 = e0 & 0x0000_FFFF_FFFF_F000;
        let e1 = table_entry(l1, i1);
        if e1 & VALID_MASK != TABLE { return None; }
        let l2 = e1 & 0x0000_FFFF_FFFF_F000;
        let e2 = table_entry(l2, i2);
        if e2 & VALID_MASK != TABLE { return None; }
        let l3 = e2 & 0x0000_FFFF_FFFF_F000;
        let e3 = table_entry(l3, i3);
        if e3 & VALID_MASK != PAGE { return None; }
        let page = e3 & 0x0000_FFFF_FFFF_F000;
        Some(page | (va & 0xFFF))
    }
}
