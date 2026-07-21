//! plib — biblioteca de userspace do Pulsar OS.
//! Wrappers de syscall, acesso ao framebuffer e fonte 8x8.
#![no_std]

mod font_ui;
mod font_big;
pub use font_ui::*;
pub use font_ui::Glyph;
pub use font_big::*;

use core::arch::asm;

// ---------------------------------------------------------------- syscalls

#[inline]
fn syscall2(num: u64, a0: u64, a1: u64) -> u64 {
    let ret: u64;
    unsafe {
        asm!("svc #0", in("x8") num, inout("x0") a0 => ret, in("x1") a1,
             options(nostack));
    }
    ret
}

#[inline]
fn syscall0(num: u64) -> u64 {
    let ret: u64;
    unsafe { asm!("svc #0", in("x8") num, out("x0") ret, options(nostack)); }
    ret
}

#[inline]
fn syscall3(num: u64, a0: u64, a1: u64, a2: u64) -> u64 {
    let ret: u64;
    unsafe {
        asm!("svc #0", in("x8") num, inout("x0") a0 => ret, in("x1") a1, in("x2") a2,
             options(nostack));
    }
    ret
}

pub fn write(s: &str) {
    syscall2(1, s.as_ptr() as u64, s.len() as u64);
}
pub fn yield_now() {
    syscall0(2);
}
pub fn exit() -> ! {
    syscall0(3);
    loop {}
}
pub fn fb_info() -> (u32, u32) {
    let v = syscall0(4);
    ((v >> 32) as u32, v as u32)
}
pub fn fb_map() -> u64 {
    syscall0(5)
}
pub fn fb_present() {
    syscall2(6, 0, 0);
}
/// Apresenta somente uma regiao (damage rect) — use para nao interferir
/// com regioes desenhadas por outros processos.
pub fn fb_present_rect(r: Rect) {
    syscall2(6, ((r.x as u64) << 32) | r.y as u64, ((r.w as u64) << 32) | r.h as u64);
}
pub fn fs_count() -> usize {
    syscall0(8) as usize
}
/// Preenche `buf` (>= 44 bytes) com a entrada `idx`. Retorna o tamanho do arquivo.
pub fn fs_stat(idx: usize, buf: &mut [u8; 44]) -> i64 {
    syscall2(9, idx as u64, buf.as_mut_ptr() as u64) as i64
}
pub fn spawn(name: &str) -> i64 {
    syscall2(10, name.as_ptr() as u64, name.len() as u64) as i64
}

// ---------------------------------------------------------------- IPC

/// Mensagem de IPC: tag + 6 palavras. Layout casa com o kernel (56 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Msg {
    pub tag: u64,
    pub data: [u64; 6],
}
impl Msg {
    pub const fn new(tag: u64) -> Self { Msg { tag, data: [0; 6] } }
}

pub fn getpid() -> usize {
    syscall0(14) as usize
}

/// Transfere o foco de teclado para o processo `pid` (o kernel roteia o
/// teclado apenas para o processo com foco; o mouse vai para todos).
pub fn set_focus(pid: usize) {
    syscall2(18, pid as u64, 0);
}

/// Registra o processo atual como servidor do `service_id`.
pub fn register_service(service_id: u64) {
    syscall2(19, service_id, 0);
}

/// Descobre o pid do servidor de `service_id` (ou <0 se ausente).
pub fn lookup_service(service_id: u64) -> i64 {
    syscall2(20, service_id, 0) as i64
}

/// ID do servico de filesystem.
pub const SVC_FS: u64 = 1;

// Opcodes do protocolo FS (tag da mensagem)
pub const FS_OP_COUNT: u64 = 1;   // -> data[0] = numero de arquivos
pub const FS_OP_STAT: u64  = 2;   // data[0]=idx -> data: size, kind, nome(4 palavras)
pub const FS_OP_READ: u64  = 3;   // data[0]=idx, data[1]=offset -> ate 40 bytes por msg
pub const FS_OP_WRITE: u64 = 4;   // (reservado)

/// Envia `m` para o processo `dst` e bloqueia ate a resposta.
/// A resposta sobrescreve `m`. Retorna 0 (ok) ou <0 (dst invalido).
pub fn send(dst: usize, m: &mut Msg) -> i64 {
    syscall2(11, dst as u64, m as *mut Msg as u64) as i64
}

/// Bloqueia ate receber. Preenche `m`. Retorna o pid do remetente.
pub fn recv(m: &mut Msg) -> usize {
    syscall2(12, m as *mut Msg as u64, 0) as usize
}

/// Responde ao remetente `to` com `m`, desbloqueando-o.
pub fn reply(to: usize, m: &Msg) {
    syscall2(13, to as u64, m as *const Msg as u64);
}

/// RECV nao-bloqueante: retorna Some(pid) se havia mensagem, senao None.
pub fn try_recv(m: &mut Msg) -> Option<usize> {
    let r = syscall2(23, m as *mut Msg as u64, 0) as i64;
    if r < 0 { None } else { Some(r as usize) }
}

/// O processo `pid` ainda esta vivo?
pub fn is_alive(pid: usize) -> bool { syscall2(24, pid as u64, 0) != 0 }

/// Milissegundos desde o boot (resolucao de 10ms).
pub fn uptime_ms() -> u64 { syscall0(25) * 10 }

// --------------------------------------------------------- Filesystem

/// Le o arquivo `idx` para `buf` (ate buf.len() bytes). Retorna bytes lidos.
pub fn fs_read(idx: usize, buf: &mut [u8]) -> i64 {
    syscall3(15, idx as u64, buf.as_mut_ptr() as u64, buf.len() as u64) as i64
}

