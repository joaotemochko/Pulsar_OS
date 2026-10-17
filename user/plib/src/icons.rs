//! Icones vetoriais desenhados proceduralmente em qualquer tamanho (o dock
//! amplia os icones sob o mouse sem perder nitidez).
use crate::gfx::{cos256, shade, sin256, Fb, Rect};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Files,
    Terminal,
    Editor,
    Calc,
    Monitor,
    Network,
    Browser,
    Ball,
    Settings,
    Pulsar,
    Document,
    Config,
    Program,
    Help,
}

/// Base "squircle" com gradiente, brilho no topo e borda sutil.
fn base(fb: &Fb, x: i32, y: i32, s: i32, top: u32, bot: u32) {
    let r = Rect::new(x, y, s, s);
    let rad = (s * 23 / 100).max(3);
    fb.round_rect_grad(r, rad, top, bot, 255);
    fb.round_border(r, rad, 0xFFFFFF, 40);
}

/// Atalho: coordenada em percentuais do icone, em 1/16 px.
struct P { x: i32, y: i32, s: i32 }
impl P {
    fn x(&self, pct: i32) -> i32 { self.x * 16 + self.s * 16 * pct / 100 }
    fn y(&self, pct: i32) -> i32 { self.y * 16 + self.s * 16 * pct / 100 }
    fn l(&self, pct: i32) -> i32 { (self.s * 16 * pct / 100).max(16) }
    fn px(&self, pct: i32) -> i32 { self.x + self.s * pct / 100 }
    fn py(&self, pct: i32) -> i32 { self.y + self.s * pct / 100 }
    fn pl(&self, pct: i32) -> i32 { self.s * pct / 100 }
}

