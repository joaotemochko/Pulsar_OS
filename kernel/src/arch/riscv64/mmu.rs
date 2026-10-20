//! MMU RISC-V Sv39: tabelas de 3 niveis, paginas de 4 KiB, ASIDs.
//!
//! Raiz do kernel (compartilhada por copia em todo espaco de usuario):
//!   [2]   gigapagina 0x8000_0000 (RAM: kernel, heap, regiao grafica), global
//!   [256] gigapagina PA 0 em MMIO_VA (UART, RTC, virtio), global, sem X
//! O resto da raiz (GB0, GB1, ...) e privado de cada processo.
use core::arch::asm;
use crate::frame_allocator;
use super::sbi;

#[repr(C, align(4096))]
struct Table { e: [u64; 512] }

static mut ROOT: Table = Table { e: [0; 512] };

const V: u64 = 1 << 0;
const R: u64 = 1 << 1;
const W: u64 = 1 << 2;
const X: u64 = 1 << 3;
const U: u64 = 1 << 4;
const G: u64 = 1 << 5;
const A: u64 = 1 << 6;
const D: u64 = 1 << 7;
const LEAF: u64 = R | W | X;
/// Bit RSW (8, livre para o software): pagina de um objeto de memoria
/// compartilhada (shm.rs). Nao e liberada com o espaco; o fork a compartilha.
const RSW_SHARED: u64 = 1 << 8;

const SATP_SV39: u64 = 8 << 60;
/// Gigapaginas da RAM: indice da raiz de cada GiB de RAM_BASE ate RAM_END.
const IDX_RAM: usize = (super::platform::RAM_BASE >> 30) as usize;
const NRAM: usize = (super::platform::RAM_SIZE.div_ceil(1 << 30)) as usize;
#[inline]
fn is_ram_idx(i: usize) -> bool { i >= IDX_RAM && i < IDX_RAM + NRAM }
const IDX_MMIO: usize = 256;

#[derive(Clone, Copy)]
pub struct PageFlags {
    pub bits: u64,
}

impl PageFlags {
    pub fn kernel_rwx() -> Self { Self { bits: R | W | X | A | D | G } }
    pub fn device() -> Self { Self { bits: R | W | A | D | G } }
    /// Codigo de usuario: R+X (W^X).
    pub fn user_code() -> Self { Self { bits: R | X | U | A } }
    /// Dados/pilha de usuario: R+W, nunca executavel.
    pub fn user_data() -> Self { Self { bits: R | W | U | A | D } }
    /// Regiao grafica no espaco do processo (mesmos bits; privada aqui).
    pub fn user_shared() -> Self { Self::user_data() }
    /// Marca a pagina como de um objeto compartilhado (ver RSW_SHARED).
    pub fn shared(self) -> Self { Self { bits: self.bits | RSW_SHARED } }
    /// Pagina de mmap com protecao `prot` (bit0 R, bit1 W, bit2 X).
    /// Sem nenhum bit: R sem U (so o kernel acessa: PROT_NONE). W sozinho
    /// nao existe no Sv39, entao W implica R.
    pub fn user_prot(prot: u64) -> Self {
        if prot & 7 == 0 { return Self { bits: R | A }; }
        let mut b = R | U | A;
        if prot & 2 != 0 { b |= W | D; }
        if prot & 4 != 0 { b |= X; }
        Self { bits: b }
    }
}

#[inline]
fn pte_pa(e: u64) -> u64 { (e >> 10) << 12 }
#[inline]
fn pa_pte(pa: u64) -> u64 { (pa >> 12) << 10 }
#[inline]
fn vpn(va: u64, level: u32) -> usize { ((va >> (12 + 9 * level)) & 0x1FF) as usize }

unsafe fn entry(table: u64, idx: usize) -> u64 {
    unsafe { (table as *const u64).add(idx).read_volatile() }
}
unsafe fn set_entry(table: u64, idx: usize, v: u64) {
    unsafe { (table as *mut u64).add(idx).write_volatile(v) }
}

pub fn kernel_root() -> u64 {
    (&raw const ROOT) as u64
}

pub unsafe fn init() {
    unsafe {
        let root = kernel_root();
        // gigapaginas exigem PA alinhado a 1 GiB
        for k in 0..NRAM {
            let pa = (((IDX_RAM + k) as u64) << 30) as u64;
            set_entry(root, IDX_RAM + k, pa_pte(pa) | PageFlags::kernel_rwx().bits | V);
        }
        set_entry(root, IDX_MMIO, pa_pte(0) | PageFlags::device().bits | V);
        enable_on_this_hart();
    }
}

pub unsafe fn enable_on_this_hart() {
    let satp = SATP_SV39 | (kernel_root() >> 12);
    unsafe { asm!("csrw satp, {}", "sfence.vma", in(reg) satp, options(nostack)) };
}

