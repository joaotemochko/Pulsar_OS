//! Framebuffer em RAM + primitivas de desenho.
//!
//! O buffer vive num endereco fisico fixo, FORA da janela do frame
//! allocator (que gerencia 0x4020_0000..0x4420_0000), e e identity-mapped
//! pelo mmu::init(). Pixels: u32 little-endian 0x00RRGGBB (casa com o
//! formato B8G8R8X8 do virtio-gpu).
//!
//! Layout de RAM da maquina virt com 128MB (default):
//!   0x4000_0000  kernel (2MB mapeados)
//!   0x4020_0000  janela do frame allocator (64MB)
//!   0x4600_0000  framebuffer (ate 8MB reservados aqui)
//!   0x4800_0000  fim da RAM

use core::ptr::write_volatile;

pub const FB_BASE: u64 = 0x4600_0000;
/// Espaco total reservado: 2 buffers de 4MB (front = scanout do GPU,
/// back = superficie de desenho). Present copia back -> front, eliminando
/// tearing de redraws parciais na tela.
pub const FB_MAX_BYTES: u64 = 32 * 1024 * 1024;
pub const BACK_OFFSET: u64 = 4 * 1024 * 1024;
pub const BACK_BASE: u64 = FB_BASE + BACK_OFFSET;
// Buffer do wallpaper cacheado (desenhado 1x, copiado por regioes).
pub const WALL_OFFSET: u64 = 8 * 1024 * 1024;
pub const WALL_BASE: u64 = FB_BASE + WALL_OFFSET;
// Superficies por app: 4 slots de 2MB cada (cabe 512x800x4 ~ 1.6MB).
pub const SURF_BASE: u64 = FB_BASE + 12*1024*1024;
pub const SURF_SLOT_BYTES: u64 = 2*1024*1024;
pub const SURF_SLOTS: u64 = 6;
/// Endereco base da superficie do slot `n` (0..3).
pub const fn surf_addr(n: u64) -> u64 { SURF_BASE + n*SURF_SLOT_BYTES }

static mut WIDTH: u32 = 0;
static mut HEIGHT: u32 = 0;

pub fn init(width: u32, height: u32) {
    unsafe {
        WIDTH = width;
        HEIGHT = height;
    }
}

/// Remapeia o framebuffer com permissao EL0 (chamado por SYS_FB_MAP).
/// Nota: com TTBR0 compartilhado, isso o expoe a todos os processos.
/// Mapeia o BACK buffer no espaco do processo ATUAL (o front, que e o
/// scanout DMA, continua exclusivo do kernel). Com espacos por processo,
/// cada cliente ganha o mapeamento explicitamente via SYS_FB_MAP.
pub fn map_for_user() {
    let root = crate::process::current_l0();
    // mapeia do BACK ate o fim da regiao (back + wallpaper) para EL0
    let mut off = BACK_OFFSET;
    while off < FB_MAX_BYTES {
        unsafe {
            crate::mmu::map_page_in(root, FB_BASE + off, FB_BASE + off,
                crate::mmu::PageFlags::user_data());
        }
        off += 4096;
    }
}

/// Mapeia a superficie do slot `n` para o espaco do processo atual.
pub fn map_surface_for_user(n: u64) {
    let root = crate::process::current_l0();
    let base = surf_addr(n);
    let mut off = 0u64;
    while off < SURF_SLOT_BYTES {
        unsafe {
            crate::mmu::map_page_in(root, base + off, base + off,
                crate::mmu::PageFlags::user_data());
        }
        off += 4096;
    }
    crate::mmu::flush_tlb();
}

#[inline]
pub fn size() -> (u32, u32) {
    unsafe { (WIDTH, HEIGHT) }
}

#[inline]
fn ptr_at(x: u32, y: u32) -> *mut u32 {
    // primitivas do kernel desenham no BACK buffer, como o userspace
    (BACK_BASE + ((y as u64 * unsafe { WIDTH } as u64 + x as u64) * 4)) as *mut u32
}

/// Copia uma regiao do back buffer para o front (damage rect).
/// Cada processo apresenta SO a regiao que possui — assim o snapshot no
/// scanout e sempre consistente, mesmo com varios processos desenhando.
pub fn flip_rect(x: u32, y: u32, rw: u32, rh: u32) {
    let (w, h) = size();
    let x1 = (x + rw).min(w);
    let y1 = (y + rh).min(h);
    let stride = w as usize * 4;
    for yy in y.min(h)..y1 {
        let off = yy as usize * stride + x.min(w) as usize * 4;
        let len = (x1 - x.min(w)) as usize * 4;
        unsafe {
            core::ptr::copy_nonoverlapping(
                (BACK_BASE as usize + off) as *const u8,
                (FB_BASE as usize + off) as *mut u8,
                len,
            );
        }
    }
}

#[inline]
pub fn put_pixel(x: u32, y: u32, color: u32) {
    let (w, h) = size();
    if x < w && y < h {
        unsafe { write_volatile(ptr_at(x, y), color) };
    }
}