/// Escreve `data` no arquivo `idx`. Retorna 0 (ok) ou <0.
pub fn fs_write(idx: usize, data: &[u8]) -> i64 {
    syscall3(16, idx as u64, data.as_ptr() as u64, data.len() as u64) as i64
}

/// Cria um arquivo de texto vazio. Retorna o indice ou <0.
pub fn fs_create(name: &str) -> i64 {
    syscall2(17, name.as_ptr() as u64, name.len() as u64) as i64
}

// ------------------------------------------------------------------ input

pub const EV_KEY: u16 = 1;
pub const EV_ABS: u16 = 3;
pub const ABS_X: u16 = 0;
pub const ABS_Y: u16 = 1;
pub const BTN_LEFT: u16 = 0x110;
pub const KEY_ENTER: u16 = 28;
pub const KEY_BACKSPACE: u16 = 14;
pub const KEY_SPACE: u16 = 57;

#[derive(Clone, Copy)]
pub struct Event {
    pub ev_type: u16,
    pub code: u16,
    pub value: u32,
}

pub fn input_poll() -> Option<Event> {
    let v = syscall0(7);
    if v >> 63 == 0 {
        return None;
    }
    Some(Event {
        ev_type: ((v >> 48) & 0x7FFF) as u16,
        code: (v >> 32) as u16,
        value: v as u32,
    })
}

/// Keycode evdev -> ASCII (maiusculas), ou 0.
pub fn keycode_to_char(code: u16) -> u8 {
    const ROW1: &[u8] = b"1234567890"; // codes 2..=11
    const ROW2: &[u8] = b"QWERTYUIOP"; // 16..=25
    const ROW3: &[u8] = b"ASDFGHJKL"; // 30..=38
    const ROW4: &[u8] = b"ZXCVBNM"; // 44..=50
    match code {
        2..=11 => ROW1[(code - 2) as usize],
        16..=25 => ROW2[(code - 16) as usize],
        30..=38 => ROW3[(code - 30) as usize],
        44..=50 => ROW4[(code - 44) as usize],
        KEY_SPACE => b' ',
        52 => b'.',
        12 => b'-',
        _ => 0,
    }
}

// ------------------------------------------------------------ framebuffer

#[derive(Clone, Copy)]
pub struct Fb {
    pub base: *mut u32,
    pub w: u32,
    pub h: u32,
    // clip (scissor): escritas fora deste retangulo sao ignoradas.
    // (cx0,cy0) inclusivo, (cx1,cy1) exclusivo. Default = tela toda.
    pub cx0: u32, pub cy0: u32, pub cx1: u32, pub cy1: u32,
}

#[derive(Clone, Copy)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub fn contains(&self, px: u32, py: u32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
}

impl Fb {
    /// Mapeia o framebuffer e retorna o handle de desenho.
    pub fn open() -> Fb {
        let base = fb_map() as *mut u32;
        let (w, h) = fb_info();
        Fb { base, w, h, cx0:0, cy0:0, cx1:w, cy1:h }
    }

    /// Endereco do buffer de wallpaper (cacheado). Mesma geometria do fb.
    pub const WALL_ADDR: u64 = 0x4600_0000 + 8*1024*1024;

    /// Ponteiro para o wallpaper como *mut u32.
    pub fn wall_ptr(&self) -> *mut u32 { Self::WALL_ADDR as *mut u32 }

    /// Copia uma regiao do wallpaper cacheado para o back buffer (rapido:
    /// memcpy por linha, sem blending). Usado para "apagar" area de janela.
    pub fn restore_wall(&self, r: Rect) {
        let x0=r.x.min(self.w); let x1=(r.x+r.w).min(self.w);
        let y0=r.y.min(self.h); let y1=(r.y+r.h).min(self.h);
        if x1<=x0 || y1<=y0 { return; }
        let wall=self.wall_ptr();
        let span=(x1-x0) as usize;
        for y in y0..y1 {
            let base=(y*self.w+x0) as usize;
            unsafe {
                core::ptr::copy_nonoverlapping(wall.add(base), self.base.add(base), span);
            }
        }
    }

    /// Desenha no buffer de WALLPAPER (nao no back). Para compor o fundo 1x.
    pub fn wall_fb(&self) -> Fb { Fb { base: self.wall_ptr(), w:self.w, h:self.h, cx0:0,cy0:0,cx1:self.w,cy1:self.h } }

    /// Ponteiro bruto do back buffer (para blit de superficies).
    pub fn base_ptr(&self) -> *mut u32 { self.base }

    /// Define o retangulo de recorte (scissor). Escritas fora sao ignoradas.
    pub fn set_clip(&mut self, r: Rect) {
        self.cx0 = r.x.min(self.w);
        self.cy0 = r.y.min(self.h);
        self.cx1 = (r.x+r.w).min(self.w);
        self.cy1 = (r.y+r.h).min(self.h);
    }
    /// Remove o recorte (volta para a tela toda).
    pub fn clear_clip(&mut self) {
        self.cx0=0; self.cy0=0; self.cx1=self.w; self.cy1=self.h;
    }
    /// Um ponto esta dentro do clip atual?
    #[inline]
    pub fn in_clip(&self, x: u32, y: u32) -> bool {
        x>=self.cx0 && x<self.cx1 && y>=self.cy0 && y<self.cy1
    }


    #[inline]
    pub fn pixel(&self, x: u32, y: u32, color: u32) {
        if x < self.w && y < self.h && self.in_clip(x,y) {
            unsafe { *self.base.add((y * self.w + x) as usize) = color };
        }
    }

    pub fn fill(&self, r: Rect, color: u32) {
        let x0 = r.x.max(self.cx0); let x1 = (r.x + r.w).min(self.w).min(self.cx1);
        let y0 = r.y.max(self.cy0); let y1 = (r.y + r.h).min(self.h).min(self.cy1);
        for y in y0..y1 {
            let row = unsafe { self.base.add((y * self.w) as usize) };
            for x in x0..x1 {
                unsafe { *row.add(x as usize) = color };
            }
        }
    }

