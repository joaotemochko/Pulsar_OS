//! Primitivas graficas de software, otimizadas para o compositor.
//!
//! Regras de desempenho:
//!  * toda primitiva itera SO sobre a intersecao com o retangulo de clip
//!    (o compositor recompoe apenas a area danificada);
//!  * preenchimentos sao feitos por spans de linha, nunca pixel a pixel
//!    com testes de limite;
//!  * cantos arredondados usam mascaras de cobertura pre-calculadas;
//!  * sombras usam distancia ao retangulo arredondado (uma passada), em
//!    vez de dezenas de molduras sobrepostas.
//! Cores: 0x00RRGGBB. Alpha: 0..=255.

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }
    pub const fn empty() -> Rect {
        Rect { x: 0, y: 0, w: 0, h: 0 }
    }
    #[inline] pub fn right(&self) -> i32 { self.x + self.w }
    #[inline] pub fn bottom(&self) -> i32 { self.y + self.h }
    #[inline] pub fn is_empty(&self) -> bool { self.w <= 0 || self.h <= 0 }
    #[inline]
    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
    pub fn contains_rect(&self, o: &Rect) -> bool {
        o.x >= self.x && o.y >= self.y && o.right() <= self.right() && o.bottom() <= self.bottom()
    }
    pub fn intersect(&self, o: &Rect) -> Rect {
        let x0 = self.x.max(o.x);
        let y0 = self.y.max(o.y);
        let x1 = self.right().min(o.right());
        let y1 = self.bottom().min(o.bottom());
        if x1 <= x0 || y1 <= y0 { Rect::empty() } else { Rect::new(x0, y0, x1 - x0, y1 - y0) }
    }
    pub fn overlaps(&self, o: &Rect) -> bool {
        !self.intersect(o).is_empty()
    }
    pub fn union(&self, o: &Rect) -> Rect {
        if self.is_empty() { return *o; }
        if o.is_empty() { return *self; }
        let x0 = self.x.min(o.x);
        let y0 = self.y.min(o.y);
        let x1 = self.right().max(o.right());
        let y1 = self.bottom().max(o.bottom());
        Rect::new(x0, y0, x1 - x0, y1 - y0)
    }
    pub fn inset(&self, d: i32) -> Rect {
        Rect::new(self.x + d, self.y + d, self.w - 2 * d, self.h - 2 * d)
    }
    pub fn offset(&self, dx: i32, dy: i32) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.w, self.h)
    }
}

/// Mistura `fg` sobre `bg` com alpha 0..=255 (duas multiplicacoes).
#[inline(always)]
pub fn mix(bg: u32, fg: u32, a: u32) -> u32 {
    if a >= 255 { return fg; }
    let a = a + (a >> 7); // 0..=256
    let ia = 256 - a;
    let rb = ((fg & 0xFF00FF) * a + (bg & 0xFF00FF) * ia) >> 8;
    let g = ((fg & 0x00FF00) * a + (bg & 0x00FF00) * ia) >> 8;
    (rb & 0xFF00FF) | (g & 0x00FF00)
}

/// Interpola duas cores (t: 0..=255).
#[inline]
pub fn lerp(c0: u32, c1: u32, t: u32) -> u32 {
    mix(c0, c1, t.min(255))
}

/// Clareia (t>0) ou escurece (t<0) uma cor.
pub fn shade(c: u32, t: i32) -> u32 {
    if t >= 0 { mix(c, 0xFFFFFF, t as u32) } else { mix(c, 0x000000, (-t) as u32) }
}

/// Raiz quadrada inteira (digito a digito, sem divisao).
pub fn isqrt(n: u32) -> u32 {
    let mut x = n;
    let mut r = 0u32;
    let mut bit = 1u32 << 30;
    while bit > x { bit >>= 2; }
    while bit != 0 {
        if x >= r + bit {
            x -= r + bit;
            r = (r >> 1) + bit;
        } else {
            r >>= 1;
        }
        bit >>= 2;
    }
    r
}

/// Raiz quadrada inteira de 64 bits.
pub fn isqrt64(n: u64) -> u64 {
    let mut x = n;
    let mut r = 0u64;
    let mut bit = 1u64 << 62;
    while bit > x { bit >>= 2; }
    while bit != 0 {
        if x >= r + bit { x -= r + bit; r = (r >> 1) + bit; } else { r >>= 1; }
        bit >>= 2;
    }
    r
}