pub fn is_enabled() -> bool {
    let satp: u64;
    unsafe { asm!("csrr {}, satp", out(reg) satp, options(nomem, nostack)) };
    satp >> 60 == 8
}

/// Proxima tabela no indice `idx`, criando-a se ausente. None se a
/// entrada for uma folha (gigapagina do kernel) ou faltar memoria.
unsafe fn next_table(table: u64, idx: usize) -> Option<u64> {
    unsafe {
        let e = entry(table, idx);
        if e & V != 0 {
            if e & LEAF != 0 { return None; }
            return Some(pte_pa(e));
        }
        let new = frame_allocator::alloc_frame()?;
        core::ptr::write_bytes(new as *mut u8, 0, 4096);
        set_entry(table, idx, pa_pte(new) | V);
        Some(new)
    }
}

pub unsafe fn map_page_locked(root: u64, va: u64, pa: u64, flags: PageFlags) -> bool {
    unsafe {
        let Some(l1) = next_table(root, vpn(va, 2)) else { return false };
        let Some(l0) = next_table(l1, vpn(va, 1)) else { return false };
        set_entry(l0, vpn(va, 0), pa_pte(pa) | flags.bits | V);
    }
    true
}

pub unsafe fn map_page_in(root: u64, va: u64, pa: u64, flags: PageFlags) -> bool {
    let _g = crate::sync::MMU.guard();
    unsafe { map_page_locked(root, va, pa, flags) }
}

pub fn create_user_space() -> Option<u64> {
    let _g = crate::sync::MMU.guard();
    let root = frame_allocator::alloc_frame()?;
    unsafe {
        core::ptr::write_bytes(root as *mut u8, 0, 4096);
        for k in IDX_RAM..IDX_RAM + NRAM {
            set_entry(root, k, entry(kernel_root(), k));
        }
        set_entry(root, IDX_MMIO, entry(kernel_root(), IDX_MMIO));
    }
    Some(root)
}

/// Libera tabelas e paginas privadas (folhas fora do heap, como a regiao
/// grafica, sao ignoradas pelo frame allocator).
pub fn free_user_space(root: u64) {
    let _g = crate::sync::MMU.guard();
    unsafe {
        for i2 in 0..512 {
            if is_ram_idx(i2) || i2 == IDX_MMIO { continue; }
            let e2 = entry(root, i2);
            if e2 & V == 0 || e2 & LEAF != 0 { continue; }
            let l1 = pte_pa(e2);
            for i1 in 0..512 {
                let e1 = entry(l1, i1);
                if e1 & V == 0 { continue; }
                if e1 & LEAF != 0 { continue; }
                let l0 = pte_pa(e1);
                for i0 in 0..512 {
                    let e0 = entry(l0, i0);
                    if e0 & V != 0 && e0 & LEAF != 0 && e0 & RSW_SHARED == 0 {
                        frame_allocator::free_frame(pte_pa(e0));
                    }
                }
                frame_allocator::free_frame(l0);
            }
            frame_allocator::free_frame(l1);
        }
        frame_allocator::free_frame(root);
    }
}

pub fn switch_space(root: u64, asid: u16) {
    let satp = SATP_SV39 | ((asid as u64) << 44) | (root >> 12);
    unsafe { asm!("csrw satp, {}", in(reg) satp, options(nostack)) };
}

/// Torna visiveis novas entradas de tabela nesta hart (entradas novas
/// nunca estao em cache como invalidas nas demais).
pub fn flush_tlb() {
    unsafe { asm!("sfence.vma", options(nostack)) };
}

#[inline]
pub fn sync_new_pages() {
    unsafe { asm!("sfence.vma", options(nostack)) };
}

pub fn flush_asid(asid: u16) {
    unsafe { asm!("sfence.vma zero, {}", in(reg) asid as u64, options(nostack)) };
    sbi::remote_sfence_asid(asid as u64);
}

pub fn flush_range(asid: u16, start: u64, end: u64) {
    if start < end {
        flush_asid(asid);
    }
}

pub fn translate_user(root: u64, va: u64, write: bool) -> Option<u64> {
    if va >= (1 << 38) { return None; }
    unsafe {
        let mut table = root;
        let mut level = 2u32;
        loop {
            let e = entry(table, vpn(va, level));
            if e & V == 0 { return None; }
            if e & LEAF != 0 {
                if e & U == 0 { return None; }
                if write && e & W == 0 { return None; }
                let page_mask = (1u64 << (12 + 9 * level)) - 1;
                return Some((pte_pa(e) & !page_mask) | (va & page_mask));
            }
            if level == 0 { return None; }
            table = pte_pa(e);
            level -= 1;
        }
    }
}