pub fn draw(fb: &Fb, icon: Icon, x: i32, y: i32, s: i32) {
    let p = P { x, y, s };
    match icon {
        Icon::Files => {
            base(fb, x, y, s, 0x6AC8FF, 0x1A73E8);
            fb.round_rect_a(Rect::new(p.px(17), p.py(27), p.pl(30), p.pl(16)), (s / 24).max(1), 0xFFFFFF, 190);
            fb.round_rect_grad(Rect::new(p.px(15), p.py(34), p.pl(70), p.pl(44)), (s / 14).max(2),
                0xFFFFFF, 0xDCEBFF, 255);
            fb.hline(p.px(15) + 2, p.py(44), p.pl(70) - 4, 0x8AB8F0, 120);
        }
        Icon::Terminal => {
            base(fb, x, y, s, 0x4A4A52, 0x141418);
            let w = p.l(7);
            fb.line16(p.x(26), p.y(36), p.x(43), p.y(50), w, 0xFFFFFF, 255);
            fb.line16(p.x(43), p.y(50), p.x(26), p.y(64), w, 0xFFFFFF, 255);
            fb.line16(p.x(50), p.y(66), p.x(72), p.y(66), w, 0x8EF0A0, 255);
        }
        Icon::Editor => {
            base(fb, x, y, s, 0xFFFFFF, 0xE6E6EB);
            for k in 0..4 {
                let yy = p.y(30 + k * 12);
                fb.line16(p.x(22), yy, p.x(if k == 3 { 54 } else { 72 }), yy, p.l(3), 0xC2C2C8, 255);
            }
            // lapis
            fb.line16(p.x(46), p.y(76), p.x(80), p.y(28), p.l(12), 0xFFB300, 255);
            fb.line16(p.x(74), p.y(36), p.x(80), p.y(28), p.l(12), 0xFF6F91, 255);
            fb.line16(p.x(42), p.y(82), p.x(47), p.y(75), p.l(6), 0x3A3A3C, 255);
        }
        Icon::Calc => {
            base(fb, x, y, s, 0x5C5C63, 0x2A2A2F);
            fb.round_rect(Rect::new(p.px(18), p.py(15), p.pl(64), p.pl(17)), (s / 20).max(1), 0x1A1A1D);
            fb.line16(p.x(58), p.y(23), p.x(74), p.y(23), p.l(4), 0xF2F2F7, 255);
            let bs = p.pl(13).max(2);
            for row in 0..3 {
                for col in 0..4 {
                    let c = if col == 3 { 0xFF9F0A } else if row == 0 { 0xA5A5AB } else { 0xE5E5EA };
                    let bx = p.px(18) + col * (p.pl(64) - bs) / 3;
                    let by = p.py(40) + row * p.pl(16);
                    fb.round_rect(Rect::new(bx, by, bs, bs), bs / 3, c);
                }
            }
        }
        Icon::Monitor => {
            base(fb, x, y, s, 0x34343A, 0x0E0E11);
            for k in 1..4 {
                fb.hline(p.px(14), p.py(20 + k * 16), p.pl(72), 0xFFFFFF, 28);
            }
            let pts = [(16, 70), (30, 58), (42, 64), (56, 38), (68, 48), (84, 24)];
            for i in 0..pts.len() - 1 {
                fb.line16(p.x(pts[i].0), p.y(pts[i].1), p.x(pts[i + 1].0), p.y(pts[i + 1].1),
                    p.l(5), 0x30D158, 255);
            }
            fb.disc16(p.x(84), p.y(24), p.l(5), 0xB4FFC4, 255);
            for (k, h) in [18, 30, 22, 40].iter().enumerate() {
                let bx = p.px(18 + k as i32 * 9);
                fb.fill_a(Rect::new(bx, p.py(84) - p.pl(*h) / 2, p.pl(5), p.pl(*h) / 2), 0x64D2FF, 200);
            }
        }
        Icon::Network => {
            base(fb, x, y, s, 0x70D7FF, 0x0A6CFF);
            let (cx, cy, r) = (p.x(50), p.y(50), p.l(31));
            let w = p.l(4);
            fb.ring16(cx, cy, r, w, 0xFFFFFF, 255);
            // meridiano (elipse) e paralelos
            let rx = r * 45 / 100;
            let mut prev = (cx, cy - r);
            for k in 1..=32 {
                let a = k * 8;
                let q = (cx + rx * sin256(a) / 1024, cy - r * cos256(a) / 1024);
                fb.line16(prev.0, prev.1, q.0, q.1, w, 0xFFFFFF, 230);
                prev = q;
            }
            for dy in [-(r * 45 / 100), 0, r * 45 / 100] {
                let half = crate::gfx::isqrt(((r * r) - dy * dy).max(0) as u32) as i32 - w / 2;
                fb.line16(cx - half, cy + dy, cx + half, cy + dy, w, 0xFFFFFF, 230);
            }
        }
        Icon::Browser => {
            base(fb, x, y, s, 0xFFFFFF, 0xDCEAFF);
            let (cx, cy) = (p.px(50), p.py(50));
            fb.disc(cx, cy, p.pl(35), 0x1A84FF, 255);
            fb.disc(cx, cy, p.pl(30), 0xFFFFFF, 255);
            for k in 0..8 {
                let a = k * 32;
                let (r0, r1) = (p.l(25), p.l(29));
                fb.line16(cx * 16 + r0 * sin256(a) / 1024, cy * 16 - r0 * cos256(a) / 1024,
                    cx * 16 + r1 * sin256(a) / 1024, cy * 16 - r1 * cos256(a) / 1024, p.l(2), 0x8E8E93, 255);
            }
            let d = p.pl(20);
            let w = p.pl(6);
            fb.triangle([(cx + d, cy - d), (cx - w, cy - w), (cx + w, cy + w)], 0xFF3B30, 255);
            fb.triangle([(cx - d, cy + d), (cx - w, cy - w), (cx + w, cy + w)], 0xB8B8BE, 255);
            fb.disc(cx, cy, p.pl(4).max(1), 0x3A3A3C, 255);
        }
        Icon::Ball => {
            base(fb, x, y, s, 0xC86CF5, 0x5856D6);
            fb.disc16(p.x(50), p.y(84), p.l(20), 0x000000, 50);
            fb.disc16(p.x(50), p.y(50), p.l(27), 0xFF7A1A, 255);
            fb.disc16(p.x(45), p.y(45), p.l(19), 0xFFB13B, 200);
            fb.disc16(p.x(40), p.y(39), p.l(7), 0xFFFFFF, 210);
        }
        Icon::Settings => {
            base(fb, x, y, s, 0xA8A8AE, 0x5A5A60);
            gear(fb, p.x(50), p.y(50), p.l(33), 0xEDEDF0, 0x7E7E85);
        }
        Icon::Pulsar => {
            base(fb, x, y, s, 0x24365E, 0x0A0F1F);
            logo(fb, p.x(50), p.y(50), p.l(32), 0x8CC8FF);
        }
        Icon::Help => {
            base(fb, x, y, s, 0x7FD6FF, 0x2D7FF9);
            fb.ring16(p.x(50), p.y(50), p.l(30), p.l(6), 0xFFFFFF, 255);
            fb.line16(p.x(50), p.y(56), p.x(50), p.y(60), p.l(8), 0xFFFFFF, 255);
            fb.disc16(p.x(50), p.y(70), p.l(5), 0xFFFFFF, 255);
            fb.line16(p.x(42), p.y(40), p.x(50), p.y(34), p.l(7), 0xFFFFFF, 255);
            fb.line16(p.x(50), p.y(34), p.x(58), p.y(42), p.l(7), 0xFFFFFF, 255);
            fb.line16(p.x(58), p.y(42), p.x(50), p.y(54), p.l(7), 0xFFFFFF, 255);
        }
        Icon::Document | Icon::Config => {
            let page = Rect::new(p.px(20), p.py(8), p.pl(60), p.pl(84));
            fb.shadow(page, 3, (s / 16).max(2), 1, 50);
            fb.round_rect_grad(page, (s / 18).max(1), 0xFFFFFF, 0xF1F1F4, 255);
            fb.round_border(page, (s / 18).max(1), 0x000000, 40);
            let fold = p.pl(16);
            fb.triangle([(page.right() - fold, page.y), (page.right(), page.y + fold),
                (page.right() - fold, page.y + fold)], 0xD8D8DE, 255);
            if icon == Icon::Document {
                for k in 0..5 {
                    let yy = p.y(36 + k * 10);
                    fb.line16(p.x(30), yy, p.x(if k == 4 { 54 } else { 70 }), yy, p.l(3), 0xB0B0B8, 255);
                }
            } else {
                gear(fb, p.x(50), p.y(56), p.l(18), 0x8E8E93, 0xFFFFFF);
            }
        }
        Icon::Program => {
            base(fb, x, y, s, 0x4B4B55, 0x1B1B20);
            logo(fb, p.x(50), p.y(50), p.l(28), 0xFFFFFF);
        }
    }
}

