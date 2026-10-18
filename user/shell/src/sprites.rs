//! Cache dos icones do dock (icone + sombra) como sprites pre-multiplicados.
//!
//! Os icones sao vetoriais: desenha-los (com a sombra) a cada quadro custava
//! mais que todo o resto do dock. Cada par (icone, tamanho) e renderizado
//! uma vez sobre preto e uma sobre branco; como todas as primitivas misturam
//! linearmente, a diferenca entre os dois da a cobertura (alpha) de cada
//! pixel e o render sobre preto e a cor ja multiplicada por ela. Compor o
//! sprite e entao `dst * (1 - a) + cor`, uma conta por pixel.
//!
//! O pool e um bump allocator: quando enche (muitos tamanhos diferentes com
//! a ampliacao do dock), e simplesmente esvaziado.
use plib::gfx::{darken, Fb, Rect};
use plib::icons::{self, Icon};

const POOL: usize = 2 * 1024 * 1024; // pixels (8 MiB)
const MAXSIDE: i32 = 160;
const MAXENT: usize = 512;

static mut PIX: [u32; POOL] = [0; POOL];
static mut WHITE: [u32; (MAXSIDE * MAXSIDE) as usize] = [0; (MAXSIDE * MAXSIDE) as usize];
static mut KEY: [u32; MAXENT] = [0; MAXENT];
static mut OFF: [u32; MAXENT] = [0; MAXENT];
static mut NENT: usize = 0;
static mut USED: usize = 0;

/// Geometria da sombra do icone no dock (a mesma de antes do cache).
fn shadow_of(s: i32) -> (i32, i32) {
    ((s / 8).max(3), s * 23 / 100) // (blur, raio)
}

fn render(fb: &Fb, icon: Icon, pad: i32, s: i32, bg: u32) {
    let (blur, rad) = shadow_of(s);
    fb.fill(fb.bounds(), bg);
    fb.shadow(Rect::new(pad, pad, s, s), rad, blur, 2, 60);
    icons::draw(fb, icon, pad, pad, s);
}

/// Desenha o icone `icon` de lado `r.w` em `r`, com a sombra do dock.
pub fn dock_icon(scr: &Fb, icon: Icon, r: Rect) {
    let s = r.w;
    let (blur, rad) = shadow_of(s);
    let pad = blur + 2;
    let side = s + 2 * pad;
    if side > MAXSIDE || s <= 0 {
        scr.shadow(r, rad, blur, 2, 60);
        return icons::draw(scr, icon, r.x, r.y, s);
    }
    let dst = Rect::new(r.x - pad, r.y - pad, side, side);
    let cr = dst.intersect(&scr.clip);
    if cr.is_empty() { return; }
    let key = (icon as u32) << 16 | s as u32;
    let n = (side * side) as usize;
    unsafe {
        let pool = &mut *(&raw mut PIX);
        let mut off = usize::MAX;
        for k in 0..NENT {
            if KEY[k] == key { off = OFF[k] as usize; break; }
        }
        if off == usize::MAX {
            if NENT == MAXENT || USED + n > POOL { NENT = 0; USED = 0; }
            off = USED;
            USED += n;
            KEY[NENT] = key;
            OFF[NENT] = off as u32;
            NENT += 1;
            let black = Fb::new(pool.as_mut_ptr().add(off), side, side, side);
            let white = Fb::new((&raw mut WHITE) as *mut u32, side, side, side);
            render(&black, icon, pad, s, 0x000000);
            render(&white, icon, pad, s, 0xFFFFFF);
            let w = &*(&raw const WHITE);
            for i in 0..n {
                let b = pool[off + i] & 0xFF_FFFF;
                let wh = w[i];
                // branco - preto = 255 * (1 - a) em cada canal
                let d = ((wh >> 16 & 255) + (wh >> 8 & 255) + (wh & 255))
                    .saturating_sub((b >> 16 & 255) + (b >> 8 & 255) + (b & 255));
                let a = 255 - (d / 3).min(255);
                pool[off + i] = a << 24 | b;
            }
        }
        let spr = &pool[off..off + n];
        for y in cr.y..cr.bottom() {
            let srow = &spr[((y - dst.y) * side) as usize..][..side as usize];
            let drow = scr.base.add((y * scr.stride) as usize);
            for x in cr.x..cr.right() {
                let p = srow[(x - dst.x) as usize];
                let a = p >> 24;
                if a == 0 { continue; }
                let d = drow.add(x as usize);
                let c = p & 0xFF_FFFF;
                if a == 255 { *d = c; continue; }
                // dst*(1-a) + cor pre-multiplicada, com saturacao por canal
                let q = darken(*d, a);
                let mut rb = (q & 0xFF00FF) + (c & 0xFF00FF);
                rb = (rb | ((rb & 0x0100_0100) >> 8) * 0xFF) & 0xFF00FF;
                let g = ((q & 0xFF00) + (c & 0xFF00)).min(0xFF00);
                *d = rb | g;
            }
        }
    }
}