    /// Linha horizontal com "buraco" (regiao que NAO deve ser pintada).
    pub fn hline_with_hole(&self, y: u32, color: u32, hole: Option<Rect>) {
        if y >= self.h {
            return;
        }
        let row = unsafe { self.base.add((y * self.w) as usize) };
        match hole {
            Some(hr) if y >= hr.y && y < hr.y + hr.h => {
                for x in 0..hr.x.min(self.w) {
                    unsafe { *row.add(x as usize) = color };
                }
                for x in (hr.x + hr.w).min(self.w)..self.w {
                    unsafe { *row.add(x as usize) = color };
                }
            }
            _ => {
                for x in 0..self.w {
                    unsafe { *row.add(x as usize) = color };
                }
            }
        }
    }

    pub fn frame(&self, r: Rect, thick: u32, color: u32) {
        self.fill(Rect { x: r.x, y: r.y, w: r.w, h: thick }, color);
        self.fill(Rect { x: r.x, y: r.y + r.h - thick, w: r.w, h: thick }, color);
        self.fill(Rect { x: r.x, y: r.y, w: thick, h: r.h }, color);
        self.fill(Rect { x: r.x + r.w - thick, y: r.y, w: thick, h: r.h }, color);
    }

    pub fn text(&self, x: u32, y: u32, s: &[u8], color: u32, scale: u32) {
        let mut cx = x;
        for &c in s {
            let g = glyph(c.to_ascii_uppercase());
            for (row, bits) in g.iter().enumerate() {
                for col in 0..8u32 {
                    if bits & (0x80 >> col) != 0 {
                        self.fill(
                            Rect {
                                x: cx + col * scale,
                                y: y + row as u32 * scale,
                                w: scale,
                                h: scale,
                            },
                            color,
                        );
                    }
                }
            }
            cx += 9 * scale;
        }
    }

    // ============================================================
    // Primitivas "bonitas" — blending, gradientes, cantos redondos,
    // sombras e brilho. Base para a estetica estilo macOS.
    // ============================================================

    /// Mistura `fg` sobre o pixel atual com alpha 0..255 (blending real
    /// lendo o framebuffer). Permite transparencia e anti-aliasing.
    #[inline]
    pub fn blend(&self, x: u32, y: u32, fg: u32, a: u32) {
        if x >= self.w || y >= self.h { return; }
        if x<self.cx0||x>=self.cx1||y<self.cy0||y>=self.cy1 { return; }
        let a = a.min(255);
        if a == 0 { return; }
        let p = unsafe { self.base.add((y*self.w+x) as usize) };
        if a == 255 {
            unsafe { *p = fg };
            return;
        }
        let bg = unsafe { *p };
        let inv = 255 - a;
        let r = (((fg>>16)&0xFF)*a + ((bg>>16)&0xFF)*inv) / 255;
        let g = (((fg>>8)&0xFF)*a + ((bg>>8)&0xFF)*inv) / 255;
        let b = ((fg&0xFF)*a + (bg&0xFF)*inv) / 255;
        unsafe { *p = (r<<16)|(g<<8)|b };
    }

    /// Interpola duas cores (t: 0..255).
    #[inline]
    pub fn lerp(c0: u32, c1: u32, t: u32) -> u32 {
        let t = t.min(255); let it = 255 - t;
        let r = (((c0>>16)&0xFF)*it + ((c1>>16)&0xFF)*t)/255;
        let g = (((c0>>8)&0xFF)*it + ((c1>>8)&0xFF)*t)/255;
        let b = ((c0&0xFF)*it + (c1&0xFF)*t)/255;
        (r<<16)|(g<<8)|b
    }

    /// Preenche um retangulo com gradiente vertical (c_top -> c_bot).
    pub fn vgrad(&self, r: Rect, c_top: u32, c_bot: u32) {
        let x1 = (r.x+r.w).min(self.w);
        let y1 = (r.y+r.h).min(self.h);
        let h = r.h.max(1);
        for y in r.y.min(self.h)..y1 {
            let t = ((y - r.y) * 255) / h;
            let c = Fb::lerp(c_top, c_bot, t);
            let row = unsafe { self.base.add((y*self.w) as usize) };
            for x in r.x.min(self.w)..x1 {
                unsafe { *row.add(x as usize) = c };
            }
        }
    }

    /// Gradiente vertical com alpha constante (para faixas translucidas).
    pub fn vgrad_alpha(&self, r: Rect, c_top: u32, c_bot: u32, a: u32) {
        let y1 = (r.y+r.h).min(self.h);
        let h = r.h.max(1);
        for y in r.y.min(self.h)..y1 {
            let t = ((y - r.y) * 255) / h;
            let c = Fb::lerp(c_top, c_bot, t);
            for x in r.x..(r.x+r.w).min(self.w) {
                self.blend(x, y, c, a);
            }
        }
    }

    /// Distancia^2 de um ponto ao centro de uma quina, para cantos redondos.
    #[inline]
    fn corner_alpha(dx: i32, dy: i32, radius: i32) -> u32 {
        // anti-aliasing: alpha cai a 0 na borda do raio
        let d2 = (dx*dx + dy*dy) as i32;
        let r2 = radius*radius;
        if d2 <= (radius-1)*(radius-1) { 255 }
        else if d2 >= (radius+1)*(radius+1) { 0 }
        else {
            // transicao suave de ~2px
            let d = isqrt(d2 as u32) as i32;
            let frac = radius + 1 - d; // 0..2
            (frac.clamp(0,2) as u32 * 127).min(255)
        }
    }

