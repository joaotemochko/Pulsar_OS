//! Alocador de frames fisicos de 4 KiB por bitmap, sobre a janela
//! [HEAP_START, HEAP_START + HEAP_SIZE) definida pela arquitetura.
//! Protegido pela trava FRAME (folha: nunca pega outra trava).
use crate::arch::platform::{FB_PA, FB_REGION_BYTES, HEAP_SIZE, HEAP_START};
use crate::sync::SpinLock;

static FRAME: SpinLock = SpinLock::new();

const PAGE_SIZE: u64 = 4096;
const NUM_FRAMES: usize = (HEAP_SIZE / PAGE_SIZE) as usize;
const WORDS: usize = NUM_FRAMES.div_ceil(64);

/// bit i = frame i (1 = usado). Vive na BSS (tudo livre no boot).
static mut BITMAP: [u64; WORDS] = [0; WORDS];
/// Contagem de referencias por frame (para compartilhar paginas somente-leitura no fork).
static mut REFCOUNT: [u16; NUM_FRAMES] = [0; NUM_FRAMES];
/// Dica de onde comecar a busca (next-fit).
static mut HINT: usize = 0;
static mut FREE: usize = NUM_FRAMES;

#[inline]
fn bm() -> &'static mut [u64; WORDS] {
    unsafe { &mut *(&raw mut BITMAP) }
}

#[inline]
fn refs() -> &'static mut [u16; NUM_FRAMES] {
    unsafe { &mut *(&raw mut REFCOUNT) }
}

pub fn init() {
    // bits alem de NUM_FRAMES na ultima palavra ficam "usados"
    let extra = WORDS * 64 - NUM_FRAMES;
    if extra > 0 {
        bm()[WORDS - 1] |= !0u64 << (64 - extra);
    }
    // a regiao grafica fica dentro da RAM gerenciada: reserva os frames (se cair no heap)
    if FB_PA >= HEAP_START && FB_PA < HEAP_START + HEAP_SIZE {
        let mut a = FB_PA;
        while a < FB_PA + FB_REGION_BYTES && a < HEAP_START + HEAP_SIZE {
            set_used(((a - HEAP_START) / PAGE_SIZE) as usize);
            a += PAGE_SIZE;
        }
    }
}

/// Frames que nunca sao devolvidos nem copiados num fork: a regiao grafica
/// mapeada para os apps e paginas da imagem do kernel expostas ao usuario
/// (a pagina de retorno de sinal).
#[inline]
pub fn reserved(addr: u64) -> bool {
    (addr >= FB_PA && addr < FB_PA + FB_REGION_BYTES) || addr < HEAP_START
}

#[inline]
fn is_free(idx: usize) -> bool {
    bm()[idx / 64] & (1 << (idx % 64)) == 0
}

#[inline]
fn set_used(idx: usize) {
    bm()[idx / 64] |= 1 << (idx % 64);
    refs()[idx] = 1;
    unsafe { FREE -= 1; }
}

#[inline]
fn addr_of(idx: usize) -> u64 {
    HEAP_START + idx as u64 * PAGE_SIZE
}

/// Incrementa a contagem de referencias de um frame privado (ex.: paginas
/// somente-leitura compartilhadas no fork).
pub fn inc_ref(addr: u64) {
    if addr < HEAP_START || addr >= HEAP_START + HEAP_SIZE || reserved(addr) {
        return;
    }
    let idx = ((addr - HEAP_START) / PAGE_SIZE) as usize;
    let _g = FRAME.guard();
    let r = &mut refs()[idx];
    *r = r.saturating_add(1);
}

/// Retorna a contagem de referencias de um frame (usado no copy-on-write).
pub fn ref_count(addr: u64) -> usize {
    if addr < HEAP_START || addr >= HEAP_START + HEAP_SIZE || reserved(addr) {
        return 1;
    }
    let idx = ((addr - HEAP_START) / PAGE_SIZE) as usize;
    let _g = FRAME.guard();
    refs()[idx] as usize
}

/// Aloca um frame de 4KB. Retorna o endereco fisico.
pub fn alloc_frame() -> Option<u64> {
    let _g = FRAME.guard();
    let start = unsafe { HINT } / 64;
    for k in 0..WORDS {
        let w = (start + k) % WORDS;
        let word = bm()[w];
        if word != u64::MAX {
            let bit = (!word).trailing_zeros() as usize;
            let idx = w * 64 + bit;
            set_used(idx);
            unsafe { HINT = idx; }
            return Some(addr_of(idx));
        }
    }
    None
}

#[inline]
fn free_idx_locked(idx: usize) {
    if !is_free(idx) {
        let r = &mut refs()[idx];
        if *r > 1 {
            *r -= 1;
            return;
        }
        *r = 0;
        bm()[idx / 64] &= !(1 << (idx % 64));
        unsafe {
            FREE += 1;
            if idx < HINT { HINT = idx; }
        }
    }
}

/// Libera um frame. Enderecos fora da janela (MMIO, regiao grafica) sao
/// ignorados, o que permite liberar tabelas de usuario sem distinguir.
pub fn free_frame(addr: u64) {
    if addr < HEAP_START || addr >= HEAP_START + HEAP_SIZE || reserved(addr) {
        return;
    }
    let idx = ((addr - HEAP_START) / PAGE_SIZE) as usize;
    let _g = FRAME.guard();
    free_idx_locked(idx);
}

pub fn free_count() -> usize {
    unsafe { FREE }
}

pub fn total_count() -> usize {
    NUM_FRAMES
}

/// Aloca `n` frames fisicamente contiguos.
pub fn alloc_contig(n: usize) -> Option<u64> {
    if n == 0 { return None; }
    if n == 1 { return alloc_frame(); }
    let _g = FRAME.guard();
    let mut run = 0usize;
    let mut start = 0usize;
    let mut idx = 0usize;
    while idx < NUM_FRAMES {
        if run == 0 && idx % 64 == 0 && bm()[idx / 64] == u64::MAX {
            idx += 64;
            continue;
        }
        if is_free(idx) {
            if run == 0 { start = idx; }
            run += 1;
            if run == n {
                for i in start..start + n { set_used(i); }
                return Some(addr_of(start));
            }
        } else {
            run = 0;
        }
        idx += 1;
    }
    None
}

pub fn free_contig(addr: u64, n: usize) {
    if addr < HEAP_START || addr >= HEAP_START + HEAP_SIZE || reserved(addr) || n == 0 {
        return;
    }
    let base = ((addr - HEAP_START) / PAGE_SIZE) as usize;
    let end = (base + n).min(NUM_FRAMES);
    let _g = FRAME.guard();
    for idx in base..end {
        free_idx_locked(idx);
    }
}