/// Seno em ponto fixo: angulo em 1/256 de volta, resultado em [-1024, 1024].
pub fn sin256(a: i32) -> i32 {
    const Q: [i16; 65] = [
        0, 25, 50, 75, 100, 125, 150, 175, 200, 224, 249, 273, 297, 321, 345, 369,
        392, 415, 438, 460, 483, 505, 526, 548, 569, 590, 610, 630, 650, 669, 688, 706,
        724, 742, 759, 775, 792, 807, 822, 837, 851, 865, 878, 891, 903, 915, 926, 936,
        946, 955, 964, 972, 980, 987, 993, 999, 1004, 1009, 1013, 1016, 1019, 1021, 1023, 1024,
        1024,
    ];
    let a = a & 255;
    let (q, i) = (a / 64, (a % 64) as usize);
    match q {
        0 => Q[i] as i32,
        1 => Q[64 - i] as i32,
        2 => -(Q[i] as i32),
        _ => -(Q[64 - i] as i32),
    }
}

pub fn cos256(a: i32) -> i32 {
    sin256(a + 64)
}

// ---------------------------------------------------------------------
// Mascaras de canto (cobertura AA de um quarto de circulo), por raio.

const MAX_RAD: usize = 32;
static mut CORNER: [[u8; MAX_RAD * MAX_RAD]; MAX_RAD + 1] = [[0; MAX_RAD * MAX_RAD]; MAX_RAD + 1];
static mut CORNER_OK: [bool; MAX_RAD + 1] = [false; MAX_RAD + 1];

/// Cobertura (0..255) do pixel (dx,dy) do quadrante superior-esquerdo de
/// um canto de raio `r` (centro do circulo em (r,r)).
fn corner_cov(r: i32, dx: i32, dy: i32) -> u32 {
    let px = dx * 16 + 8 - r * 16;
    let py = dy * 16 + 8 - r * 16;
    let d = isqrt((px * px + py * py) as u32) as i32;
    ((r * 16 + 8 - d).clamp(0, 16) * 16).min(255) as u32
}

fn corner_mask(r: i32) -> &'static [u8; MAX_RAD * MAX_RAD] {
    let ri = r as usize;
    unsafe {
        let m = &mut *(&raw mut CORNER[ri]);
        if !CORNER_OK[ri] {
            for dy in 0..r {
                for dx in 0..r {
                    m[(dy * r + dx) as usize] = corner_cov(r, dx, dy) as u8;
                }
            }
            CORNER_OK[ri] = true;
        }
        m
    }
}

// ---------------------------------------------------------------------

/// Destino de desenho: tela, wallpaper ou superficie de app.
#[derive(Clone, Copy)]
pub struct Fb {
    pub base: *mut u32,
    pub w: i32,
    pub h: i32,
    pub stride: i32,
    pub clip: Rect,
}

impl Fb {
    pub fn new(base: *mut u32, w: i32, h: i32, stride: i32) -> Fb {
        Fb { base, w, h, stride, clip: Rect::new(0, 0, w, h) }
    }

    pub fn bounds(&self) -> Rect {
        Rect::new(0, 0, self.w, self.h)
    }

    pub fn set_clip(&mut self, r: Rect) {
        self.clip = r.intersect(&self.bounds());
    }

    pub fn clear_clip(&mut self) {
        self.clip = self.bounds();
    }

    #[inline(always)]
    fn row(&self, y: i32) -> *mut u32 {
        unsafe { self.base.add((y * self.stride) as usize) }
    }

    #[inline]
    pub fn pixel(&self, x: i32, y: i32, c: u32) {
        if self.clip.contains(x, y) {
            unsafe { *self.row(y).add(x as usize) = c };
        }
    }

    #[inline]
    pub fn get(&self, x: i32, y: i32) -> u32 {
        if x >= 0 && y >= 0 && x < self.w && y < self.h {
            unsafe { *self.row(y).add(x as usize) }
        } else {
            0
        }
    }

    #[inline]
    pub fn blend(&self, x: i32, y: i32, c: u32, a: u32) {
        if a != 0 && self.clip.contains(x, y) {
            unsafe {
                let p = self.row(y).add(x as usize);
                *p = mix(*p, c, a);
            }
        }
    }