    /// Retangulo de cantos arredondados preenchido com cor solida.
    pub fn round_rect(&self, r: Rect, radius: u32, color: u32) {
        self.round_rect_grad(r, radius, color, color);
    }

    /// Retangulo de cantos arredondados com gradiente vertical + AA nas quinas.
    pub fn round_rect_grad(&self, r: Rect, radius: u32, c_top: u32, c_bot: u32) {
        let rad = radius.min(r.w/2).min(r.h/2) as i32;
        let x0 = r.x as i32; let y0 = r.y as i32;
        let x1 = (r.x+r.w) as i32; let y1 = (r.y+r.h) as i32;
        let h = r.h.max(1);
        for y in y0..y1 {
            if y < 0 || y as u32 >= self.h { continue; }
            let t = (((y - y0) as u32) * 255) / h;
            let c = Fb::lerp(c_top, c_bot, t);
            for x in x0..x1 {
                if x < 0 || x as u32 >= self.w { continue; }
                // dentro de qual quina?
                let cx = if x < x0+rad { x0+rad } else if x >= x1-rad { x1-rad-1 } else { x };
                let cy = if y < y0+rad { y0+rad } else if y >= y1-rad { y1-rad-1 } else { y };
                if cx != x || cy != y {
                    let a = Fb::corner_alpha(x-cx, y-cy, rad);
                    if a > 0 { self.blend(x as u32, y as u32, c, a); }
                } else {
                    self.blend(x as u32, y as u32, c, 255);
                }
            }
        }
    }

    /// Contorno arredondado (1px) — usado para bordas sutis de janela.
    pub fn round_frame(&self, r: Rect, radius: u32, color: u32, a: u32) {
        let rad = radius.min(r.w/2).min(r.h/2) as i32;
        let x0 = r.x as i32; let y0 = r.y as i32;
        let x1 = (r.x+r.w) as i32; let y1 = (r.y+r.h) as i32;
        for y in y0..y1 {
            if y < 0 || y as u32 >= self.h { continue; }
            for x in x0..x1 {
                if x < 0 || x as u32 >= self.w { continue; }
                let cx = if x < x0+rad { x0+rad } else if x >= x1-rad { x1-rad-1 } else { x };
                let cy = if y < y0+rad { y0+rad } else if y >= y1-rad { y1-rad-1 } else { y };
                let on_edge = x==x0 || x==x1-1 || y==y0 || y==y1-1;
                if cx != x || cy != y {
                    // na quina: pinta so o anel externo
                    let d = isqrt(((x-cx)*(x-cx)+(y-cy)*(y-cy)) as u32) as i32;
                    if d >= rad-1 && d <= rad { self.blend(x as u32, y as u32, color, a); }
                } else if on_edge {
                    self.blend(x as u32, y as u32, color, a);
                }
            }
        }
    }

    /// Sombra suave sob um retangulo (desfoque fake por camadas de alpha).
    pub fn drop_shadow(&self, r: Rect, radius: u32, spread: u32) {
        // varias molduras arredondadas concentricas, alpha decrescente
        let layers = spread.max(1);
        for i in 0..layers {
            let off = (layers - i) as u32;
            let a = 6 + i*4; // mais forte perto da borda
            let sr = Rect {
                x: r.x.saturating_sub(off),
                y: r.y.saturating_sub(off).saturating_add(2), // desloca p/ baixo
                w: r.w + off*2,
                h: r.h + off*2,
            };
            self.round_frame(sr, radius+off, 0x00000000, a);
        }
    }

    /// Circulo preenchido com AA (para pontos do dock, botoes de janela).
    pub fn disc(&self, cx: u32, cy: u32, radius: u32, color: u32) {
        let r = radius as i32;
        let cxi = cx as i32; let cyi = cy as i32;
        for dy in -r..=r {
            for dx in -r..=r {
                let d2 = dx*dx + dy*dy;
                let a = if d2 <= (r-1)*(r-1) { 255 }
                        else if d2 >= (r+1)*(r+1) { 0 }
                        else { 140 };
                if a > 0 {
                    let x = cxi+dx; let y = cyi+dy;
                    if x>=0 && y>=0 { self.blend(x as u32, y as u32, color, a); }
                }
            }
        }
    }

    /// Texto com uma leve sombra (legibilidade sobre gradientes).
    pub fn text_shadow(&self, x: u32, y: u32, s: &[u8], color: u32, scale: u32) {
        self.text_blend(x+1, y+1, s, 0x00000000, 90, scale);
        self.text_blend(x, y, s, color, 255, scale);
    }

    /// Texto com alpha (para labels suaves).
    pub fn text_blend(&self, x: u32, y: u32, s: &[u8], color: u32, a: u32, scale: u32) {
        let mut cx = x;
        for &c in s {
            let g = glyph(c.to_ascii_uppercase());
            for (row, bits) in g.iter().enumerate() {
                for col in 0..8u32 {
                    if bits & (0x80 >> col) != 0 {
                        for sy in 0..scale { for sx in 0..scale {
                            self.blend(cx+col*scale+sx, y+row as u32*scale+sy, color, a);
                        }}
                    }
                }
            }
            cx += 9*scale;
        }
    }
}

// helper: raiz quadrada inteira (para cantos redondos)
pub fn isqrt(n: u32) -> u32 {
    if n == 0 { return 0; }
    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x { x = y; y = (x + n / x) / 2; }
    x
}

// ------------------------------------------------------------------ fonte

