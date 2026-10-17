//! Papeis de parede procedurais e o "vidro fosco" (versao desfocada do
//! papel de parede usada pela barra de menus, dock e paineis).
use plib::gfx::{isqrt, mix, sin256, Fb};

#[inline]
fn hash(x: i32, y: i32) -> u32 {
    let mut h = (x as u32).wrapping_mul(374_761_393) ^ (y as u32).wrapping_mul(668_265_263);
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    h ^ (h >> 16)
}

/// Brilho radial suave: 0..=256 dentro do raio, 0 fora.
#[inline]
fn glow(x: i32, y: i32, cx: i32, cy: i32, r: i32) -> u32 {
    let (dx, dy) = ((x - cx) as i64, (y - cy) as i64);
    let d2 = dx * dx + dy * dy;
    let r2 = (r as i64 * r as i64).max(1);
    if d2 >= r2 { return 0; }
    let t = (256 - d2 * 256 / r2) as u32;
    (t * t) >> 8
}

#[inline]
fn add(c: u32, col: u32, g: u32, strength: u32) -> u32 {
    if g == 0 { c } else { mix(c, col, (g * strength) >> 8) }
}

#[inline]
fn vgrad3(t: u32, a: u32, b: u32, c: u32) -> u32 {
    if t < 128 { mix(a, b, t * 2) } else { mix(b, c, (t - 128) * 2) }
}

/// Ruido de +-1 por canal: elimina bandas nos gradientes.
#[inline]
fn dither(c: u32, x: i32, y: i32) -> u32 {
    let n = (hash(x, y) >> 7) % 3;
    let add = |v: u32| -> u32 { (v + n).saturating_sub(1).min(255) };
    (add((c >> 16) & 0xFF) << 16) | (add((c >> 8) & 0xFF) << 8) | add(c & 0xFF)
}

/// Altura (em px) de uma onda com fase e frequencia dadas.
#[inline]
fn wave(x: i32, w: i32, freq: i32, phase: i32, amp: i32) -> i32 {
    amp * sin256(x * freq * 256 / w.max(1) + phase) / 1024
}

/// Cor do papel de parede `kind` no ponto (x,y) de uma tela w x h.
pub fn color_at(kind: u8, x: i32, y: i32, w: i32, h: i32) -> u32 {
    let ty = (y * 255 / h.max(1)) as u32;
    let c = match kind {
        // Aurora: azul profundo com luzes frias e violeta
        0 => {
            let mut c = mix(0x0A1530, 0x1C3366, ty);
            c = add(c, 0x3D7BFA, glow(x, y, w * 30 / 100, h * 22 / 100, w * 55 / 100), 170);
            c = add(c, 0x8B5CF6, glow(x, y, w * 82 / 100, h * 80 / 100, w * 50 / 100), 140);
            c = add(c, 0x22D3EE, glow(x, y, w * 66 / 100, h * 12 / 100, w * 34 / 100), 90);
            c = add(c, 0xF472B6, glow(x, y, w * 10 / 100, h * 95 / 100, w * 30 / 100), 60);
            c
        }
        // Ondas: camadas onduladas roxo -> coral -> pessego (estilo macOS)
        1 | 2 => {
            let (sky_top, sky_bot, cols): (u32, u32, [u32; 5]) = if kind == 1 {
                (0x1B1035, 0x5B2A86, [0x6D2E8F, 0x9B3C8F, 0xD0567E, 0xF2846A, 0xFFB38A])
            } else {
                (0x041530, 0x0B4F8A, [0x0D5FA8, 0x1479B8, 0x1C9CC4, 0x3CC3C8, 0x8FE3D2])
            };
            let mut c = mix(sky_top, sky_bot, ty);
            let freqs = [1, 2, 1, 3, 2];
            for k in 0..5 {
                let base = h * (34 + k as i32 * 13) / 100;
                let edge = base + wave(x, w, freqs[k], k as i32 * 53, h * (7 - k as i32) / 100)
                    + wave(x, w, freqs[k] * 3, k as i32 * 91 + 40, h / 60);
                if y >= edge {
                    let depth = ((y - edge) * 255 / (h / 3).max(1)).min(255) as u32;
                    c = mix(cols[k], plib::gfx::shade(cols[k], -90), depth);
                    // brilho fino na crista
                    if y - edge < 3 { c = mix(c, 0xFFFFFF, 60 - (y - edge) as u32 * 20); }
                }
            }
            c
        }
        // Por do sol: ceu em degrade, sol e montanhas em camadas
        3 => {
            let mut c = vgrad3(ty.min(255) * 255 / 170, 0x1E1B4B, 0xB4235A, 0xFDB474);
            if ty > 170 { c = 0xFDB474; }
            let (sx, sy, sr) = (w * 50 / 100, h * 60 / 100, w * 8 / 100);
            c = add(c, 0xFFD9A0, glow(x, y, sx, sy, sr * 5), 120);
            let d = isqrt(((x - sx) * (x - sx) + (y - sy) * (y - sy)) as u32) as i32;
            if d < sr { c = mix(0xFFF1D6, 0xFFC27A, (d * 255 / sr.max(1)) as u32); }
            else if d < sr + 2 { c = mix(c, 0xFFE2B0, 128); }
            let layers = [(66, 0x6B2A6F, 1, 9), (74, 0x45195A, 2, 7), (84, 0x260E3B, 3, 5)];
            for (k, &(base, col, f, amp)) in layers.iter().enumerate() {
                let ridge = h * base / 100 + wave(x, w, f, k as i32 * 77, h * amp / 100)
                    + wave(x, w, f * 4 + 1, k as i32 * 31, h / 70);
                if y >= ridge {
                    c = mix(col, 0x0B0614, ((y - ridge) * 255 / (h / 2).max(1)).min(200) as u32);
                }
            }
            c
        }
        // Grafite: escuro e sobrio, com luz difusa
        4 => {
            let mut c = mix(0x30333A, 0x131418, ty);
            c = add(c, 0x5B6474, glow(x, y, w * 18 / 100, -h / 10, w * 70 / 100), 110);
            c = add(c, 0x3A4F6B, glow(x, y, w * 95 / 100, h * 110 / 100, w * 50 / 100), 90);
            c
        }
        // Nebulosa: espaco profundo, nuvens coloridas, estrelas e um pulsar
        _ => {
            let mut c = mix(0x04050E, 0x0A0B1F, ty);
            c = add(c, 0x7C3AED, glow(x, y, w * 25 / 100, h * 35 / 100, w * 40 / 100), 120);
            c = add(c, 0xDB2777, glow(x, y, w * 40 / 100, h * 60 / 100, w * 25 / 100), 90);
            c = add(c, 0x2563EB, glow(x, y, w * 78 / 100, h * 70 / 100, w * 38 / 100), 110);
            let hs = hash(x, y);
            if hs % 700 == 0 { c = mix(c, 0xFFFFFF, 120 + (hs >> 24) % 136); }
            let (px, py) = (w * 70 / 100, h * 32 / 100);
            c = add(c, 0xBFE3FF, glow(x, y, px, py, w * 6 / 100), 255);
            c = add(c, 0xFFFFFF, glow(x, y, px, py, w / 90), 255);
            let (dx, dy) = ((x - px).abs(), (y - py).abs());
            if dy < 2 && dx < w * 18 / 100 { c = add(c, 0xCFE8FF, 256 - (dx * 256 / (w * 18 / 100)) as u32, 160 - dy as u32 * 60); }
            if dx < 2 && dy < h * 22 / 100 { c = add(c, 0xCFE8FF, 256 - (dy * 256 / (h * 22 / 100)) as u32, 160 - dx as u32 * 60); }
            c
        }
    };
    dither(c, x, y)
}

