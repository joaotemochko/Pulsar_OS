//! Imagens do cursor de hardware (64x64 ARGB, desenhadas com AA por
//! superamostragem 4x4). O QEMU desenha o cursor por cima do scanout:
//! mover o mouse nao custa nenhuma recomposicao.

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Arrow,
    ResizeH,
    ResizeV,
    ResizeDiag,
    Hand,
}

const N: usize = 64;

/// Cobertura (0..16) de um poligono (coordenadas em 1/4 px) no pixel.
fn cover(poly: &[(i32, i32)], px: i32, py: i32) -> u32 {
    let mut n = 0;
    for sy in 0..4 {
        for sx in 0..4 {
            let (x, y) = (px * 4 + sx, py * 4 + sy);
            // par-impar com raio horizontal
            let mut inside = false;
            let mut j = poly.len() - 1;
            for i in 0..poly.len() {
                let (xi, yi) = poly[i];
                let (xj, yj) = poly[j];
                if (yi > y) != (yj > y) {
                    let dy = yj - yi;
                    let lhs = (x - xi) * dy;
                    let rhs = (xj - xi) * (y - yi);
                    if (dy > 0 && lhs < rhs) || (dy < 0 && lhs > rhs) {
                        inside = !inside;
                    }
                }
                j = i;
            }
            if inside { n += 1; }
        }
    }
    n
}

fn paint(img: &mut [u32; N * N], poly: &[(i32, i32)], rgb: u32) {
    let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, 0, 0);
    for &(x, y) in poly {
        x0 = x0.min(x / 4); y0 = y0.min(y / 4);
        x1 = x1.max(x / 4 + 1); y1 = y1.max(y / 4 + 1);
    }
    for py in y0.max(0)..y1.min(N as i32) {
        for px in x0.max(0)..x1.min(N as i32) {
            let c = cover(poly, px, py);
            if c == 0 { continue; }
            let a = (c * 255 / 16).min(255);
            let p = &mut img[py as usize * N + px as usize];
            let ba = *p >> 24;
            // "over" em ARGB nao pre-multiplicado
            let oa = a + ba * (255 - a) / 255;
            if oa == 0 { continue; }
            let ch = |s: u32| -> u32 {
                let f = (rgb >> s) & 0xFF;
                let b = (*p >> s) & 0xFF;
                (f * a + b * ba * (255 - a) / 255) / oa
            };
            *p = (oa << 24) | (ch(16) << 16) | (ch(8) << 8) | ch(0);
        }
    }
}

fn scale(poly: &[(i32, i32)], out: &mut [(i32, i32); 16], ox: i32, oy: i32) -> usize {
    for (i, &(x, y)) in poly.iter().enumerate() {
        out[i] = (x + ox * 4, y + oy * 4);
    }
    poly.len()
}

/// Gera a imagem e o hotspot do cursor.
pub fn make(shape: Shape, img: &mut [u32; N * N]) -> (u32, u32) {
    img.fill(0);
    let mut buf = [(0, 0); 16];
    match shape {
        Shape::Arrow | Shape::Hand => {
            // seta classica (1/4 px), contorno preto e miolo branco
            const OUT: [(i32, i32); 7] = [(0, 0), (0, 74), (17, 58), (29, 86), (42, 80), (30, 53), (53, 53)];
            const IN: [(i32, i32); 7] = [(5, 12), (5, 61), (18, 48), (31, 77), (36, 75), (23, 46), (41, 46)];
            // sombra suave
            let n = scale(&OUT, &mut buf, 2, 3);
            paint(img, &buf[..n], 0x000000);
            for p in img.iter_mut() {
                let a = (*p >> 24) * 70 / 255;
                *p = a << 24;
            }
            let n = scale(&OUT, &mut buf, 1, 1);
            paint(img, &buf[..n], 0x000000);
            let n = scale(&IN, &mut buf, 1, 1);
            paint(img, &buf[..n], 0xFFFFFF);
            (1, 1)
        }
        Shape::ResizeH | Shape::ResizeV | Shape::ResizeDiag => {
            // seta dupla: contorno branco, miolo preto, centrada em (16,16)
            let arrow = |img: &mut [u32; N * N], grow: i32, rgb: u32| {
                let (t, hh, tip) = (8 + grow / 2, 20 + grow, 44 + grow);
                let poly: [(i32, i32); 10] = [
                    (-tip, 0), (-24, -hh), (-24, -t), (24, -t), (24, -hh),
                    (tip, 0), (24, hh), (24, t), (-24, t), (-24, hh),
                ];
                let mut out = [(0, 0); 16];
                for (i, &(x, y)) in poly.iter().enumerate() {
                    let (rx, ry) = match shape {
                        Shape::ResizeH => (x, y),
                        Shape::ResizeV => (y, x),
                        _ => ((x - y) * 7 / 10, (x + y) * 7 / 10),
                    };
                    out[i] = (rx + 64, ry + 64);
                }
                paint(img, &out[..10], rgb);
            };
            arrow(img, 6, 0xFFFFFF);
            arrow(img, 0, 0x000000);
            (16, 16)
        }
    }
}