pub fn glyph(c: u8) -> [u8; 8] {
    match c {
        b'A' => [0x38, 0x6C, 0xC6, 0xC6, 0xFE, 0xC6, 0xC6, 0x00],
        b'B' => [0xFC, 0xC6, 0xC6, 0xFC, 0xC6, 0xC6, 0xFC, 0x00],
        b'C' => [0x7C, 0xC6, 0xC0, 0xC0, 0xC0, 0xC6, 0x7C, 0x00],
        b'D' => [0xF8, 0xCC, 0xC6, 0xC6, 0xC6, 0xCC, 0xF8, 0x00],
        b'E' => [0xFE, 0xC0, 0xC0, 0xF8, 0xC0, 0xC0, 0xFE, 0x00],
        b'F' => [0xFE, 0xC0, 0xC0, 0xF8, 0xC0, 0xC0, 0xC0, 0x00],
        b'G' => [0x7C, 0xC6, 0xC0, 0xCE, 0xC6, 0xC6, 0x7C, 0x00],
        b'H' => [0xC6, 0xC6, 0xC6, 0xFE, 0xC6, 0xC6, 0xC6, 0x00],
        b'I' => [0x7E, 0x18, 0x18, 0x18, 0x18, 0x18, 0x7E, 0x00],
        b'J' => [0x1E, 0x06, 0x06, 0x06, 0xC6, 0xC6, 0x7C, 0x00],
        b'K' => [0xC6, 0xCC, 0xD8, 0xF0, 0xD8, 0xCC, 0xC6, 0x00],
        b'L' => [0xC0, 0xC0, 0xC0, 0xC0, 0xC0, 0xC0, 0xFE, 0x00],
        b'M' => [0xC6, 0xEE, 0xFE, 0xD6, 0xC6, 0xC6, 0xC6, 0x00],
        b'N' => [0xC6, 0xE6, 0xF6, 0xDE, 0xCE, 0xC6, 0xC6, 0x00],
        b'O' => [0x7C, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x7C, 0x00],
        b'P' => [0xFC, 0xC6, 0xC6, 0xFC, 0xC0, 0xC0, 0xC0, 0x00],
        b'Q' => [0x7C, 0xC6, 0xC6, 0xC6, 0xD6, 0xCC, 0x76, 0x00],
        b'R' => [0xFC, 0xC6, 0xC6, 0xFC, 0xD8, 0xCC, 0xC6, 0x00],
        b'S' => [0x7E, 0xC0, 0xC0, 0x7C, 0x06, 0x06, 0xFC, 0x00],
        b'T' => [0x7E, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x00],
        b'U' => [0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x7C, 0x00],
        b'V' => [0xC6, 0xC6, 0xC6, 0xC6, 0x6C, 0x38, 0x10, 0x00],
        b'W' => [0xC6, 0xC6, 0xC6, 0xD6, 0xFE, 0xEE, 0xC6, 0x00],
        b'X' => [0xC6, 0x6C, 0x38, 0x10, 0x38, 0x6C, 0xC6, 0x00],
        b'Y' => [0x66, 0x66, 0x3C, 0x18, 0x18, 0x18, 0x18, 0x00],
        b'Z' => [0xFE, 0x06, 0x0C, 0x18, 0x30, 0x60, 0xFE, 0x00],
        b'0' => [0x7C, 0xC6, 0xCE, 0xD6, 0xE6, 0xC6, 0x7C, 0x00],
        b'1' => [0x18, 0x38, 0x18, 0x18, 0x18, 0x18, 0x7E, 0x00],
        b'2' => [0x7C, 0xC6, 0x06, 0x1C, 0x70, 0xC0, 0xFE, 0x00],
        b'3' => [0x7C, 0xC6, 0x06, 0x3C, 0x06, 0xC6, 0x7C, 0x00],
        b'4' => [0x0C, 0x1C, 0x3C, 0x6C, 0xFE, 0x0C, 0x0C, 0x00],
        b'5' => [0xFE, 0xC0, 0xFC, 0x06, 0x06, 0xC6, 0x7C, 0x00],
        b'6' => [0x7C, 0xC6, 0xC0, 0xFC, 0xC6, 0xC6, 0x7C, 0x00],
        b'7' => [0xFE, 0x06, 0x0C, 0x18, 0x30, 0x30, 0x30, 0x00],
        b'8' => [0x7C, 0xC6, 0xC6, 0x7C, 0xC6, 0xC6, 0x7C, 0x00],
        b'9' => [0x7C, 0xC6, 0xC6, 0x7E, 0x06, 0xC6, 0x7C, 0x00],
        b'.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x00],
        b':' => [0x00, 0x18, 0x18, 0x00, 0x18, 0x18, 0x00, 0x00],
        b'-' => [0x00, 0x00, 0x00, 0x7E, 0x00, 0x00, 0x00, 0x00],
        b'>' => [0x60, 0x30, 0x18, 0x0C, 0x18, 0x30, 0x60, 0x00],
        b'_' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFE, 0x00],
        _ => [0; 8],
    }
}

// ====================================================================
// Toolkit de janelas (client-side) — usado pelo shell/WM e pelos apps.
// ====================================================================

/// Uma janela desenhavel com barra de titulo. O WM (shell) desenha a
/// decoracao; os apps desenham o conteudo dentro da area cliente.
#[derive(Clone, Copy)]
pub struct Window {
    pub rect: Rect,       // moldura externa (inclui titulo)
    pub title_h: u32,
}

pub const TITLE_H: u32 = 26;

impl Window {
    pub fn new(x: u32, y: u32, w: u32, h: u32) -> Self {
        Window { rect: Rect { x, y, w, h }, title_h: TITLE_H }
    }

    /// Area util (abaixo da barra de titulo).
    pub fn client(&self) -> Rect {
        Rect {
            x: self.rect.x + 2,
            y: self.rect.y + self.title_h,
            w: self.rect.w - 4,
            h: self.rect.h - self.title_h - 2,
        }
    }