    /// Span horizontal [x0,x1) na linha y, ja recortado.
    #[inline]
    fn span(&self, y: i32, x0: i32, x1: i32, c: u32, a: u32) {
        if y < self.clip.y || y >= self.clip.bottom() { return; }
        let x0 = x0.max(self.clip.x);
        let x1 = x1.min(self.clip.right());
        if x0 >= x1 || a == 0 { return; }
        let row = self.row(y);
        unsafe {
            let s = core::slice::from_raw_parts_mut(row.add(x0 as usize), (x1 - x0) as usize);
            if a >= 255 {
                s.fill(c);
            } else {
                for p in s.iter_mut() { *p = mix(*p, c, a); }
            }
        }
    }

    pub fn fill(&self, r: Rect, c: u32) {
        self.fill_a(r, c, 255);
    }

    pub fn fill_a(&self, r: Rect, c: u32, a: u32) {
        let r = r.intersect(&self.clip);
        for y in r.y..r.bottom() {
            self.span(y, r.x, r.right(), c, a);
        }
    }

    pub fn hline(&self, x: i32, y: i32, w: i32, c: u32, a: u32) {
        self.span(y, x, x + w, c, a);
    }

    pub fn vline(&self, x: i32, y: i32, h: i32, c: u32, a: u32) {
        for yy in y..y + h { self.blend(x, yy, c, a); }
    }

    pub fn frame(&self, r: Rect, c: u32, a: u32) {
        self.hline(r.x, r.y, r.w, c, a);
        self.hline(r.x, r.bottom() - 1, r.w, c, a);
        self.vline(r.x, r.y + 1, r.h - 2, c, a);
        self.vline(r.right() - 1, r.y + 1, r.h - 2, c, a);
    }

    /// Gradiente vertical.
    pub fn vgrad(&self, r: Rect, top: u32, bot: u32) {
        self.vgrad_a(r, top, bot, 255);
    }

    pub fn vgrad_a(&self, r: Rect, top: u32, bot: u32, a: u32) {
        let cr = r.intersect(&self.clip);
        let h = r.h.max(1);
        for y in cr.y..cr.bottom() {
            let t = ((y - r.y) * 255 / h) as u32;
            self.span(y, cr.x, cr.right(), lerp(top, bot, t), a);
        }
    }

    /// Gradiente horizontal.
    pub fn hgrad(&self, r: Rect, left: u32, right: u32) {
        let cr = r.intersect(&self.clip);
        let w = r.w.max(1);
        for y in cr.y..cr.bottom() {
            let row = self.row(y);
            for x in cr.x..cr.right() {
                let t = ((x - r.x) * 255 / w) as u32;
                unsafe { *row.add(x as usize) = lerp(left, right, t) };
            }
        }
    }

    /// Retangulo arredondado com gradiente vertical e alpha global.
    pub fn round_rect_grad(&self, r: Rect, rad: i32, top: u32, bot: u32, a: u32) {
        if r.is_empty() { return; }
        let rad = rad.min(r.w / 2).min(r.h / 2).clamp(0, MAX_RAD as i32);
        let cr = r.intersect(&self.clip);
        if cr.is_empty() { return; }
        let mask = corner_mask(rad.max(1));
        let h = r.h.max(1);
        for y in cr.y..cr.bottom() {
            let c = if top == bot { top } else { lerp(top, bot, ((y - r.y) * 255 / h) as u32) };
            let dy = if y < r.y + rad { y - r.y } else if y >= r.bottom() - rad { r.bottom() - 1 - y } else { -1 };
            if dy < 0 || rad == 0 {
                self.span(y, r.x, r.right(), c, a);
                continue;
            }
            // cantos: pixels com cobertura parcial; meio: span cheio
            let mrow = &mask[(dy * rad) as usize..(dy * rad + rad) as usize];
            for dx in 0..rad {
                let cov = mrow[dx as usize] as u32;
                if cov == 0 { continue; }
                let aa = cov * a / 255;
                self.blend(r.x + dx, y, c, aa);
                self.blend(r.right() - 1 - dx, y, c, aa);
            }
            self.span(y, r.x + rad, r.right() - rad, c, a);
        }
    }

    pub fn round_rect(&self, r: Rect, rad: i32, c: u32) {
        self.round_rect_grad(r, rad, c, c, 255);
    }

    pub fn round_rect_a(&self, r: Rect, rad: i32, c: u32, a: u32) {
        self.round_rect_grad(r, rad, c, c, a);
    }