/// Engrenagem centrada em (cx,cy), raio externo r (1/16 px).
pub fn gear(fb: &Fb, cx: i32, cy: i32, r: i32, color: u32, hole: u32) {
    let tooth = r * 34 / 100;
    for k in 0..8 {
        let a = k * 32;
        let (r0, r1) = (r * 55 / 100, r - tooth / 2);
        fb.line16(cx + r0 * sin256(a) / 1024, cy - r0 * cos256(a) / 1024,
            cx + r1 * sin256(a) / 1024, cy - r1 * cos256(a) / 1024, tooth, color, 255);
    }
    fb.disc16(cx, cy, r * 72 / 100, color, 255);
    fb.disc16(cx, cy, r * 30 / 100, hole, 255);
}

/// Logo do Pulsar: anel luminoso cortado por um batimento (pulso).
pub fn logo(fb: &Fb, cx: i32, cy: i32, r: i32, color: u32) {
    fb.disc16(cx, cy, r + r / 4, color, 30);
    fb.ring16(cx, cy, r, (r / 6).max(16), color, 255);
    let span = r * 70 / 100;
    let pts = [(-span, 0), (-span / 2, 0), (-span / 4, -r / 6), (0, -r / 2), (span / 6, r / 3),
        (span / 2, 0), (span, 0)];
    let w = (r / 7).max(16);
    for i in 0..pts.len() - 1 {
        fb.line16(cx + pts[i].0, cy + pts[i].1, cx + pts[i + 1].0, cy + pts[i + 1].1, w, 0xFFFFFF, 255);
    }
}

// ---------------------------------------------------------------- simbolos
// Glifos monocromaticos pequenos (barra de menus, barras de ferramentas).

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Sym {
    Search,
    Toggles,
    Network,
    Cpu,
    Power,
    ChevronLeft,
    ChevronRight,
    ChevronDown,
    List,
    Grid,
    Close,
    Minus,
    Plus,
    Check,
    Moon,
    Sun,
    Image,
    Dock,
    Info,
    Keyboard,
    Disk,
    Folder,
    Apps,
    Doc,
    Clock,
}