    /// Desenha a decoracao (fundo, barra, titulo, botao fechar).
    /// `focused` muda a cor da barra. Retorna o retangulo do botao fechar.
    pub fn decorate(&self, fb: &Fb, title: &[u8], focused: bool, bg: u32) -> Rect {
        let bar = if focused { 0x001E2A44 } else { 0x00161C2C };
        let accent = if focused { 0x004FA3FF } else { 0x00304050 };
        fb.fill(self.rect, bg);
        fb.fill(Rect { x: self.rect.x, y: self.rect.y, w: self.rect.w, h: self.title_h }, bar);
        fb.frame(self.rect, 2, accent);
        fb.text(self.rect.x + 10, self.rect.y + 8, title, 0x00E8EEF8, 1);
        // botao fechar (quadrado vermelho a direita)
        let close = Rect { x: self.rect.x + self.rect.w - 22, y: self.rect.y + 6, w: 14, h: 14 };
        fb.fill(close, 0x00E05060);
        fb.text(close.x + 3, close.y + 3, b"X", 0x00FFFFFF, 1);
        close
    }
}

/// Converte keycode evdev -> ASCII minusculo (para o editor de texto).
pub fn keycode_to_lower(code: u16) -> u8 {
    let c = keycode_to_char(code);
    if c.is_ascii_uppercase() { c + 32 } else { c }
}

// ====================================================================
// Logo do Pulsar OS + icones vetoriais para o dock.
// A logo: um anel luminoso com uma onda de pulso (batimento) cruzando —
// remete a "pulsar" (estrela de neutrons que pulsa) e a um heartbeat.
// ====================================================================

impl Fb {
    /// Desenha a logo do Pulsar centrada em (cx,cy) com raio `r`.
    /// Um anel luminoso + um traçado de batimento (ECG) contínuo cruzando.
    pub fn pulsar_logo(&self, cx: u32, cy: u32, r: u32, tint: u32, glow: bool) {
        let cxi = cx as i32; let cyi = cy as i32; let ri = r as i32;

        // halo suave externo
        if glow {
            for dy in -(ri+10)..=(ri+10) {
                for dx in -(ri+10)..=(ri+10) {
                    let d = isqrt((dx*dx+dy*dy) as u32) as i32;
                    if d > ri && d <= ri+10 {
                        let a = ((ri+10-d) as u32 * 8).min(50);
                        let x = cxi+dx; let y = cyi+dy;
                        if x>=0 && y>=0 { self.blend(x as u32, y as u32, tint, a); }
                    }
                }
            }
        }

        // disco de fundo translucido dentro do anel (dá corpo à logo)
        for dy in -ri..=ri {
            for dx in -ri..=ri {
                let d2 = dx*dx+dy*dy;
                if d2 <= (ri-3)*(ri-3) {
                    let x = cxi+dx; let y = cyi+dy;
                    if x>=0 && y>=0 { self.blend(x as u32, y as u32, tint, 28); }
                }
            }
        }

        // anel externo brilhante (espessura ~3px, com AA)
        for dy in -ri..=ri {
            for dx in -ri..=ri {
                let d = isqrt((dx*dx+dy*dy) as u32) as i32;
                if d >= ri-3 && d <= ri {
                    let a = if d==ri || d==ri-3 { 160 } else { 255 };
                    let x = cxi+dx; let y = cyi+dy;
                    if x>=0 && y>=0 { self.blend(x as u32, y as u32, tint, a); }
                }
            }
        }

        // traçado de batimento (ECG) contínuo, dentro do anel.
        // Definimos pontos-chave e ligamos com linhas para nao fragmentar.
        let span = (ri*7)/10; // largura util do traçado
        // pontos (dx relativo, dy relativo ao centro): base, subida, pico,
        // vale profundo, recuperacao, base
        let pts: [(i32,i32); 7] = [
            (-span,        0),
            (-span/2,      0),
            (-span/4,   -ri/6),
            (0,         -ri/2),   // pico
            (span/6,     ri/3),   // vale
            (span/2,     0),
            (span,       0),
        ];
        for k in 0..pts.len()-1 {
            let (x0,y0) = pts[k];
            let (x1,y1) = pts[k+1];
            self.thick_line(cxi+x0, cyi+y0, cxi+x1, cyi+y1, 0x00FFFFFF, 2);
        }

        // ponto de luz no pico
        self.disc((cxi) as u32, (cyi - ri/2) as u32, 3, 0x00FFFFFF);
    }

    /// Linha com espessura (Bresenham + engrossamento vertical).
    pub fn thick_line(&self, x0:i32, y0:i32, x1:i32, y1:i32, color:u32, thick:i32) {
        let dx = (x1-x0).abs(); let sx = if x0<x1 {1} else {-1};
        let dy = -(y1-y0).abs(); let sy = if y0<y1 {1} else {-1};
        let mut err = dx+dy;
        let (mut x, mut y) = (x0,y0);
        loop {
            for t in 0..thick {
                if x>=0 && (y+t)>=0 { self.blend(x as u32,(y+t) as u32,color,255); }
            }
            if x==x1 && y==y1 { break; }
            let e2 = 2*err;
            if e2>=dy { err+=dy; x+=sx; }
            if e2<=dx { err+=dx; y+=sy; }
        }
    }

    // ============================================================
    // Texto anti-aliased usando atlas de fonte (Poppins).
    // Cada glifo e uma bitmap de alpha desenhada com blending.
    // ============================================================

    /// Desenha texto AA na fonte UI. (x,y) = canto superior esquerdo da
    /// caixa de texto; a baseline e calculada internamente. Retorna a
    /// largura desenhada (para centralizar/alinhar).
    pub fn text_ui(&self, x: i32, y: i32, s: &[u8], color: u32) -> i32 {
        self.draw_glyphs(x, y, s, color, &FONTUI_GLYPHS[..], &FONTUI_PIXELS[..], FONTUI_ASCENT, 255)
    }

