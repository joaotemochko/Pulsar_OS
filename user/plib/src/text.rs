//! Texto anti-aliased: atlas de alpha gerados por tools/genfonts.py.
//! As strings sao UTF-8; o renderizador cobre ASCII + Latin-1 (acentos do
//! portugues), com pontuacao tipografica trocada pela mais proxima; o resto
//! vira '?'.
use crate::gfx::Fb;

#[derive(Clone, Copy)]
pub struct Glyph {
    pub w: u16,
    pub h: u16,
    pub adv: i16,
    pub left: i16,
    pub top: i16,
    pub off: u32,
}

pub struct Font {
    pub glyphs: &'static [Glyph],
    pub pix: &'static [u8],
    pub ascent: i32,
    pub height: i32,
}

/// Decodifica o proximo codepoint UTF-8 a partir de `*i`.
pub fn next_char(s: &[u8], i: &mut usize) -> u32 {
    let b = s[*i];
    *i += 1;
    if b < 0x80 {
        return b as u32;
    }
    let (len, init) = if b & 0xE0 == 0xC0 { (1, (b & 0x1F) as u32) }
        else if b & 0xF0 == 0xE0 { (2, (b & 0x0F) as u32) }
        else if b & 0xF8 == 0xF0 { (3, (b & 0x07) as u32) }
        else { return '?' as u32 };
    let mut cp = init;
    for _ in 0..len {
        if *i >= s.len() || s[*i] & 0xC0 != 0x80 { return '?' as u32; }
        cp = (cp << 6) | (s[*i] & 0x3F) as u32;
        *i += 1;
    }
    cp
}

/// Numero de caracteres (codepoints) da string UTF-8.
pub fn char_count(s: &[u8]) -> usize {
    s.iter().filter(|&&b| b & 0xC0 != 0x80).count()
}

impl Font {
    #[inline]
    fn index(cp: u32) -> usize {
        match cp {
            32..=126 => (cp - 32) as usize,
            160..=255 => (cp - 160 + 95) as usize,
            // pontuacao tipografica comum em titulos (ex.: "Pagina — Mozilla
            // Firefox"): o equivalente mais proximo do Latin-1
            0x2010..=0x2015 | 0x2212 => ('-' as u32 - 32) as usize,
            0x2018..=0x201B => ('\'' as u32 - 32) as usize,
            0x201C..=0x201F => ('"' as u32 - 32) as usize,
            0x2022 | 0x2026 => (0xB7 - 160 + 95) as usize,       // bullet / reticencias: ponto medio
            _ => ('?' as u32 - 32) as usize,
        }
    }

    #[inline]
    pub fn glyph(&self, cp: u32) -> &Glyph {
        &self.glyphs[Self::index(cp)]
    }

    pub fn char_width(&self, cp: u32) -> i32 {
        self.glyph(cp).adv as i32
    }

    /// Largura em pixels de uma string UTF-8.
    pub fn width(&self, s: &[u8]) -> i32 {
        let mut w = 0;
        let mut i = 0;
        while i < s.len() {
            w += self.char_width(next_char(s, &mut i));
        }
        w
    }

    /// Quantos bytes de `s` cabem em `max_w` pixels.
    pub fn fit(&self, s: &[u8], max_w: i32) -> usize {
        let mut w = 0;
        let mut i = 0;
        while i < s.len() {
            let start = i;
            let cw = self.char_width(next_char(s, &mut i));
            if w + cw > max_w { return start; }
            w += cw;
        }
        s.len()
    }
}

impl Fb {
    /// Desenha `s` com o topo da caixa de texto em `y`. Retorna a largura.
    pub fn text(&self, f: &Font, x: i32, y: i32, s: &[u8], color: u32) -> i32 {
        self.text_a(f, x, y, s, color, 255)
    }

    pub fn text_a(&self, f: &Font, x: i32, y: i32, s: &[u8], color: u32, alpha: u32) -> i32 {
        let mut pen = x;
        let c = self.clip;
        let below = y + f.height < c.y || y > c.y + c.h;
        let mut i = 0;
        while i < s.len() {
            let cp = next_char(s, &mut i);
            let g = f.glyph(cp);
            if !below && g.w > 0 && pen + g.left as i32 + (g.w as i32) > c.x && pen < c.x + c.w {
                self.draw_glyph(f, g, pen, y, color, alpha);
            }
            pen += g.adv as i32;
            if pen > c.x + c.w { break; }
        }
        pen - x
    }

    fn draw_glyph(&self, f: &Font, g: &Glyph, pen: i32, y: i32, color: u32, alpha: u32) {
        let gx = pen + g.left as i32;
        let gy = y + f.ascent + g.top as i32;
        let gw = g.w as i32;
        let gh = g.h as i32;
        let c = self.clip;
        let x0 = gx.max(c.x);
        let x1 = (gx + gw).min(c.x + c.w);
        let y0 = gy.max(c.y);
        let y1 = (gy + gh).min(c.y + c.h);
        if x0 >= x1 || y0 >= y1 { return; }
        let data = &f.pix[g.off as usize..];
        for yy in y0..y1 {
            let row = unsafe { self.base.add((yy * self.stride) as usize) };
            let src = ((yy - gy) * gw) as usize;
            for xx in x0..x1 {
                let a = data[src + (xx - gx) as usize] as u32;
                if a != 0 {
                    let a = if alpha == 255 { a } else { a * alpha / 255 };
                    unsafe {
                        let p = row.add(xx as usize);
                        *p = crate::gfx::mix(*p, color, a);
                    }
                }
            }
        }
    }

    /// Texto centralizado horizontal e verticalmente em `r`.
    pub fn text_center(&self, f: &Font, r: crate::gfx::Rect, s: &[u8], color: u32) {
        let w = f.width(s);
        self.text(f, r.x + (r.w - w) / 2, r.y + (r.h - f.height) / 2, s, color);
    }

    /// Texto alinhado a direita terminando em `right`.
    pub fn text_right(&self, f: &Font, right: i32, y: i32, s: &[u8], color: u32) -> i32 {
        let w = f.width(s);
        self.text(f, right - w, y, s, color);
        w
    }

    /// Texto truncado com "..." se passar de `max_w`.
    pub fn text_clip(&self, f: &Font, x: i32, y: i32, max_w: i32, s: &[u8], color: u32) -> i32 {
        if f.width(s) <= max_w {
            return self.text(f, x, y, s, color);
        }
        let dots = f.width(b"...");
        let n = f.fit(s, max_w - dots);
        let w = self.text(f, x, y, &s[..n], color);
        w + self.text(f, x + w, y, b"...", color)
    }
}
