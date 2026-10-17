//! Widgets do WM: vidro fosco, botoes, interruptores, menus e o registro
//! de areas clicaveis (Hits) montado durante o desenho.
use crate::catalog::{Action, MI};
use plib::icons::{self, Sym};
use plib::*;

/// Areas clicaveis registradas ao desenhar uma janela/painel.
pub struct Hits {
    r: [Rect; 96],
    a: [Action; 96],
    n: usize,
}

impl Hits {
    pub const fn new() -> Hits {
        Hits { r: [Rect::empty(); 96], a: [Action::None; 96], n: 0 }
    }
    pub fn clear(&mut self) { self.n = 0; }
    pub fn push(&mut self, r: Rect, a: Action) {
        if self.n < 96 { self.r[self.n] = r; self.a[self.n] = a; self.n += 1; }
    }
    /// Acao sob o ponto (a ultima registrada vence: fica "por cima").
    pub fn at(&self, x: i32, y: i32) -> Option<(Rect, Action)> {
        (0..self.n).rev().find(|&i| self.r[i].contains(x, y)).map(|i| (self.r[i], self.a[i]))
    }
}

/// Painel de vidro fosco: papel de parede desfocado + tinta do tema.
pub fn glass(scr: &Fb, blur: &Fb, r: Rect, rad: i32, th: &Theme, on: bool) {
    if on {
        scr.blit_rounded(blur, r.x, r.y, r.w, r.h, r.x, r.y, rad, true, true);
        scr.round_rect_a(r, rad, th.glass, th.glass_alpha);
    } else {
        scr.round_rect(r, rad, th.menu_bg);
    }
    scr.round_border(r, rad, if th.dark { 0xFFFFFF } else { 0x000000 }, if th.dark { 40 } else { 28 });
}

/// Painel flutuante solido (menus, popovers) com sombra.
pub fn popover(scr: &Fb, r: Rect, rad: i32, th: &Theme) {
    scr.shadow(r, rad, 18, 6, if th.dark { 120 } else { 70 });
    scr.round_rect_a(r, rad, th.menu_bg, th.menu_alpha);
    scr.round_border(r, rad, if th.dark { 0xFFFFFF } else { 0x000000 }, if th.dark { 36 } else { 26 });
}

pub fn button(fb: &Fb, r: Rect, label: &str, th: &Theme, primary: bool, hover: bool) {
    if primary {
        let top = gfx::shade(th.accent, if hover { 40 } else { 20 });
        fb.round_rect_grad(r, 7, top, th.accent, 255);
        fb.text_center(&FONT_UI_BOLD, r, label.as_bytes(), 0xFFFFFF);
    } else {
        fb.shadow(r, 7, 3, 1, 25);
        fb.round_rect(r, 7, if hover { th.hover } else { th.ctrl_bg });
        fb.round_border(r, 7, th.ctrl_border, 255);
        fb.text_center(&FONT_UI, r, label.as_bytes(), th.text);
    }
}

/// Interruptor estilo iOS/macOS (40x24). Retorna o retangulo.
pub fn toggle(fb: &Fb, x: i32, y: i32, on: bool, th: &Theme) -> Rect {
    let r = Rect::new(x, y, 40, 24);
    fb.round_rect(r, 12, if on { th.accent } else if th.dark { 0x48484E } else { 0xD5D5DA });
    let kx = if on { x + 28 } else { x + 12 };
    fb.disc(kx, y + 12, 10, 0x000000, 40);
    fb.disc(kx, y + 12, 9, 0xFFFFFF, 255);
    r
}