    pub fn text_ui_a(&self, x: i32, y: i32, s: &[u8], color: u32, alpha: u32) -> i32 {
        self.draw_glyphs(x, y, s, color, &FONTUI_GLYPHS[..], &FONTUI_PIXELS[..], FONTUI_ASCENT, alpha)
    }

    /// Fonte grande (titulos, logo).
    pub fn text_big(&self, x: i32, y: i32, s: &[u8], color: u32) -> i32 {
        self.draw_glyphs(x, y, s, color, &FONTBIG_GLYPHS[..], &FONTBIG_PIXELS[..], FONTBIG_ASCENT, 255)
    }

    /// Largura de uma string na fonte UI (sem desenhar).
    pub fn text_ui_width(s: &[u8]) -> i32 {
        let mut w = 0i32;
        for &c in s {
            if c < 32 || c > 126 { w += 6; continue; }
            w += FONTUI_GLYPHS[(c - 32) as usize].adv as i32;
        }
        w
    }

    fn draw_glyphs(&self, x: i32, y: i32, s: &[u8], color: u32,
                   glyphs: &[Glyph], pixels: &[u8], ascent: i32, alpha: u32) -> i32 {
        let mut pen = x;
        for &c in s {
            if c < 32 || c > 126 {
                pen += 6;
                continue;
            }
            let g = glyphs[(c - 32) as usize];
            // posicao: pen + left, baseline (y + ascent) + top
            let gx = pen + g.left as i32;
            let gy = y + ascent + g.top as i32;
            let gw = g.w as i32;
            let data = &pixels[g.off..g.off + g.len];
            for row in 0..g.h as i32 {
                for col in 0..gw {
                    let a = data[(row * gw + col) as usize] as u32;
                    if a > 0 {
                        let px = gx + col;
                        let py = gy + row;
                        if px >= 0 && py >= 0 {
                            self.blend(px as u32, py as u32, color, a * alpha / 255);
                        }
                    }
                }
            }
            pen += g.adv as i32;
        }
        pen - x
    }
}

// (pulse_shape removida: a logo agora usa thick_line entre pontos-chave)

// ====================================================================
// Cursor com backing store (nao deixa rastro) + eventos de mouse.
// ====================================================================

pub struct Cursor {
    pub x: u32, pub y: u32,
    saved: [u32; 24*24],   // fundo salvo sob o cursor
    sx: u32, sy: u32, sw: u32, sh: u32,
    has_save: bool,
}

impl Cursor {
    pub const fn new(x:u32,y:u32) -> Self {
        Cursor { x,y, saved:[0;24*24], sx:0,sy:0,sw:0,sh:0, has_save:false }
    }
    /// Restaura o fundo onde o cursor estava (chamar antes de recompor).
    pub fn hide(&mut self, fb:&Fb) {
        if !self.has_save { return; }
        let mut i=0;
        for yy in 0..self.sh {
            for xx in 0..self.sw {
                let px=self.sx+xx; let py=self.sy+yy;
                if px<fb.w && py<fb.h {
                    unsafe { *fb.base.add((py*fb.w+px) as usize) = self.saved[i as usize]; }
                }
                i+=1;
            }
        }
        self.has_save=false;
    }
    /// Salva o fundo e desenha o cursor (chamar por ultimo, antes do present).
    pub fn show(&mut self, fb:&Fb) {
        let cw=16u32; let ch=22u32;
        self.sx=self.x; self.sy=self.y; self.sw=cw.min(fb.w.saturating_sub(self.x)); self.sh=ch.min(fb.h.saturating_sub(self.y));
        let mut i=0;
        for yy in 0..self.sh {
            for xx in 0..self.sw {
                let px=self.sx+xx; let py=self.sy+yy;
                self.saved[i as usize] = if px<fb.w&&py<fb.h { unsafe{*fb.base.add((py*fb.w+px) as usize)} } else {0};
                i+=1;
            }
        }
        self.has_save=true;
        // seta: preenchida branca com contorno preto
        draw_arrow(fb, self.x, self.y);
    }
    pub fn bounds(&self) -> Rect { Rect{x:self.x,y:self.y,w:16,h:22} }
}

fn draw_arrow(fb:&Fb, mx:u32, my:u32) {
    // silhueta de seta 12x18 (0=nada,1=borda,2=preenchimento)
    const A:[&[u8];18]=[
        b"2",b"22",b"232",b"2332",b"23332",b"233332",b"2333332",b"23333332",
        b"233333332",b"2333333332",b"23333333332",b"233333222",b"2332332",
        b"232 2332",b"22  2332",b"2    2332",b"      2332",b"       22"];
    for (row,line) in A.iter().enumerate() {
        for (col,&c) in line.iter().enumerate() {
            let color = match c { b'2'=>0x00202020u32, b'3'=>0x00FFFFFF, _=>continue };
            fb.blend(mx+col as u32, my+row as u32, color, 255);
        }
    }
}