/// Desenha um simbolo de tamanho `s` (px) com canto superior em (x,y).
pub fn sym(fb: &Fb, sym: Sym, x: i32, y: i32, s: i32, c: u32) {
    let p = P { x, y, s };
    let w = p.l(11);
    match sym {
        Sym::Search => {
            fb.ring16(p.x(42), p.y(42), p.l(30), w, c, 255);
            fb.line16(p.x(64), p.y(64), p.x(86), p.y(86), w + 4, c, 255);
        }
        Sym::Toggles => {
            for (k, on) in [(0, false), (1, true)] {
                let yy = 26 + k * 46;
                fb.round_rect_a(Rect::new(p.px(8), p.py(yy - 16), p.pl(84), p.pl(32)), s / 6, c, 255);
                let kx = if on { 70 } else { 28 };
                fb.disc16(p.x(kx), p.y(yy), p.l(11), shade(c, if c > 0x808080 { -200 } else { 200 }), 255);
            }
        }
        Sym::Network => {
            for (k, r) in [(0, 46), (1, 30)] {
                let _ = k;
                for a in -24..=24 {
                    let rr = p.l(r);
                    fb.disc16(p.x(50) + rr * sin256(a) / 1024, p.y(82) - rr * cos256(a) / 1024, w / 2, c, 160);
                }
            }
            fb.disc16(p.x(50), p.y(78), p.l(10), c, 255);
        }
        Sym::Cpu => {
            let r = Rect::new(p.px(22), p.py(22), p.pl(56), p.pl(56));
            fb.round_border(r, s / 8, c, 255);
            fb.fill(Rect::new(p.px(38), p.py(38), p.pl(24), p.pl(24)), c);
            for k in 0..3 {
                let o = 32 + k * 18;
                fb.line16(p.x(o), p.y(8), p.x(o), p.y(20), w / 2 + 4, c, 255);
                fb.line16(p.x(o), p.y(80), p.x(o), p.y(92), w / 2 + 4, c, 255);
                fb.line16(p.x(8), p.y(o), p.x(20), p.y(o), w / 2 + 4, c, 255);
                fb.line16(p.x(80), p.y(o), p.x(92), p.y(o), w / 2 + 4, c, 255);
            }
        }
        Sym::Power => {
            for a in 24..=232 {
                if a % 4 != 0 { continue; }
                let rr = p.l(34);
                fb.disc16(p.x(50) + rr * sin256(a) / 1024, p.y(54) - rr * cos256(a) / 1024, w / 2, c, 255);
            }
            fb.line16(p.x(50), p.y(10), p.x(50), p.y(48), w, c, 255);
        }
        Sym::ChevronLeft => {
            fb.line16(p.x(62), p.y(20), p.x(34), p.y(50), w, c, 255);
            fb.line16(p.x(34), p.y(50), p.x(62), p.y(80), w, c, 255);
        }
        Sym::ChevronRight => {
            fb.line16(p.x(38), p.y(20), p.x(66), p.y(50), w, c, 255);
            fb.line16(p.x(66), p.y(50), p.x(38), p.y(80), w, c, 255);
        }
        Sym::ChevronDown => {
            fb.line16(p.x(20), p.y(38), p.x(50), p.y(66), w, c, 255);
            fb.line16(p.x(50), p.y(66), p.x(80), p.y(38), w, c, 255);
        }
        Sym::List => {
            for k in 0..3 {
                let yy = 22 + k * 28;
                fb.disc16(p.x(14), p.y(yy), p.l(8), c, 255);
                fb.line16(p.x(32), p.y(yy), p.x(88), p.y(yy), w, c, 255);
            }
        }
        Sym::Grid => {
            for k in 0..4 {
                let (gx, gy) = (10 + (k % 2) * 46, 10 + (k / 2) * 46);
                fb.round_rect(Rect::new(p.px(gx), p.py(gy), p.pl(34), p.pl(34)), s / 10, c);
            }
        }
        Sym::Close => {
            fb.line16(p.x(26), p.y(26), p.x(74), p.y(74), w, c, 255);
            fb.line16(p.x(74), p.y(26), p.x(26), p.y(74), w, c, 255);
        }
        Sym::Minus => fb.line16(p.x(24), p.y(50), p.x(76), p.y(50), w, c, 255),
        Sym::Plus => {
            fb.line16(p.x(24), p.y(50), p.x(76), p.y(50), w, c, 255);
            fb.line16(p.x(50), p.y(24), p.x(50), p.y(76), w, c, 255);
        }
        Sym::Check => {
            fb.line16(p.x(20), p.y(52), p.x(42), p.y(74), w + 4, c, 255);
            fb.line16(p.x(42), p.y(74), p.x(82), p.y(28), w + 4, c, 255);
        }
        Sym::Moon => {
            // crescente: cobertura do disco grande menos a do disco deslocado
            let (cx, cy, r) = (p.x(48), p.y(52), p.l(40));
            let (cx2, cy2, r2) = (p.x(70), p.y(34), p.l(32));
            for yy in y..y + s {
                for xx in x..x + s {
                    let (px, py) = (xx * 16 + 8, yy * 16 + 8);
                    let d1 = crate::gfx::isqrt(((px - cx) * (px - cx) + (py - cy) * (py - cy)) as u32) as i32;
                    let d2 = crate::gfx::isqrt(((px - cx2) * (px - cx2) + (py - cy2) * (py - cy2)) as u32) as i32;
                    let c1 = (r + 8 - d1).clamp(0, 16);
                    let c2 = (d2 - r2 + 8).clamp(0, 16);
                    let cov = c1.min(c2) as u32;
                    if cov > 0 { fb.blend(xx, yy, c, (cov * 16).min(255)); }
                }
            }
        }
        Sym::Sun => {
            fb.disc16(p.x(50), p.y(50), p.l(22), c, 255);
            for k in 0..8 {
                let a = k * 32;
                let (r0, r1) = (p.l(32), p.l(44));
                fb.line16(p.x(50) + r0 * sin256(a) / 1024, p.y(50) - r0 * cos256(a) / 1024,
                    p.x(50) + r1 * sin256(a) / 1024, p.y(50) - r1 * cos256(a) / 1024, w, c, 255);
            }
        }
        Sym::Image => {
            fb.round_border(Rect::new(p.px(8), p.py(16), p.pl(84), p.pl(68)), s / 8, c, 255);
            fb.triangle([(p.px(16), p.py(76)), (p.px(42), p.py(44)), (p.px(64), p.py(76))], c, 255);
            fb.triangle([(p.px(48), p.py(76)), (p.px(66), p.py(54)), (p.px(84), p.py(76))], c, 200);
            fb.disc16(p.x(68), p.y(34), p.l(8), c, 255);
        }
        Sym::Dock => {
            fb.round_border(Rect::new(p.px(6), p.py(14), p.pl(88), p.pl(72)), s / 8, c, 255);
            fb.round_rect(Rect::new(p.px(20), p.py(62), p.pl(60), p.pl(14)), s / 14, c);
        }
        Sym::Info => {
            fb.ring16(p.x(50), p.y(50), p.l(44), w, c, 255);
            fb.disc16(p.x(50), p.y(30), p.l(8), c, 255);
            fb.line16(p.x(50), p.y(46), p.x(50), p.y(74), w + 2, c, 255);
        }
        Sym::Keyboard => {
            fb.round_border(Rect::new(p.px(4), p.py(22), p.pl(92), p.pl(56)), s / 8, c, 255);
            for row in 0..2 {
                for col in 0..5 {
                    fb.fill(Rect::new(p.px(14 + col * 16), p.py(34 + row * 14), p.pl(8).max(1), p.pl(6).max(1)), c);
                }
            }
            fb.fill(Rect::new(p.px(28), p.py(62), p.pl(44), p.pl(6).max(1)), c);
        }
        Sym::Disk => {
            fb.round_border(Rect::new(p.px(8), p.py(24), p.pl(84), p.pl(52)), s / 8, c, 255);
            fb.line16(p.x(8), p.y(56), p.x(92), p.y(56), w / 2 + 4, c, 255);
            fb.disc16(p.x(78), p.y(66), p.l(6), c, 255);
        }
        Sym::Folder => {
            fb.round_rect(Rect::new(p.px(8), p.py(20), p.pl(38), p.pl(18)), s / 10, c);
            fb.round_rect(Rect::new(p.px(8), p.py(30), p.pl(84), p.pl(52)), s / 10, c);
        }
        Sym::Apps => {
            for k in 0..9 {
                fb.disc16(p.x(20 + (k % 3) * 30), p.y(20 + (k / 3) * 30), p.l(10), c, 255);
            }
        }
        Sym::Doc => {
            fb.round_border(Rect::new(p.px(18), p.py(8), p.pl(64), p.pl(84)), s / 10, c, 255);
            for k in 0..3 {
                fb.line16(p.x(32), p.y(34 + k * 16), p.x(68), p.y(34 + k * 16), w / 2 + 4, c, 255);
            }
        }
        Sym::Clock => {
            fb.ring16(p.x(50), p.y(50), p.l(44), w, c, 255);
            fb.line16(p.x(50), p.y(50), p.x(50), p.y(24), w, c, 255);
            fb.line16(p.x(50), p.y(50), p.x(68), p.y(60), w, c, 255);
        }
    }
}