    /// Contorno arredondado de 1 px (anti-aliased).
    pub fn round_border(&self, r: Rect, rad: i32, c: u32, a: u32) {
        if r.is_empty() { return; }
        let rad = rad.min(r.w / 2).min(r.h / 2).clamp(1, MAX_RAD as i32);
        let outer = corner_mask(rad);
        let inner_r = rad - 1;
        let cr = r.intersect(&self.clip);
        if cr.is_empty() { return; }
        // bordas retas
        self.span(r.y, r.x + rad, r.right() - rad, c, a);
        self.span(r.bottom() - 1, r.x + rad, r.right() - rad, c, a);
        for y in (r.y + rad).max(cr.y)..(r.bottom() - rad).min(cr.bottom()) {
            self.blend(r.x, y, c, a);
            self.blend(r.right() - 1, y, c, a);
        }
        // cantos: cobertura externa menos a interna (raio-1, deslocada 1px)
        for dy in 0..rad {
            for dx in 0..rad {
                let o = outer[(dy * rad + dx) as usize] as i32;
                let i = if inner_r > 0 && dx >= 1 && dy >= 1 {
                    corner_mask(inner_r)[((dy - 1) * inner_r + dx - 1) as usize] as i32
                } else { 0 };
                let cov = (o - i).max(0) as u32;
                if cov == 0 { continue; }
                let aa = cov * a / 255;
                self.blend(r.x + dx, r.y + dy, c, aa);
                self.blend(r.right() - 1 - dx, r.y + dy, c, aa);
                self.blend(r.x + dx, r.bottom() - 1 - dy, c, aa);
                self.blend(r.right() - 1 - dx, r.bottom() - 1 - dy, c, aa);
            }
        }
    }

    /// Sombra suave de um retangulo arredondado: alpha cai com o quadrado
    /// da distancia ate `blur` px. `dy` desloca a sombra para baixo.
    pub fn shadow(&self, r: Rect, rad: i32, blur: i32, dy: i32, alpha: u32) {
        if blur <= 0 || alpha == 0 { return; }
        let s = r.offset(0, dy);
        let outer = s.inset(-blur);
        let cr = outer.intersect(&self.clip);
        if cr.is_empty() { return; }
        let inner = r.inset(rad.max(2)); // coberto pela propria janela
        let b16 = blur * 16;
        let (sx0, sx1) = ((s.x + rad) * 16, (s.right() - rad) * 16);
        let (sy0, sy1) = ((s.y + rad) * 16, (s.bottom() - rad) * 16);
        for y in cr.y..cr.bottom() {
            let py = y * 16 + 8;
            let qy = if py < sy0 { sy0 - py } else if py > sy1 { py - sy1 } else { 0 };
            let in_inner_rows = y >= inner.y && y < inner.bottom();
            let row = self.row(y);
            let mut x = cr.x;
            while x < cr.right() {
                if in_inner_rows && x >= inner.x && x < inner.right() {
                    x = inner.right();
                    continue;
                }
                let px = x * 16 + 8;
                let qx = if px < sx0 { sx0 - px } else if px > sx1 { px - sx1 } else { 0 };
                let d = if qx == 0 { qy } else if qy == 0 { qx } else {
                    isqrt((qx * qx + qy * qy) as u32) as i32
                } - rad * 16;
                if d < b16 {
                    let a = if d <= 0 { alpha } else {
                        let t = (256 - d * 256 / b16) as u32; // 256..0
                        alpha * ((t * t) >> 8) >> 8
                    };
                    if a > 0 {
                        unsafe {
                            let p = row.add(x as usize);
                            *p = mix(*p, 0, a);
                        }
                    }
                }
                x += 1;
            }
        }
    }

    /// Disco anti-aliased de centro (cx,cy) (em px, borda de pixel) e raio r.
    pub fn disc(&self, cx: i32, cy: i32, r: i32, c: u32, a: u32) {
        self.disc16(cx * 16, cy * 16, r * 16, c, a);
    }

