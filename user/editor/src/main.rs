//! Editor de texto do Pulsar OS. Abre o arquivo indicado pelo WM (duplo
//! clique no Finder) ou welcome.txt; cursor com setas/Home/End/PgUp/PgDn,
//! texto UTF-8 com acentos, numeros de linha, desfazer e Ctrl+S.
#![no_std]
#![no_main]
use plib::*;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    write("[editor] PANIC\n");
    exit()
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let ed = unsafe { &mut *(&raw mut ED) };
    ed.run();
    exit()
}

const MAX: usize = 16384;

struct Editor {
    app: Option<App>,
    buf: [u8; MAX],
    len: usize,
    undo: [u8; MAX],
    undo_len: usize,
    undo_cur: usize,
    last_edit: u64,
    cur: usize,
    want_col: i32,
    top: usize,
    left: i32,
    file: Option<usize>,
    name: [u8; 28],
    nlen: usize,
    dirty: bool,
    msg: Buf<64>,
    msg_until: u64,
    clip: [u8; 512],
    clip_len: usize,
    line_numbers: bool,
}

static mut ED: Editor = Editor {
    app: None, buf: [0; MAX], len: 0, undo: [0; MAX], undo_len: 0, undo_cur: 0, last_edit: 0,
    cur: 0, want_col: -1, top: 0, left: 0, file: None, name: [0; 28], nlen: 0, dirty: false,
    msg: Buf::new(), msg_until: 0, clip: [0; 512], clip_len: 0, line_numbers: true,
};

impl Editor {
    fn app(&mut self) -> &mut App { self.app.as_mut().unwrap() }

    fn run(&mut self) {
        let app = App::new(1, b"Editor", 620, 440);
        self.app = Some(app);
        let f = self.app().open_file.or_else(|| find_file(b"welcome.txt"));
        if let Some(i) = f { self.open(i); }
        self.update_title();
        let mut caret = true;
        let mut blink = uptime_ms();
        self.draw(caret);
        let mut ev = self.app().commit();
        loop {
            if ev.close { exit(); }
            let mut redraw = ev.changed;
            if let Some(k) = ev.key {
                self.key(k);
                caret = true;
                blink = uptime_ms();
                redraw = true;
            }
            if ev.menu != 0 { self.menu(ev.menu); redraw = true; }
            if ev.click { self.click(ev.mx, ev.my); caret = true; redraw = true; }
            if ev.wheel != 0 {
                let lines = self.line_count();
                self.top = (self.top as i32 - ev.wheel * 3).clamp(0, lines.saturating_sub(1) as i32) as usize;
                redraw = true;
            }
            let now = uptime_ms();
            if now - blink > 530 { caret = !caret; blink = now; redraw = true; }
            if self.msg_until != 0 && now > self.msg_until { self.msg_until = 0; redraw = true; }
            if ev.more { ev = self.app().poll(); continue; }
            ev = if redraw { self.draw(caret); self.app().commit() } else { self.app().poll() };
        }
    }

    fn open(&mut self, idx: usize) {
        let Some(fi) = file_info(idx) else { return };
        let n = fs_read(idx, &mut self.buf);
        self.len = if n > 0 { (n as usize).min(MAX) } else { 0 };
        // remove \r e bytes nulos
        let mut w = 0;
        for r in 0..self.len {
            let c = self.buf[r];
            if c != b'\r' && c != 0 { self.buf[w] = c; w += 1; }
        }
        self.len = w;
        self.file = Some(idx);
        self.name = fi.name;
        self.nlen = fi.nlen;
        self.cur = 0;
        self.top = 0;
        self.dirty = false;
        self.snapshot();
    }

    fn update_title(&mut self) {
        let mut t = Buf::<24>::new();
        if self.nlen > 0 { t.bytes(&self.name[..self.nlen.min(18)]); } else { t.s("Sem título"); }
        if self.dirty { t.s(" •"); }
        let t = t;
        self.app().set_title(t.get());
    }

    fn flash(&mut self, s: &str) {
        self.msg.clear().s(s);
        self.msg_until = uptime_ms() + 2500;
    }

