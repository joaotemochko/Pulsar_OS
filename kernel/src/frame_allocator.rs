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
/// Dica de onde comecar a busca (next-fit).
static mut HINT: usize = 0;
static mut FREE: usize = NUM_FRAMES;

#[inline]
fn bm() -> &'static mut [u64; WORDS] {
    unsafe { &mut *(&raw mut BITMAP) }
}

pub fn init() {
    // bits alem de NUM_FRAMES na ultima palavra ficam "usados"
    let extra = WORDS * 64 - NUM_FRAMES;
    if extra > 0 {
        bm()[WORDS - 1] |= !0u64 << (64 - extra);
    }
    // a regiao grafica fica dentro da RAM gerenciada: reserva os frames
    let mut a = FB_PA;
    while a < FB_PA + FB_REGION_BYTES {
        set_used(((a - HEAP_START) / PAGE_SIZE) as usize);
        a += PAGE_SIZE;
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
    unsafe { FREE -= 1; }
}

#[inline]
fn addr_of(idx: usize) -> u64 {
    HEAP_START + idx as u64 * PAGE_SIZE
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

/// Libera um frame. Enderecos fora da janela (MMIO, regiao grafica) sao
/// ignorados, o que permite liberar tabelas de usuario sem distinguir.
pub fn free_frame(addr: u64) {
    if addr < HEAP_START || addr >= HEAP_START + HEAP_SIZE || reserved(addr) {
        return;
    }
    let idx = ((addr - HEAP_START) / PAGE_SIZE) as usize;
    let _g = FRAME.guard();
    if !is_free(idx) {
        bm()[idx / 64] &= !(1 << (idx % 64));
        unsafe { FREE += 1; }
    }
}

pub fn free_count() -> usize {
    unsafe { FREE }
}

pub fn total_count() -> usize {
    NUM_FRAMES
}

/// Aloca `n` frames fisicamente contiguos.
pub fn alloc_contig(n: usize) -> Option<u64> {
    let _g = FRAME.guard();
    let mut run = 0usize;
    let mut start = 0usize;
    for idx in 0..NUM_FRAMES {
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
    }
    None
}

pub fn free_contig(addr: u64, n: usize) {
    for i in 0..n {
        free_frame(addr + (i as u64) * PAGE_SIZE);
    }
}