    /// Disco em coordenadas de 1/16 px (posicoes sub-pixel para icones).
    /// Por linha: o miolo e um span solido; so a borda (~1-2 px de cada
    /// lado) calcula cobertura com raiz quadrada.
    pub fn disc16(&self, cx: i32, cy: i32, r: i32, c: u32, a: u32) {
        if r <= 0 { return; }
        let bb = Rect::new((cx - r) / 16 - 1, (cy - r) / 16 - 1, r / 8 + 3, r / 8 + 3);
        let cr = bb.intersect(&self.clip);
        let rin = (r - 8).max(0);
        let rout = r + 8;
        for y in cr.y..cr.bottom() {
            let py = y * 16 + 8 - cy;
            if py.abs() >= rout { continue; }
            // meia-largura (1/16 px) totalmente coberta e a de alcance maximo
            let full = if py.abs() < rin { isqrt((rin * rin - py * py) as u32) as i32 } else { -1 };
            let reach = isqrt((rout * rout - py * py) as u32) as i32;
            let x_lo = ((cx - reach - 8) / 16).max(cr.x);
            let x_hi = ((cx + reach + 8) / 16 + 1).min(cr.right());
            let mut x = x_lo;
            while x < x_hi {
                let px = x * 16 + 8 - cx;
                if full >= 0 && px.abs() <= full - 8 {
                    // pula o span solido inteiro de uma vez
                    let x_end = ((cx + full - 8 - 8) / 16 + 1).min(x_hi);
                    if x_end > x {
                        self.span(y, x, x_end, c, a);
                        x = x_end;
                        continue;
                    }
                }
                let d = isqrt((px * px + py * py) as u32) as i32;
                let cov = (r + 8 - d).clamp(0, 16) as u32;
                if cov > 0 {
                    self.blend(x, y, c, (cov * 16).min(255) * a / 255);
                }
                x += 1;
            }
        }
    }

    /// Elipse anti-aliased (1/16 px): centro (cx,cy), semi-eixos rx, ry.
    pub fn ellipse16(&self, cx: i32, cy: i32, rx: i32, ry: i32, c: u32, a: u32) {
        if rx <= 0 || ry <= 0 { return; }
        let bb = Rect::new((cx - rx) / 16 - 1, (cy - ry) / 16 - 1, rx / 8 + 3, ry / 8 + 3);
        let cr = bb.intersect(&self.clip);
        let (rx, ry) = (rx as i64, ry as i64);
        let rmin = rx.min(ry);
        for y in cr.y..cr.bottom() {
            let py = (y * 16 + 8 - cy) as i64;
            for x in cr.x..cr.right() {
                let px = (x * 16 + 8 - cx) as i64;
                // distancia normalizada ao contorno, escalada para o menor eixo
                let q = (px * ry) * (px * ry) + (py * rx) * (py * rx);
                let d = isqrt64(q as u64) as i64 * rmin / (rx * ry);
                let cov = (rmin + 8 - d).clamp(0, 16) as u32;
                if cov > 0 { self.blend(x, y, c, (cov * 16).min(255) * a / 255); }
            }
        }
    }

    /// Anel anti-aliased (raio externo r, espessura t), em px.
    pub fn ring(&self, cx: i32, cy: i32, r: i32, t: i32, c: u32, a: u32) {
        self.ring16(cx * 16, cy * 16, r * 16, t * 16, c, a);
    }

    pub fn ring16(&self, cx: i32, cy: i32, r: i32, t: i32, c: u32, a: u32) {
        let bb = Rect::new((cx - r) / 16 - 1, (cy - r) / 16 - 1, r / 8 + 3, r / 8 + 3);
        let cr = bb.intersect(&self.clip);
        for y in cr.y..cr.bottom() {
            let py = y * 16 + 8 - cy;
            for x in cr.x..cr.right() {
                let px = x * 16 + 8 - cx;
                let d = isqrt((px * px + py * py) as u32) as i32;
                let outer = r + 8 - d;
                let inner = d - (r - t) + 8;
                let cov = outer.min(inner).clamp(0, 16) as u32;
                if cov > 0 {
                    self.blend(x, y, c, (cov * 16).min(255) * a / 255);
                }
            }
        }
    }

    /// Linha grossa anti-aliased com pontas arredondadas (coordenadas em px).
    pub fn line(&self, x0: i32, y0: i32, x1: i32, y1: i32, width: i32, c: u32, a: u32) {
        self.line16(x0 * 16, y0 * 16, x1 * 16, y1 * 16, width * 16, c, a);
    }