/// Estado de mouse com deteccao de click e double-click.
pub struct Mouse {
    pub x:u32, pub y:u32,
    pub down:bool,
    prev_down:bool,
    pub clicked:bool,
    pub double:bool,
    pending_press:u32,
    last_click_tick:u32,
    last_x:u32, last_y:u32,
    pub dragging:bool,
}
impl Mouse {
    pub const fn new(x:u32,y:u32)->Self{
        Mouse{x,y,down:false,prev_down:false,clicked:false,double:false,pending_press:0,last_click_tick:0,last_x:0,last_y:0,dragging:false}
    }
    /// Processa um evento bruto. tick = contador global (para double-click).
    pub fn feed(&mut self, ev:&Event, w:u32, h:u32) {
        match ev.ev_type {
            EV_ABS => {
                if ev.code==ABS_X { self.x=(ev.value.min(32767)*(w-1))/32767; }
                else if ev.code==ABS_Y { self.y=(ev.value.min(32767)*(h-1))/32767; }
            }
            EV_KEY if ev.code==BTN_LEFT => {
                let was=self.down;
                self.down = ev.value==1;
                if self.down && !was { self.pending_press += 1; } // conta cada press
            }
            _=>{}
        }
    }
    /// Atualiza flags de click/double. `now_ms` = tempo real em ms.
    pub fn update(&mut self, now_ms:u64) {
        self.clicked=false; self.double=false;
        let presses=self.pending_press; self.pending_press=0;
        if presses>=2 {
            // dois ou mais press no mesmo frame = double-click imediato
            self.clicked=true; self.double=true;
            self.last_click_tick=now_ms as u32; self.last_x=self.x; self.last_y=self.y;
        } else if presses==1 {
            self.clicked=true;
            let delta=now_ms.wrapping_sub(self.last_click_tick as u64);
            if delta<800 && self.x.abs_diff(self.last_x)<10 && self.y.abs_diff(self.last_y)<10 {
                self.double=true;
            }
            self.last_click_tick=now_ms as u32; self.last_x=self.x; self.last_y=self.y;
        }
        if !self.down && self.prev_down { self.dragging=false; }
        self.prev_down=self.down;
    }
    pub fn pressed(&self)->bool { self.down && !self.prev_down }
    pub fn released(&self)->bool { !self.down && self.prev_down }
}

// ====================================================================
// Superficies por cliente. Cada app desenha na SUA superficie (buffer
// privado); o WM le e compoe. Isso isola o desenho dos apps entre si.
// ====================================================================

pub const SURF_W: u32 = 512;
pub const SURF_H: u32 = 512;

/// Mapeia a superficie do slot `slot` e retorna um Fb que desenha nela.
/// A superficie tem SURF_W x SURF_H; o app usa a sub-regiao que quiser.
pub fn surface_open(slot: u64) -> Fb {
    let addr = syscall2(21, slot, 0); // SYS_SURF_MAP
    Fb { base: addr as *mut u32, w: SURF_W, h: SURF_H, cx0:0,cy0:0,cx1:SURF_W,cy1:SURF_H }
}

/// Protocolo WM<->app via IPC (service SVC_WM). O app pergunta ao WM:
/// - qual meu slot de superficie?
/// - qual a geometria da minha area cliente (w,h)?
/// - devo fechar? (o WM sinaliza fechamento pelo semaforo)
pub const SVC_WM: u64 = 2;
pub const WM_OP_HELLO: u64 = 1;   // app->wm: registro. data[0]=app_kind, data[1]=w_desejada, data[2]=h_desejada. resp: data[0]=slot, data[1]=w, data[2]=h
pub const WM_OP_COMMIT: u64 = 2;  // app->wm: "desenhei, componha". data[0]=slot
pub const WM_OP_POLL: u64 = 3;
// Acoes de menu enviadas pelo WM ao app em foco (WmEvent.menu):
pub const APP_MENU_SAVE:u16=1;
pub const APP_MENU_NEW:u16=2;
pub const APP_MENU_COPY:u16=3;
pub const APP_MENU_PASTE:u16=4;
pub const APP_MENU_UNDO:u16=5;
pub const APP_MENU_CLEAR:u16=6;
pub const APP_MENU_PAUSE:u16=7;
pub const APP_MENU_RESET:u16=8;    // app->wm: eventos. resp: data[0]=flags, data[1]=mouse_x_local, data[2]=mouse_y_local, data[3]=mouse_flags(bit0=down,bit1=clicked), data[4]=key(evdev code se tecla)

pub const WM_FLAG_CLOSE: u64 = 1;
pub const WM_FLAG_FOCUS: u64 = 2;
pub const WM_MOUSE_DOWN: u64 = 1;
pub const WM_MOUSE_CLICK: u64 = 2;

/// Evento que o app recebe do WM via POLL.
pub struct WmEvent {
    pub close: bool,
    pub focused: bool,
    pub mx: i32, pub my: i32,     // mouse local (na area cliente), -1 se fora
    pub click: bool,             // clicou neste frame dentro da area
    pub key: u16,                // tecla evdev (0 = nenhuma)
    pub menu: u16,               // acao de menu da barra (0 = nenhuma)
}

/// Faz POLL ao WM e decodifica os eventos.
pub fn wm_poll(wm: usize) -> WmEvent {
    let mut m = Msg::new(WM_OP_POLL);
    send(wm, &mut m);
    let mf = m.data[3];
    WmEvent {
        close: m.data[0] & WM_FLAG_CLOSE != 0,
        focused: m.data[0] & WM_FLAG_FOCUS != 0,
        mx: m.data[1] as i32, my: m.data[2] as i32,
        click: mf & WM_MOUSE_CLICK != 0,
        key: m.data[4] as u16,
        menu: m.data[5] as u16,
    }
}

/// HELLO com tamanho desejado + titulo (ate 24 bytes em data[3..6]).
pub fn wm_hello(wm: usize, app_kind: u64, want_w: u32, want_h: u32) -> (u64, u32, u32) {
    let mut m = Msg::new(WM_OP_HELLO);
    m.data[0] = app_kind; m.data[1] = want_w as u64; m.data[2] = want_h as u64;
    send(wm, &mut m);
    (m.data[0], m.data[1] as u32, m.data[2] as u32)
}

/// COMMIT: avisa o WM que desenhou.
pub fn wm_commit(wm: usize, slot: u64) {
    let mut m = Msg::new(WM_OP_COMMIT); m.data[0] = slot;
    send(wm, &mut m);
}
