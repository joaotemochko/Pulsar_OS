use core::sync::atomic::{AtomicBool, Ordering};

/// Inicio da RAM gerenciada pelo alocador (depois do kernel + regiao de usuario).
const HEAP_START: u64 = 0x4020_0000;
/// Tamanho da janela gerenciada: 64 MB.
const HEAP_SIZE: u64 = 64 * 1024 * 1024;
/// Tamanho de uma pagina (frame).
const PAGE_SIZE: u64 = 4096;
/// Numero total de frames gerenciados.
const NUM_FRAMES: usize = (HEAP_SIZE / PAGE_SIZE) as usize; // 16384
/// Tamanho do bitmap em bytes (1 bit por frame).
const BITMAP_BYTES: usize = NUM_FRAMES / 8; // 2048

/// Bitmap: bit i = frame i. 0 = livre, 1 = usado. Vive na BSS (zerado no boot).
static mut BITMAP: [u8; BITMAP_BYTES] = [0; BITMAP_BYTES];

/// Trava simples para evitar uso antes de init (e marcar "ja inicializado").
static INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Inicializa o alocador. Por enquanto so marca como pronto — o bitmap ja
/// nasce zerado (tudo livre) gracas ao zero da BSS no boot.
pub fn init() {
    INITIALIZED.store(true, Ordering::SeqCst);
}

/// Marca um frame como usado no bitmap.
fn set_used(idx: usize) {
    unsafe {
        let byte = &raw mut BITMAP[idx / 8];
        *byte |= 1 << (idx % 8);
    }
}

/// Marca um frame como livre no bitmap.
fn set_free(idx: usize) {
    unsafe {
        let byte = &raw mut BITMAP[idx / 8];
        *byte &= !(1 << (idx % 8));
    }
}

/// Verifica se um frame esta livre.
fn is_free(idx: usize) -> bool {
    unsafe {
        let byte = (&raw const BITMAP[idx / 8]).read();
        (byte & (1 << (idx % 8))) == 0
    }
}

/// Aloca um frame de 4KB. Retorna o endereco fisico, ou None se nao houver.
pub fn alloc_frame() -> Option<u64> {
    for idx in 0..NUM_FRAMES {
        if is_free(idx) {
            set_used(idx);
            let addr = HEAP_START + (idx as u64) * PAGE_SIZE;
            return Some(addr);
        }
    }
    None
}

/// Libera um frame previamente alocado.
pub fn free_frame(addr: u64) {
    if addr < HEAP_START || addr >= HEAP_START + HEAP_SIZE {
        return; // fora da janela gerenciada — ignora
    }
    let idx = ((addr - HEAP_START) / PAGE_SIZE) as usize;
    set_free(idx);
}

/// Conta quantos frames estao livres (util para diagnostico).
pub fn free_count() -> usize {
    (0..NUM_FRAMES).filter(|&i| is_free(i)).count()
}
/// Aloca `n` frames FISICAMENTE CONTIGUOS. Retorna o endereco do primeiro.
/// Usado para buffers de arquivo (leitura de disco) e stacks de processo.
pub fn alloc_contig(n: usize) -> Option<u64> {
    let mut run = 0usize;
    let mut start = 0usize;
    for idx in 0..NUM_FRAMES {
        if is_free(idx) {
            if run == 0 {
                start = idx;
            }
            run += 1;
            if run == n {
                for i in start..start + n {
                    set_used(i);
                }
                return Some(HEAP_START + (start as u64) * PAGE_SIZE);
            }
        } else {
            run = 0;
        }
    }
    None
}

/// Libera `n` frames contiguos a partir de `addr`.
pub fn free_contig(addr: u64, n: usize) {
    for i in 0..n {
        free_frame(addr + (i as u64) * PAGE_SIZE);
    }
}