    fn save(&mut self) {
        if self.file.is_none() {
            let mut name = Buf::<28>::new();
            for k in 1..100u64 {
                name.clear().s("Sem titulo");
                if k > 1 { name.s(" ").u(k); }
                name.s(".txt");
                if find_file(name.get()).is_none() { break; }
            }
            let idx = fs_create(name.get());
            if idx < 0 { return self.flash("Não foi possível criar o arquivo"); }
            self.file = Some(idx as usize);
            self.nlen = name.n;
            self.name[..name.n].copy_from_slice(name.get());
        }
        let r = fs_write(self.file.unwrap(), &self.buf[..self.len]);
        if r == 0 {
            self.dirty = false;
            self.flash("Salvo");
        } else {
            self.flash("Arquivo cheio: capacidade do disco excedida");
        }
        self.update_title();
    }

    fn snapshot(&mut self) {
        self.undo[..self.len].copy_from_slice(&self.buf[..self.len]);
        self.undo_len = self.len;
        self.undo_cur = self.cur;
    }

    /// Agrupa edicoes: um ponto de desfazer a cada pausa de 1 s.
    fn before_edit(&mut self) {
        let now = uptime_ms();
        if now - self.last_edit > 1000 { self.snapshot(); }
        self.last_edit = now;
        if !self.dirty {
            self.dirty = true;
            self.update_title();
        }
    }

    fn insert(&mut self, s: &[u8]) {
        if self.len + s.len() > MAX { return self.flash("Limite de 16 KB atingido"); }
        self.before_edit();
        self.buf.copy_within(self.cur..self.len, self.cur + s.len());
        self.buf[self.cur..self.cur + s.len()].copy_from_slice(s);
        self.len += s.len();
        self.cur += s.len();
        self.want_col = -1;
    }

    fn delete_range(&mut self, a: usize, b: usize) {
        if a >= b { return; }
        self.before_edit();
        self.buf.copy_within(b..self.len, a);
        self.len -= b - a;
        self.cur = a;
        self.want_col = -1;
    }

    fn prev_char(&self, i: usize) -> usize {
        let mut j = i.saturating_sub(1);
        while j > 0 && self.buf[j] & 0xC0 == 0x80 { j -= 1; }
        j
    }

    fn next_char(&self, i: usize) -> usize {
        let mut j = (i + 1).min(self.len);
        while j < self.len && self.buf[j] & 0xC0 == 0x80 { j += 1; }
        j
    }

    fn line_start(&self, i: usize) -> usize {
        self.buf[..i].iter().rposition(|&c| c == b'\n').map(|p| p + 1).unwrap_or(0)
    }

    fn line_end(&self, i: usize) -> usize {
        self.buf[i..self.len].iter().position(|&c| c == b'\n').map(|p| i + p).unwrap_or(self.len)
    }

    fn line_count(&self) -> usize {
        self.buf[..self.len].iter().filter(|&&c| c == b'\n').count() + 1
    }

    fn line_of(&self, i: usize) -> usize {
        self.buf[..i].iter().filter(|&&c| c == b'\n').count()
    }

    /// Coluna (em caracteres) do indice `i`.
    fn col_of(&self, i: usize) -> i32 {
        text::char_count(&self.buf[self.line_start(i)..i]) as i32
    }

    /// Indice na linha que comeca em `ls` correspondente a coluna `col`.
    fn at_col(&self, ls: usize, col: i32) -> usize {
        let le = self.line_end(ls);
        let mut i = ls;
        let mut c = 0;
        while i < le && c < col { i = self.next_char(i); c += 1; }
        i
    }

    fn vertical(&mut self, d: i32) {
        if self.want_col < 0 { self.want_col = self.col_of(self.cur); }
        let mut ls = self.line_start(self.cur);
        if d < 0 {
            for _ in 0..-d {
                if ls == 0 { break; }
                ls = self.line_start(ls - 1);
            }
        } else {
            for _ in 0..d {
                let le = self.line_end(ls);
                if le >= self.len { break; }
                ls = le + 1;
            }
        }
        self.cur = self.at_col(ls, self.want_col);
    }