/// Controle segmentado; registra cada segmento em `hits`.
pub fn segmented(fb: &Fb, r: Rect, labels: &[&str], sel: usize, th: &Theme, hits: &mut Hits, acts: &[Action]) {
    fb.round_rect(r, 8, if th.dark { 0x2C2D33 } else { 0xE9E9EE });
    let n = labels.len() as i32;
    let sw = r.w / n;
    for (i, l) in labels.iter().enumerate() {
        let sr = Rect::new(r.x + i as i32 * sw, r.y, sw, r.h);
        if i == sel {
            let k = sr.inset(2);
            fb.shadow(k, 6, 3, 1, 40);
            fb.round_rect(k, 6, if th.dark { 0x5A5B62 } else { 0xFFFFFF });
        }
        fb.text_center(&FONT_UI, sr, l.as_bytes(), th.text);
        hits.push(sr, acts[i]);
    }
}

/// Amostra de cor circular (selecionada = anel).
pub fn swatch(fb: &Fb, cx: i32, cy: i32, color: u32, selected: bool, th: &Theme) -> Rect {
    if selected {
        fb.ring(cx, cy, 13, 2, color, 255);
    }
    fb.disc(cx, cy, 9, color, 255);
    fb.ring(cx, cy, 9, 1, if th.dark { 0xFFFFFF } else { 0x000000 }, 30);
    Rect::new(cx - 13, cy - 13, 26, 26)
}

pub const MENU_ITEM_H: i32 = 26;
pub const MENU_SEP_H: i32 = 9;
pub const MENU_PAD: i32 = 6;

/// Geometria de um menu suspenso com os itens dados, ancorado em (x,y).
pub fn menu_rect(x: i32, y: i32, items: &[MI], screen_w: i32) -> Rect {
    let mut h = MENU_PAD * 2;
    let mut w = 180;
    for it in items {
        if it.0 == "-" { h += MENU_SEP_H; continue; }
        h += MENU_ITEM_H;
        let iw = FONT_UI.width(it.0.as_bytes()) + 48 + if it.2.is_empty() { 0 } else { FONT_UI.width(it.2.as_bytes()) + 24 };
        w = w.max(iw);
    }
    let x = x.min(screen_w - w - 4).max(4);
    Rect::new(x, y, w, h)
}

/// Desenha um menu; o item sob (mx,my) fica destacado. Registra os itens.
pub fn menu(fb: &Fb, r: Rect, items: &[MI], mx: i32, my: i32, th: &Theme, hits: &mut Hits, checks: &dyn Fn(Action) -> bool) {
    popover(fb, r, 9, th);
    let mut y = r.y + MENU_PAD;
    for it in items {
        if it.0 == "-" {
            fb.hline(r.x + 12, y + MENU_SEP_H / 2, r.w - 24, th.sep, 255);
            y += MENU_SEP_H;
            continue;
        }
        let ir = Rect::new(r.x + 5, y, r.w - 10, MENU_ITEM_H);
        let enabled = it.1 != Action::None;
        let hover = enabled && ir.contains(mx, my);
        if hover {
            fb.round_rect(ir, 5, th.accent);
        }
        let tc = if hover { 0xFFFFFF } else if enabled { th.text } else { th.text3 };
        if checks(it.1) {
            icons::sym(fb, Sym::Check, ir.x + 6, ir.y + 7, 12, tc);
        }
        fb.text(&FONT_UI, ir.x + 22, ir.y + 4, it.0.as_bytes(), tc);
        if !it.2.is_empty() {
            fb.text_right(&FONT_UI, ir.right() - 10, ir.y + 4, it.2.as_bytes(),
                if hover { 0xFFFFFF } else { th.text3 });
        }
        if enabled { hits.push(ir, it.1); }
        y += MENU_ITEM_H;
    }
}

/// Tooltip/rotulo em pilula (dock).
pub fn label_pill(fb: &Fb, cx: i32, bottom: i32, text: &[u8], th: &Theme) {
    let w = FONT_UI.width(text) + 22;
    let r = Rect::new(cx - w / 2, bottom - 28, w, 26);
    fb.shadow(r, 8, 8, 2, 50);
    fb.round_rect_a(r, 8, th.menu_bg, 240);
    fb.round_border(r, 8, if th.dark { 0xFFFFFF } else { 0x000000 }, 30);
    fb.text_center(&FONT_UI, r, text, th.text);
}