/// Visita as folhas de 4 KiB existentes de [start, end) sem criar tabelas;
/// niveis ausentes sao pulados inteiros. Gigapaginas (kernel) sao ignoradas.
unsafe fn walk_leaves(root: u64, start: u64, end: u64, mut f: impl FnMut(u64, *mut u64)) {
    let next = |va: u64, shift: u32| ((va >> shift) + 1) << shift;
    let mut va = start & !0xFFF;
    let end = end.min(1 << 38);
    while va < end {
        unsafe {
            let e2 = entry(root, vpn(va, 2));
            if e2 & V == 0 || e2 & LEAF != 0 { va = next(va, 30); continue; }
            let e1 = entry(pte_pa(e2), vpn(va, 1));
            if e1 & V == 0 || e1 & LEAF != 0 { va = next(va, 21); continue; }
            f(va, (pte_pa(e1) as *mut u64).add(vpn(va, 0)));
        }
        va += 4096;
    }
}

/// Desmapeia [start, end) e devolve os frames. Retorna quantas PTEs validas foram removidas.
pub fn unmap_range(root: u64, start: u64, end: u64) -> usize {
    let _g = crate::sync::MMU.guard();
    let mut count = 0;
    unsafe {
        walk_leaves(root, start, end, |_, p| {
            let e = p.read_volatile();
            if e & V != 0 {
                p.write_volatile(0);
                if e & RSW_SHARED == 0 { frame_allocator::free_frame(pte_pa(e)); }
                count += 1;
            }
        });
    }
    count
}

/// Troca as permissoes das paginas presentes em [start, end). Retorna quantas PTEs mudaram.
pub fn protect_range(root: u64, start: u64, end: u64, flags: PageFlags) -> usize {
    let _g = crate::sync::MMU.guard();
    let mut count = 0;
    unsafe {
        walk_leaves(root, start, end, |_, p| {
            let e = p.read_volatile();
            if e & V != 0 {
                let new_e = pa_pte(pte_pa(e)) | flags.bits | V | (e & RSW_SHARED);
                if new_e != e {
                    p.write_volatile(new_e);
                    count += 1;
                }
            }
        });
    }
    count
}

/// Move as PTEs presentes de [old_start, old_end) para `new_start` (mremap).
pub fn move_range(root: u64, old_start: u64, old_end: u64, new_start: u64) -> usize {
    let _g = crate::sync::MMU.guard();
    let mut count = 0;
    unsafe {
        walk_leaves(root, old_start, old_end, |va, p| {
            let e = p.read_volatile();
            if e & V != 0 {
                let dst_va = new_start + (va - old_start);
                let flags = PageFlags { bits: e & 0x3FE };
                if map_page_locked(root, dst_va, pte_pa(e), flags) {
                    p.write_volatile(0);
                    count += 1;
                }
            }
        });
    }
    count
}

/// A pagina de `va` esta mapeada (com qualquer permissao)?
pub fn is_mapped(root: u64, va: u64) -> bool {
    let _g = crate::sync::MMU.guard();
    let mut found = false;
    unsafe { walk_leaves(root, va, va + 1, |_, p| found = p.read_volatile() & V != 0) };
    found
}

/// fork(): espaco novo com uma COPIA de cada pagina privada gravavel de `src`
/// e compartilhamento das paginas somente-leitura.
pub fn clone_user_space(src: u64) -> Option<u64> {
    let dst = create_user_space()?;
    let _g = crate::sync::MMU.guard();
    unsafe {
        for i2 in 0..512 {
            if is_ram_idx(i2) || i2 == IDX_MMIO { continue; }
            let e2 = entry(src, i2);
            if e2 & V == 0 || e2 & LEAF != 0 { continue; }
            let l1 = pte_pa(e2);
            for i1 in 0..512 {
                let e1 = entry(l1, i1);
                if e1 & V == 0 || e1 & LEAF != 0 { continue; }
                let l0 = pte_pa(e1);
                for i0 in 0..512 {
                    let e0 = entry(l0, i0);
                    if e0 & V == 0 || e0 & LEAF == 0 { continue; }
                    let va = ((i2 as u64) << 30) | ((i1 as u64) << 21) | ((i0 as u64) << 12);
                    let pa = pte_pa(e0);
                    let flags = PageFlags { bits: e0 & 0x3FE };
                    let read_only = (e0 & W) == 0;
                    let (target, child_flags) = if frame_allocator::reserved(pa) || e0 & RSW_SHARED != 0 {
                        (pa, flags)
                    } else if read_only {
                        frame_allocator::inc_ref(pa);
                        (pa, flags)
                    } else {
                        // Copy-on-write: limpa bit W no pai e no filho e incrementa ref
                        set_entry(l0, i0, e0 & !W);
                        frame_allocator::inc_ref(pa);
                        (pa, PageFlags { bits: flags.bits & !W })
                    };
                    if !map_page_locked(dst, va, target, child_flags) {
                        if !frame_allocator::reserved(pa) && e0 & RSW_SHARED == 0 {
                            frame_allocator::free_frame(target);
                        }
                        drop(_g);
                        free_user_space(dst);
                        return None;
                    }
                }
            }
        }
        flush_tlb();
    }
    Some(dst)
}