    fn key(&mut self, k: Key) {
        if k.ctrl() {
            match k.code {
                key::S => self.save(),
                key::Z => self.menu(APP_MENU_UNDO),
                key::C => self.menu(APP_MENU_COPY),
                key::V => self.menu(APP_MENU_PASTE),
                key::N => self.menu(APP_MENU_NEW),
                key::A => { self.cur = self.len; }
                key::HOME => self.cur = 0,
                key::END => self.cur = self.len,
                _ => {}
            }
            return;
        }
        let page = self.visible_lines() as i32 - 1;
        match k.code {
            key::LEFT => { self.cur = self.prev_char(self.cur); self.want_col = -1; }
            key::RIGHT => { self.cur = self.next_char(self.cur); self.want_col = -1; }
            key::UP => self.vertical(-1),
            key::DOWN => self.vertical(1),
            key::PGUP => self.vertical(-page),
            key::PGDN => self.vertical(page),
            key::HOME => { self.cur = self.line_start(self.cur); self.want_col = -1; }
            key::END => { self.cur = self.line_end(self.cur); self.want_col = -1; }
            key::BACKSPACE => {
                if self.cur > 0 { let p = self.prev_char(self.cur); self.delete_range(p, self.cur); }
            }
            key::DELETE => {
                if self.cur < self.len { let n = self.next_char(self.cur); self.delete_range(self.cur, n); }
            }
            key::ENTER | key::KPENTER => {
                // mantem a indentacao da linha atual
                let ls = self.line_start(self.cur);
                let ind = self.buf[ls..self.cur].iter().take_while(|&&c| c == b' ').count().min(32);
                let mut t = [b' '; 33];
                t[0] = b'\n';
                self.insert(&t[..ind + 1]);
            }
            key::TAB => self.insert(b"    "),
            key::ESC => {}
            _ => {
                if k.ch >= 32 {
                    let mut t = [0u8; 4];
                    let n = utf8_encode(k.ch, &mut t);
                    self.insert(&t[..n]);
                }
            }
        }
    }

    fn menu(&mut self, code: u16) {
        match code {
            APP_MENU_SAVE => self.save(),
            APP_MENU_NEW => {
                self.len = 0;
                self.cur = 0;
                self.top = 0;
                self.file = None;
                self.nlen = 0;
                self.dirty = false;
                self.snapshot();
                self.update_title();
            }
            APP_MENU_UNDO => {
                let (ul, uc) = (self.undo_len, self.undo_cur);
                // troca buffer <-> desfazer (desfazer de novo = refazer)
                for i in 0..ul.max(self.len) { let a = self.buf[i]; self.buf[i] = self.undo[i]; self.undo[i] = a; }
                self.undo_len = self.len;
                self.undo_cur = self.cur;
                self.len = ul;
                self.cur = uc.min(ul);
                self.dirty = true;
                self.update_title();
                self.flash("Desfeito");
            }
            APP_MENU_COPY => {
                let (a, b) = (self.line_start(self.cur), self.line_end(self.cur));
                let n = (b - a).min(511);
                self.clip[..n].copy_from_slice(&self.buf[a..a + n]);
                self.clip[n] = b'\n';
                self.clip_len = n + 1;
                self.flash("Linha copiada");
            }
            APP_MENU_PASTE => {
                if self.clip_len > 0 {
                    let mut t = [0u8; 512];
                    t[..self.clip_len].copy_from_slice(&self.clip[..self.clip_len]);
                    let n = self.clip_len;
                    self.insert(&t[..n]);
                }
            }
            APP_MENU_SELECT_ALL => self.cur = self.len,
            APP_MENU_ZOOM_IN | APP_MENU_ZOOM_OUT => self.line_numbers = !self.line_numbers,
            _ => {}
        }
    }

    fn gutter(&self) -> i32 {
        if self.line_numbers { 48 } else { 12 }
    }

    fn visible_lines(&mut self) -> usize {
        let h = self.app().h;
        ((h - 28 - 8) / line_h()).max(1) as usize
    }