/// Desenha o papel de parede em `fb` (tela inteira).
pub fn render(kind: u8, fb: &Fb) {
    for y in 0..fb.h {
        let row = unsafe { fb.base.add((y * fb.stride) as usize) };
        for x in 0..fb.w {
            unsafe { *row.add(x as usize) = color_at(kind, x, y, fb.w, fb.h) };
        }
    }
}

/// Miniatura (para Configuracoes/Central de Controle).
pub fn render_thumb(kind: u8, out: &mut [u32], tw: i32, th: i32, w: i32, h: i32) {
    for y in 0..th {
        for x in 0..tw {
            out[(y * tw + x) as usize] = color_at(kind, x * w / tw, y * h / th, w, h);
        }
    }
}

const DS: i32 = 4; // fator de reducao do desfoque
static mut SMALL: [u32; 400 * 300] = [0; 400 * 300];
static mut TMP: [u32; 1024] = [0; 1024];

/// Gera `blur` = versao fortemente desfocada de `wall`: reduz 4x (media),
/// desfoca a imagem pequena e amplia com interpolacao bilinear.
pub fn make_blur(wall: &Fb, blur: &Fb) {
    let (sw, sh) = ((wall.w / DS).min(400), (wall.h / DS).min(300));
    let small = unsafe { &mut *(&raw mut SMALL) };
    for sy in 0..sh {
        for sx in 0..sw {
            let (mut r, mut g, mut b) = (0u32, 0u32, 0u32);
            for dy in 0..DS {
                for dx in 0..DS {
                    let c = wall.get(sx * DS + dx, sy * DS + dy);
                    r += (c >> 16) & 0xFF; g += (c >> 8) & 0xFF; b += c & 0xFF;
                }
            }
            let n = (DS * DS) as u32;
            small[(sy * sw + sx) as usize] = ((r / n) << 16) | ((g / n) << 8) | (b / n);
        }
    }
    let sfb = Fb::new(small.as_mut_ptr(), sw, sh, sw);
    sfb.box_blur(sfb.bounds(), 6, 3, unsafe { &mut *(&raw mut TMP) });
    // ampliacao bilinear (ponto fixo 8.8)
    for y in 0..blur.h {
        let fy = ((y * 256) / DS - 128).clamp(0, (sh - 1) * 256);
        let (y0, ty) = (fy >> 8, (fy & 255) as u32);
        let y1 = (y0 + 1).min(sh - 1);
        let row = unsafe { blur.base.add((y * blur.stride) as usize) };
        for x in 0..blur.w {
            let fx = ((x * 256) / DS - 128).clamp(0, (sw - 1) * 256);
            let (x0, tx) = (fx >> 8, (fx & 255) as u32);
            let x1 = (x0 + 1).min(sw - 1);
            let a = mix(small[(y0 * sw + x0) as usize], small[(y0 * sw + x1) as usize], tx);
            let b = mix(small[(y1 * sw + x0) as usize], small[(y1 * sw + x1) as usize], tx);
            unsafe { *row.add(x as usize) = dither(mix(a, b, ty), x, y) };
        }
    }
}