pub fn fill_rect(x: u32, y: u32, rw: u32, rh: u32, color: u32) {
    let (w, h) = size();
    let x1 = (x + rw).min(w);
    let y1 = (y + rh).min(h);
    for yy in y.min(h)..y1 {
        let row = ptr_at(0, yy);
        for xx in x.min(w)..x1 {
            unsafe { write_volatile(row.add(xx as usize), color) };
        }
    }
}

pub fn clear(color: u32) {
    let (w, h) = size();
    fill_rect(0, 0, w, h, color);
}

// ---------------------------------------------------------------------------
// Fonte minima 8x8 (glifos proprios) — apenas o necessario por enquanto.
// Cada glifo: 8 bytes, MSB = pixel mais a esquerda.
// ---------------------------------------------------------------------------

fn glyph(c: u8) -> [u8; 8] {
    match c {
        b'P' => [0xFC, 0xC6, 0xC6, 0xFC, 0xC0, 0xC0, 0xC0, 0x00],
        b'U' => [0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x7C, 0x00],
        b'L' => [0xC0, 0xC0, 0xC0, 0xC0, 0xC0, 0xC0, 0xFE, 0x00],
        b'S' => [0x7E, 0xC0, 0xC0, 0x7C, 0x06, 0x06, 0xFC, 0x00],
        b'A' => [0x38, 0x6C, 0xC6, 0xC6, 0xFE, 0xC6, 0xC6, 0x00],
        b'R' => [0xFC, 0xC6, 0xC6, 0xFC, 0xD8, 0xCC, 0xC6, 0x00],
        b'O' => [0x7C, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x7C, 0x00],
        b'E' => [0xFE, 0xC0, 0xC0, 0xF8, 0xC0, 0xC0, 0xFE, 0x00],
        b'G' => [0x7C, 0xC6, 0xC0, 0xCE, 0xC6, 0xC6, 0x7C, 0x00],
        b'I' => [0x7E, 0x18, 0x18, 0x18, 0x18, 0x18, 0x7E, 0x00],
        b'.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x00],
        b'0' => [0x7C, 0xC6, 0xCE, 0xD6, 0xE6, 0xC6, 0x7C, 0x00],
        b'1' => [0x18, 0x38, 0x18, 0x18, 0x18, 0x18, 0x7E, 0x00],
        b'2' => [0x7C, 0xC6, 0x06, 0x1C, 0x70, 0xC0, 0xFE, 0x00],
        b'V' => [0xC6, 0xC6, 0xC6, 0xC6, 0x6C, 0x38, 0x10, 0x00],
        _ => [0; 8], // espaco / desconhecido
    }
}

/// Desenha texto com a fonte 8x8, escalada por `scale`.
pub fn draw_text(x: u32, y: u32, text: &str, color: u32, scale: u32) {
    let mut cx = x;
    for &c in text.as_bytes() {
        let g = glyph(c.to_ascii_uppercase());
        for (row, bits) in g.iter().enumerate() {
            for col in 0..8u32 {
                if bits & (0x80 >> col) != 0 {
                    fill_rect(
                        cx + col * scale,
                        y + row as u32 * scale,
                        scale,
                        scale,
                        color,
                    );
                }
            }
        }
        cx += 9 * scale; // 8px de glifo + 1 de espacamento
    }
}

/// Padrao de teste: fundo em gradiente, barras de cor, borda e titulo.
/// Serve como prova visual de que scanout/transfer/flush funcionam.
pub fn draw_test_pattern() {
    let (w, h) = size();

    // Gradiente azul-escuro vertical
    for y in 0..h {
        let shade = 16 + (y * 48 / h.max(1)) as u32;
        fill_rect(0, y, w, 1, shade / 3 << 16 | shade / 2 << 8 | shade);
    }

    // Barras de cor (estilo SMPTE simplificado) no terco inferior
    let bar_colors = [
        0x00FF_FFFFu32, // branco
        0x00FF_FF00,    // amarelo
        0x0000_FFFF,    // ciano
        0x0000_FF00,    // verde
        0x00FF_00FF,    // magenta
        0x00FF_0000,    // vermelho
        0x0000_00FF,    // azul
        0x0000_0000,    // preto
    ];
    let bar_w = w / bar_colors.len() as u32;
    let bar_y = h * 2 / 3;
    let bar_h = h - bar_y - 40;
    for (i, &c) in bar_colors.iter().enumerate() {
        fill_rect(i as u32 * bar_w, bar_y, bar_w, bar_h, c);
    }

    // Borda branca de 4px (confere alinhamento das quatro arestas)
    fill_rect(0, 0, w, 4, 0x00FF_FFFF);
    fill_rect(0, h - 4, w, 4, 0x00FF_FFFF);
    fill_rect(0, 0, 4, h, 0x00FF_FFFF);
    fill_rect(w - 4, 0, 4, h, 0x00FF_FFFF);

    // Titulo
    draw_text(40, 60, "PULSAR OS", 0x00FF_FFFF, 6);
    draw_text(40, 130, "GUI STAGE 2 . 0", 0x0080_C0FF, 3);
    draw_text(40, 170, "LOADING", 0x0060_80A0, 2);
}