    fn click(&mut self, mx: i32, my: i32) {
        let row = ((my - 6) / line_h()).max(0) as usize + self.top;
        let mut ls = 0;
        for _ in 0..row {
            let le = self.line_end(ls);
            if le >= self.len { break; }
            ls = le + 1;
        }
        let x = mx - self.gutter() - 6 + self.left;
        let le = self.line_end(ls);
        let mut i = ls;
        let mut px = 0;
        while i < le {
            let n = self.next_char(i);
            let mut j = 0;
            let cp = text::next_char(&self.buf[i..n], &mut j);
            let w = FONT_MONO.char_width(cp);
            if px + w / 2 > x { break; }
            px += w;
            i = n;
        }
        self.cur = i;
        self.want_col = -1;
    }

    fn draw(&mut self, caret: bool) {
        let vis = self.visible_lines();
        // rolagem para manter o cursor visivel
        let cl = self.line_of(self.cur);
        if cl < self.top { self.top = cl; }
        if cl >= self.top + vis { self.top = cl + 1 - vis; }
        let app = self.app.as_ref().unwrap();
        let (fb, th, w, h) = (app.fb, app.theme, app.w, app.h);
        let lh = line_h();
        let g = self.gutter();
        let text_w = w - g - 12;
        // rolagem horizontal
        let cx_line = FONT_MONO.width(&self.buf[self.line_start(self.cur)..self.cur]);
        if cx_line - self.left > text_w - 16 { self.left = cx_line - text_w + 48; }
        if cx_line < self.left { self.left = (cx_line - 48).max(0); }

        fb.fill(Rect::new(0, 0, w, h), th.win_bg);
        if self.line_numbers {
            fb.fill(Rect::new(0, 0, g - 6, h - 28), th.side_bg);
        }
        let mut ls = 0usize;
        for _ in 0..self.top {
            let le = self.line_end(ls);
            if le >= self.len { break; }
            ls = le + 1;
        }
        let mut body = fb;
        body.set_clip(Rect::new(g, 0, w - g, h - 28));
        for row in 0..vis {
            let ln = self.top + row;
            let y = 6 + row as i32 * lh;
            let le = self.line_end(ls);
            if ln == cl {
                fb.fill_a(Rect::new(g - 6, y - 1, w - g + 6, lh), th.accent, if th.dark { 30 } else { 18 });
            }
            if self.line_numbers {
                let mut b = Buf::<8>::new();
                b.u(ln as u64 + 1);
                fb.text_right(&FONT_MONO, g - 14, y, b.get(), if ln == cl { th.text2 } else { th.text3 });
            }
            body.text(&FONT_MONO, g - self.left, y, &self.buf[ls..le], th.text);
            if ln == cl && caret && app.focused {
                let x = g - self.left + FONT_MONO.width(&self.buf[ls..self.cur]);
                body.fill(Rect::new(x, y, 2, lh - 2), th.accent);
            }
            if le >= self.len { break; }
            ls = le + 1;
        }
        // barra de status
        let sb = Rect::new(0, h - 28, w, 28);
        fb.fill(sb, th.side_bg);
        fb.hline(0, h - 28, w, th.sep, 255);
        let mut s = Buf::<64>::new();
        s.s("Linha ").u(cl as u64 + 1).s(", Coluna ").u(self.col_of(self.cur) as u64 + 1);
        fb.text(&FONT_SMALL, 12, h - 22, s.get(), th.text2);
        let mut r = Buf::<64>::new();
        if self.msg_until != 0 { r.bytes(self.msg.get()); }
        else {
            r.u(text::char_count(&self.buf[..self.len]) as u64).s(" caracteres · UTF-8 · ");
            r.s(if self.dirty { "Não salvo" } else { "Salvo" });
        }
        fb.text_right(&FONT_SMALL, w - 12, h - 22, r.get(),
            if self.msg_until != 0 { th.accent } else { th.text2 });
    }
}

fn line_h() -> i32 {
    FONT_MONO.height + 3
}