    /// Linha em 1/16 px: distancia de cada pixel ao segmento.
    pub fn line16(&self, x0: i32, y0: i32, x1: i32, y1: i32, width: i32, c: u32, a: u32) {
        let hw = width / 2;
        let bb = Rect::new(
            (x0.min(x1) - hw) / 16 - 1,
            (y0.min(y1) - hw) / 16 - 1,
            ((x0 - x1).abs() + width) / 16 + 3,
            ((y0 - y1).abs() + width) / 16 + 3,
        );
        let cr = bb.intersect(&self.clip);
        let dx = (x1 - x0) as i64;
        let dy = (y1 - y0) as i64;
        let len2 = (dx * dx + dy * dy).max(1);
        for y in cr.y..cr.bottom() {
            let py = (y * 16 + 8) as i64;
            for x in cr.x..cr.right() {
                let px = (x * 16 + 8) as i64;
                // projecao no segmento, t em [0, len2]
                let t = ((px - x0 as i64) * dx + (py - y0 as i64) * dy).clamp(0, len2);
                let qx = x0 as i64 + dx * t / len2 - px;
                let qy = y0 as i64 + dy * t / len2 - py;
                let d = isqrt((qx * qx + qy * qy).min(u32::MAX as i64) as u32) as i32;
                let cov = (hw + 8 - d).clamp(0, 16) as u32;
                if cov > 0 {
                    self.blend(x, y, c, (cov * 16).min(255) * a / 255);
                }
            }
        }
    }

    /// Triangulo preenchido (sem AA nas arestas; use `line` por cima se
    /// precisar de bordas suaves). Coordenadas em px.
    pub fn triangle(&self, p: [(i32, i32); 3], c: u32, a: u32) {
        let mut v = p;
        if v[1].1 < v[0].1 { v.swap(0, 1); }
        if v[2].1 < v[1].1 { v.swap(1, 2); }
        if v[1].1 < v[0].1 { v.swap(0, 1); }
        let (x0, y0) = v[0];
        let (x1, y1) = v[1];
        let (x2, y2) = v[2];
        let edge = |ya: i32, xa: i32, yb: i32, xb: i32, y: i32| -> i32 {
            if yb == ya { xa } else { xa + (xb - xa) * (y - ya) / (yb - ya) }
        };
        for y in y0.max(self.clip.y)..y2.min(self.clip.bottom()) {
            let xa = edge(y0, x0, y2, x2, y);
            let xb = if y < y1 { edge(y0, x0, y1, x1, y) } else { edge(y1, x1, y2, x2, y) };
            let (l, r) = if xa < xb { (xa, xb) } else { (xb, xa) };
            self.span(y, l, r, c, a);
        }
    }

    /// Copia um retangulo de outro buffer (mesmo formato) para (dx,dy).
    pub fn blit(&self, src: &Fb, sx: i32, sy: i32, w: i32, h: i32, dx: i32, dy: i32) {
        let dst = Rect::new(dx, dy, w, h).intersect(&self.clip);
        if dst.is_empty() { return; }
        let ox = sx - dx;
        let oy = sy - dy;
        for y in dst.y..dst.bottom() {
            let syy = y + oy;
            if syy < 0 || syy >= src.h { continue; }
            let x0 = dst.x.max(-ox);
            let x1 = dst.right().min(src.w - ox);
            if x0 >= x1 { continue; }
            unsafe {
                core::ptr::copy_nonoverlapping(
                    src.row(syy).add((x0 + ox) as usize),
                    self.row(y).add(x0 as usize),
                    (x1 - x0) as usize,
                );
            }
        }
    }

