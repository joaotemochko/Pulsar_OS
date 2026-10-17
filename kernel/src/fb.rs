//! Regiao grafica compartilhada + primitivas de desenho do kernel (splash).
//!
//! Layout (64 MiB a partir de FB_PA; o userspace a ve em FB_UVA):
//!   +0      back buffer: e o backing do recurso da GPU. O compositor
//!           desenha aqui e o present faz TRANSFER_TO_HOST do retangulo
//!           alterado (a copia e um snapshot: nao ha tearing e nao existe
//!           mais um "front buffer" a ser copiado a cada quadro).
//!   +8 MiB  wallpaper cacheado (WM)
//!   +16 MiB wallpaper desfocado (vidro fosco da barra de menus e do dock)
//!   +24 MiB superficies dos apps: SURF_SLOTS x 4 MiB, stride SURF_W
//! Pixels: u32 0x00RRGGBB (formato B8G8R8X8 do virtio-gpu).
use crate::arch::platform::{FB_PA, FB_REGION_BYTES, FB_UVA};
use core::ptr::write_volatile;

pub const BACK_OFFSET: u64 = 0;
// (+8 MiB e +16 MiB: wallpapers, usados so pelo compositor em userspace)
pub const SURF_OFFSET: u64 = 24 * 1024 * 1024;
pub const SURF_SLOT_BYTES: u64 = 4 * 1024 * 1024;
pub const SURF_SLOTS: u64 = 8;
/// Superficie: stride de 1280 px, ate 816 linhas (cabe em 4 MiB).
pub const SURF_W: u64 = 1280;
pub const SURF_H: u64 = 816;
/// Maior tela suportada pelo back buffer de 8 MiB.
pub const MAX_FB_BYTES: u64 = 8 * 1024 * 1024;

const _: () = assert!(SURF_OFFSET + SURF_SLOTS * SURF_SLOT_BYTES <= FB_REGION_BYTES);

/// Endereco fisico do back buffer (backing da GPU e alvo do splash).
pub const BACK_PA: u64 = FB_PA + BACK_OFFSET;

/// Endereco (de usuario) da superficie do slot `n`.
pub const fn surf_uva(n: u64) -> u64 { FB_UVA + SURF_OFFSET + n * SURF_SLOT_BYTES }

static mut WIDTH: u32 = 0;
static mut HEIGHT: u32 = 0;

pub fn init(width: u32, height: u32) {
    unsafe {
        WIDTH = width;
        HEIGHT = height;
    }
}

#[inline]
pub fn size() -> (u32, u32) {
    unsafe { (WIDTH, HEIGHT) }
}

fn map_user(off: u64, len: u64) {
    let root = crate::process::current_root();
    let mut o = off;
    while o < off + len {
        unsafe {
            crate::arch::mmu::map_page_in(root, FB_UVA + o, FB_PA + o,
                crate::arch::mmu::PageFlags::user_shared());
        }
        o += 4096;
    }
    crate::arch::mmu::flush_tlb();
}

/// SYS_FB_MAP: mapeia toda a regiao grafica no processo corrente (o
/// compositor precisa do back buffer, dos wallpapers e de todas as
/// superficies). Retorna o VA do back buffer.
pub fn map_for_user() -> u64 {
    map_user(0, SURF_OFFSET + SURF_SLOTS * SURF_SLOT_BYTES);
    FB_UVA + BACK_OFFSET
}

/// SYS_SURF_MAP: mapeia so a superficie do slot `n` (apps).
pub fn map_surface_for_user(n: u64) -> u64 {
    map_user(SURF_OFFSET + n * SURF_SLOT_BYTES, SURF_SLOT_BYTES);
    surf_uva(n)
}

#[inline]
fn ptr_at(x: u32, y: u32) -> *mut u32 {
    (BACK_PA + ((y as u64 * unsafe { WIDTH } as u64 + x as u64) * 4)) as *mut u32
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

fn blend_px(x: u32, y: u32, c: u32, a: u32) {
    let (w, h) = size();
    if x >= w || y >= h || a == 0 { return; }
    let p = ptr_at(x, y);
    let bg = unsafe { *p };
    let inv = 255 - a;
    let ch = |s: u32| (((c >> s) & 0xFF) * a + ((bg >> s) & 0xFF) * inv) / 255;
    unsafe { *p = (ch(16) << 16) | (ch(8) << 8) | ch(0) };
}

fn isqrt(n: u32) -> u32 {
    if n == 0 { return 0; }
    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x { x = y; y = (x + n / x) / 2; }
    x
}

/// Anel anti-aliased (logo do splash).
fn ring(cx: i32, cy: i32, r: i32, thick: i32, color: u32) {
    for dy in -r - 1..=r + 1 {
        for dx in -r - 1..=r + 1 {
            let d16 = isqrt(((dx * dx + dy * dy) * 256) as u32) as i32; // dist * 16
            let outer = r * 16 - d16;          // >0 dentro do raio externo
            let inner = d16 - (r - thick) * 16; // >0 fora do raio interno
            let cov = outer.min(inner).clamp(0, 16);
            if cov > 0 {
                blend_px((cx + dx) as u32, (cy + dy) as u32, color, (cov * 255 / 16) as u32);
            }
        }
    }
}

fn glyph(c: u8) -> [u8; 8] {
    match c {
        b'P' => [0xFC, 0xC6, 0xC6, 0xFC, 0xC0, 0xC0, 0xC0, 0x00],
        b'U' => [0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x7C, 0x00],
        b'L' => [0xC0, 0xC0, 0xC0, 0xC0, 0xC0, 0xC0, 0xFE, 0x00],
        b'S' => [0x7E, 0xC0, 0xC0, 0x7C, 0x06, 0x06, 0xFC, 0x00],
        b'A' => [0x38, 0x6C, 0xC6, 0xC6, 0xFE, 0xC6, 0xC6, 0x00],
        b'R' => [0xFC, 0xC6, 0xC6, 0xFC, 0xD8, 0xCC, 0xC6, 0x00],
        b'O' => [0x7C, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x7C, 0x00],
        _ => [0; 8],
    }
}

fn draw_text(x: u32, y: u32, text: &str, color: u32, scale: u32) {
    let mut cx = x;
    for &c in text.as_bytes() {
        let g = glyph(c);
        for (row, bits) in g.iter().enumerate() {
            for col in 0..8u32 {
                if bits & (0x80 >> col) != 0 {
                    fill_rect(cx + col * scale, y + row as u32 * scale, scale, scale, color);
                }
            }
        }
        cx += 9 * scale;
    }
}

/// Tela de boot: fundo escuro, anel do Pulsar e o nome do sistema.
pub fn draw_splash() {
    let (w, h) = size();
    for y in 0..h {
        let t = y * 24 / h.max(1);
        fill_rect(0, y, w, 1, ((8 + t) << 16) | ((10 + t) << 8) | (18 + t * 2));
    }
    let cx = (w / 2) as i32;
    let cy = (h / 2) as i32 - 40;
    ring(cx, cy, 46, 5, 0x0060A8FF);
    ring(cx, cy, 30, 3, 0x00A0C8FF);
    ring(cx, cy, 10, 10, 0x00FFFFFF);
    let title = "PULSAR OS";
    let tw = title.len() as u32 * 9 * 3;
    draw_text(w / 2 - tw / 2, (cy + 80) as u32, title, 0x00E8F0FF, 3);
}