    /// Como `blit`, mas com cantos arredondados (AA) no topo e/ou na base:
    /// fora da curva o destino e preservado.
    pub fn blit_rounded(&self, src: &Fb, sx: i32, sy: i32, w: i32, h: i32, dx: i32, dy: i32,
                        rad: i32, top: bool, bottom: bool) {
        let rad = rad.min(w / 2).min(h / 2).clamp(0, MAX_RAD as i32);
        if rad == 0 || (!top && !bottom) {
            return self.blit(src, sx, sy, w, h, dx, dy);
        }
        let mask = corner_mask(rad);
        let dst = Rect::new(dx, dy, w, h).intersect(&self.clip);
        for y in dst.y..dst.bottom() {
            let ry = y - dy;
            let cdy = if top && ry < rad { ry } else if bottom && ry >= h - rad { h - 1 - ry } else { -1 };
            let syy = sy + ry;
            if syy < 0 || syy >= src.h { continue; }
            let srow = src.row(syy);
            let drow = self.row(y);
            if cdy < 0 {
                let x0 = dst.x.max(dx - sx);
                let x1 = dst.right().min(src.w + dx - sx);
                if x0 < x1 {
                    unsafe {
                        core::ptr::copy_nonoverlapping(srow.add((x0 - dx + sx) as usize),
                            drow.add(x0 as usize), (x1 - x0) as usize);
                    }
                }
                continue;
            }
            let mrow = &mask[(cdy * rad) as usize..(cdy * rad + rad) as usize];
            for x in dst.x..dst.right() {
                let rx = x - dx;
                let sxx = sx + rx;
                if sxx < 0 || sxx >= src.w { continue; }
                let cov = if rx < rad { mrow[rx as usize] as u32 }
                    else if rx >= w - rad { mrow[(w - 1 - rx) as usize] as u32 }
                    else { 255 };
                if cov == 0 { continue; }
                unsafe {
                    let p = drow.add(x as usize);
                    *p = mix(*p, *srow.add(sxx as usize), cov);
                }
            }
        }
    }

    /// Copia o mesmo retangulo de `src` (mesma geometria) — restaura fundo.
    pub fn copy_from(&self, src: &Fb, r: Rect) {
        self.blit(src, r.x, r.y, r.w, r.h, r.x, r.y);
    }

    /// Como `blit`, mas misturando com alpha global.
    pub fn blit_a(&self, src: &Fb, sx: i32, sy: i32, w: i32, h: i32, dx: i32, dy: i32, a: u32) {
        if a >= 255 { return self.blit(src, sx, sy, w, h, dx, dy); }
        let dst = Rect::new(dx, dy, w, h).intersect(&self.clip);
        for y in dst.y..dst.bottom() {
            let syy = y - dy + sy;
            if syy < 0 || syy >= src.h { continue; }
            for x in dst.x..dst.right() {
                let sxx = x - dx + sx;
                if sxx < 0 || sxx >= src.w { continue; }
                unsafe {
                    let p = self.row(y).add(x as usize);
                    *p = mix(*p, *src.row(syy).add(sxx as usize), a);
                }
            }
        }
    }

    /// Desfoque de caixa separavel (3 passadas ~ gaussiano) em `r`, usando
    /// `tmp` (>= max(w,h) u32) como linha auxiliar. Para o vidro fosco.
    pub fn box_blur(&self, r: Rect, radius: i32, passes: u32, tmp: &mut [u32]) {
        let r = r.intersect(&self.bounds());
        if r.is_empty() || radius <= 0 { return; }
        for _ in 0..passes {
            for y in r.y..r.bottom() {
                let row = self.row(y);
                unsafe { blur_line(row.add(r.x as usize), 1, r.w as usize, radius as usize, tmp) };
            }
            for x in r.x..r.right() {
                let col = unsafe { self.row(r.y).add(x as usize) };
                unsafe { blur_line(col, self.stride as usize, r.h as usize, radius as usize, tmp) };
            }
        }
    }
}

/// Media movel (janela 2r+1, bordas replicadas) de n pixels com passo `step`.
unsafe fn blur_line(p: *mut u32, step: usize, n: usize, r: usize, tmp: &mut [u32]) {
    unsafe {
        for i in 0..n { tmp[i] = *p.add(i * step); }
    }
    let at = |i: isize| -> u32 { tmp[(i.clamp(0, n as isize - 1)) as usize] };
    let win = (2 * r + 1) as u32;
    let (mut sr, mut sg, mut sb) = (0u32, 0u32, 0u32);
    for k in -(r as isize)..=(r as isize) {
        let c = at(k);
        sr += (c >> 16) & 0xFF; sg += (c >> 8) & 0xFF; sb += c & 0xFF;
    }
    for i in 0..n {
        unsafe { *p.add(i * step) = ((sr / win) << 16) | ((sg / win) << 8) | (sb / win) };
        let add = at(i as isize + r as isize + 1);
        let sub = at(i as isize - r as isize);
        sr = sr + ((add >> 16) & 0xFF) - ((sub >> 16) & 0xFF);
        sg = sg + ((add >> 8) & 0xFF) - ((sub >> 8) & 0xFF);
        sb = sb + (add & 0xFF) - (sub & 0xFF);
    }
}
