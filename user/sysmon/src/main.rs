//! Monitor do Sistema — o "gerenciador de tarefas" do Pulsar OS.
//! Aba Desempenho: uso de cada nucleo (grafico dos ultimos 60 s) e memoria.
//! Aba Processos: lista com pid, estado, nucleo e uso de CPU; o botao
//! "Encerrar processo" (ou Delete) mata o processo selecionado.
#![no_std]
#![no_main]
use plib::*;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { exit() }

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let m = unsafe { &mut *(&raw mut MON) };
    m.run();
    exit()
}

const HIST: usize = 120; // amostras a cada 500 ms = 60 s
const MAXP: usize = 16;
const CPU_COLORS: [u32; 8] = [0x0A84FF, 0x30D158, 0xFF9F0A, 0xBF5AF2, 0xFF375F, 0x64D2FF, 0xFFD60A, 0x8E8E93];

struct Mon {
    tab: u8,
    hist: [[u8; HIST]; 8],
    hpos: usize,
    ncpu: usize,
    procs: [ProcInfo; MAXP],
    pct: [u32; MAXP],
    prev_ms: [(u64, u64); MAXP],
    np: usize,
    sel_pid: u64,
    sys: SysInfo,
    last_sample: u64,
    msg: Buf<64>,
}

static mut MON: Mon = Mon {
    tab: 0, hist: [[0; HIST]; 8], hpos: 0, ncpu: 1, procs: [ProcInfo::zero(); MAXP], pct: [0; MAXP],
    prev_ms: [(0, 0); MAXP], np: 0, sel_pid: u64::MAX,
    sys: SysInfo { arch_id: 0, ncpus: 0, mem_total_kb: 0, mem_free_kb: 0, fb_w: 0, fb_h: 0, uptime_ms: 0, arch_name: [0; 16] },
    last_sample: 0, msg: Buf::new(),
};

impl Mon {
    fn sample(&mut self) {
        self.sys = sysinfo();
        self.ncpu = (self.sys.ncpus as usize).clamp(1, 8);
        for c in 0..8 {
            self.hist[c][self.hpos] = cpu_load(c).unwrap_or(0).min(100) as u8;
        }
        self.hpos = (self.hpos + 1) % HIST;
        let now = uptime_ms();
        let mut n = 0;
        let mut slot = 0;
        while let Ok(p) = proc_info(slot) {
            if let Some(p) = p {
                if n < MAXP {
                    // %CPU = delta do tempo de CPU / delta de tempo real
                    let prev = self.prev_ms.iter().find(|q| q.0 == p.pid + 1).copied();
                    let pct = match prev {
                        Some((_, pm)) => {
                            let dt = now.saturating_sub(self.last_sample).max(1);
                            ((p.cpu_ms.saturating_sub(pm)) * 100 / dt).min(100) as u32
                        }
                        None => 0,
                    };
                    self.procs[n] = p;
                    self.pct[n] = pct;
                    n += 1;
                }
            }
            slot += 1;
        }
        self.np = n;
        for i in 0..MAXP {
            self.prev_ms[i] = if i < n { (self.procs[i].pid + 1, self.procs[i].cpu_ms) } else { (0, 0) };
        }
        // ordena por uso de CPU (desc), depois pid
        for a in 1..n {
            let mut b = a;
            while b > 0 && (self.pct[b] > self.pct[b - 1] || (self.pct[b] == self.pct[b - 1] && self.procs[b].pid < self.procs[b - 1].pid)) {
                self.procs.swap(b, b - 1);
                self.pct.swap(b, b - 1);
                b -= 1;
            }
        }
        self.last_sample = now;
    }

    fn kill_selected(&mut self) {
        if self.sel_pid == u64::MAX { return; }
        let me = getpid() as u64;
        self.msg.clear();
        if self.sel_pid == me {
            self.msg.s("Use o botão fechar para sair do Monitor.");
        } else if self.procs[..self.np].iter().any(|p| p.pid == self.sel_pid && (p.name() == b"shell" || p.name() == b"fsd")) {
            self.msg.s("Processo essencial do sistema: não encerrado.");
        } else if kill(self.sel_pid as usize) {
            self.msg.s("Processo ").u(self.sel_pid).s(" encerrado.");
            self.sel_pid = u64::MAX;
        } else {
            self.msg.s("Não foi possível encerrar.");
        }
    }

    fn run(&mut self) {
        let mut app = App::new(8, "Monitor do Sistema".as_bytes(), 620, 440);
        self.sample();
        self.tab = 1;
        self.draw(&app, -1, -1);
        let mut ev = app.commit();
        let (mut lmx, mut lmy) = (-1, -1);
        loop {
            if ev.close { exit(); }
            let mut redraw = ev.changed;
            if ev.click {
                redraw = true;
                let (mx, my) = (ev.mx, ev.my);
                let seg = Rect::new(app.w / 2 - 110, 12, 220, 28);
                if seg.contains(mx, my) { self.tab = if mx < seg.x + 110 { 0 } else { 1 }; }
                if self.tab == 1 {
                    let row = (my - 84) / 26;
                    if my >= 84 && (row as usize) < self.np && my < app.h - 56 {
                        self.sel_pid = self.procs[row as usize].pid;
                    }
                    if kill_btn(&app).contains(mx, my) { self.kill_selected(); self.sample(); }
                }
            }
            if let Some(k) = ev.key {
                redraw = true;
                match k.code {
                    key::DELETE => { self.kill_selected(); self.sample(); }
                    key::TAB => self.tab ^= 1,
                    key::DOWN | key::UP => {
                        let cur = self.procs[..self.np].iter().position(|p| p.pid == self.sel_pid);
                        let i = match (cur, k.code) {
                            (None, _) => 0,
                            (Some(c), key::DOWN) => (c + 1).min(self.np.saturating_sub(1)),
                            (Some(c), _) => c.saturating_sub(1),
                        };
                        if self.np > 0 { self.sel_pid = self.procs[i].pid; }
                    }
                    _ => {}
                }
            }
            match ev.menu {
                APP_MENU_CLEAR => { self.kill_selected(); self.sample(); redraw = true; }
                APP_MENU_RELOAD => { self.sample(); redraw = true; }
                _ => {}
            }
            if kill_btn(&app).contains(ev.mx, ev.my) != kill_btn(&app).contains(lmx, lmy) { redraw = true; }
            lmx = ev.mx;
            lmy = ev.my;
            if uptime_ms() - self.last_sample >= 500 { self.sample(); redraw = true; }
            if ev.more { ev = app.poll(); continue; }
            ev = if redraw { self.draw(&app, ev.mx, ev.my); app.commit() } else { app.poll() };
        }
    }

    fn draw(&self, app: &App, mx: i32, my: i32) {
        let fb = app.fb;
        let th = &app.theme;
        let (w, h) = (app.w, app.h);
        fb.fill(Rect::new(0, 0, w, h), if th.dark { 0x1C1D21 } else { 0xF5F5F7 });
        // abas
        let seg = Rect::new(w / 2 - 110, 12, 220, 28);
        fb.round_rect(seg, 8, if th.dark { 0x2C2D33 } else { 0xE4E4E9 });
        for (k, l) in ["Desempenho", "Processos"].iter().enumerate() {
            let r = Rect::new(seg.x + k as i32 * 110, seg.y, 110, 28);
            if self.tab == k as u8 {
                fb.shadow(r.inset(2), 6, 3, 1, 40);
                fb.round_rect(r.inset(2), 6, if th.dark { 0x5A5B62 } else { 0xFFFFFF });
            }
            fb.text_center(&FONT_UI, r, l.as_bytes(), th.text);
        }
        if self.tab == 0 { self.draw_perf(&fb, th, w, h); } else { self.draw_procs(&fb, th, w, h, mx, my); }
    }

    fn draw_perf(&self, fb: &Fb, th: &Theme, w: i32, h: i32) {
        let card = Rect::new(16, 54, w - 32, h - 54 - 96);
        fb.round_rect(card, 10, if th.dark { 0x26272D } else { 0xFFFFFF });
        let mut t = Buf::<64>::new();
        t.s("Uso de CPU · ").u(self.ncpu as u64).s(" núcleos ").bytes(self.sys.arch());
        fb.text(&FONT_UI_BOLD, card.x + 14, card.y + 10, t.get(), th.text);
        let g = Rect::new(card.x + 14, card.y + 38, card.w - 28, card.h - 76);
        for k in 0..=4 {
            fb.hline(g.x, g.y + g.h * k / 4, g.w, th.sep, 255);
        }
        for c in 0..self.ncpu {
            let col = CPU_COLORS[c];
            let mut prev: Option<(i32, i32)> = None;
            for s in 0..HIST {
                let v = self.hist[c][(self.hpos + s) % HIST] as i32;
                let x = g.x + g.w * s as i32 / (HIST as i32 - 1);
                let y = g.bottom() - g.h * v / 100;
                if let Some((px, py)) = prev { fb.line(px, py, x, y, 2, col, 230); }
                prev = Some((x, y));
            }
        }
        // legenda
        for c in 0..self.ncpu {
            let lx = card.x + 14 + (c as i32 % 4) * ((card.w - 28) / 4);
            let ly = card.bottom() - 32 + (c as i32 / 4) * 0;
            let cur = self.hist[c][(self.hpos + HIST - 1) % HIST];
            fb.disc(lx + 5, ly + 9, 5, CPU_COLORS[c], 255);
            let mut b = Buf::<16>::new();
            b.s("CPU ").u(c as u64).s("  ").u(cur as u64).s("%");
            fb.text(&FONT_SMALL, lx + 16, ly + 1, b.get(), th.text2);
        }
        // memoria
        let mc = Rect::new(16, h - 84, w - 32, 68);
        fb.round_rect(mc, 10, if th.dark { 0x26272D } else { 0xFFFFFF });
        let used = self.sys.mem_total_kb.saturating_sub(self.sys.mem_free_kb) as u64;
        let mut b = Buf::<64>::new();
        b.s("Memória: ").size(used * 1024).s(" de ").size(self.sys.mem_total_kb as u64 * 1024);
        fb.text(&FONT_UI_BOLD, mc.x + 14, mc.y + 10, b.get(), th.text);
        let bar = Rect::new(mc.x + 14, mc.y + 40, mc.w - 28, 12);
        fb.round_rect(bar, 6, if th.dark { 0x3A3B41 } else { 0xE5E5EA });
        let frac = (used * bar.w as u64 / (self.sys.mem_total_kb as u64).max(1)) as i32;
        fb.round_rect(Rect::new(bar.x, bar.y, frac.max(12), 12), 6, th.accent);
        let up = self.sys.uptime_ms / 1000;
        let mut u = Buf::<32>::new();
        u.s("Ativo há ").u(up / 3600).s("h ").u0((up / 60) % 60, 2).s("min");
        fb.text_right(&FONT_SMALL, mc.right() - 14, mc.y + 12, u.get(), th.text2);
    }

    fn draw_procs(&self, fb: &Fb, th: &Theme, w: i32, h: i32, mx: i32, my: i32) {
        let card = Rect::new(16, 54, w - 32, h - 54 - 56);
        fb.round_rect(card, 10, if th.dark { 0x26272D } else { 0xFFFFFF });
        let cols = [(card.x + 16, "PID"), (card.x + 64, "Nome"), (card.x + 200, "Estado"),
            (card.x + 336, "Núcleo"), (card.x + 400, "% CPU"), (card.x + card.w - 64, "Tempo")];
        for (x, l) in cols.iter() { fb.text(&FONT_SMALL, *x, card.y + 8, l.as_bytes(), th.text2); }
        fb.hline(card.x, card.y + 28, card.w, th.sep, 255);
        let states = ["?", "Rodando", "Pronto", "Dormindo", "Aguardando IPC"];
        for i in 0..self.np {
            let p = &self.procs[i];
            let y = 84 + i as i32 * 26;
            if y + 26 > card.bottom() { break; }
            let row = Rect::new(card.x + 6, y, card.w - 12, 26);
            let sel = p.pid == self.sel_pid;
            if sel { fb.round_rect(row, 6, th.accent); }
            else if row.contains(mx, my) { fb.round_rect(row, 6, th.hover); }
            let tc = if sel { 0xFFFFFF } else { th.text };
            let tc2 = if sel { 0xE6EEFF } else { th.text2 };
            let mut b = Buf::<24>::new();
            fb.text(&FONT_UI, cols[0].0, y + 4, b.clear().u(p.pid).get(), tc);
            fb.text(&FONT_UI, cols[1].0, y + 4, p.name(), tc);
            let st = states[p.state.min(4) as usize];
            if p.state == 1 { fb.disc(cols[2].0 - 8, y + 13, 3, if sel { 0xFFFFFF } else { 0x30D158 }, 255); }
            fb.text(&FONT_UI, cols[2].0, y + 4, st.as_bytes(), tc2);
            fb.text(&FONT_UI, cols[3].0, y + 4, b.clear().u(p.cpu as u64).get(), tc2);
            let pct = self.pct[i];
            let bar = Rect::new(cols[4].0, y + 9, 40, 8);
            fb.round_rect(bar, 4, if sel { 0x5AA2FF } else if th.dark { 0x3A3B41 } else { 0xE5E5EA });
            if pct > 0 { fb.round_rect(Rect::new(bar.x, bar.y, (40 * pct as i32 / 100).max(6), 8), 4, if sel { 0xFFFFFF } else { th.accent }); }
            fb.text(&FONT_SMALL, cols[4].0 + 46, y + 6, b.clear().u(pct as u64).s("%").get(), tc2);
            let s = p.cpu_ms / 1000;
            fb.text(&FONT_UI, cols[5].0, y + 4, b.clear().u(s / 60).s(":").u0(s % 60, 2).get(), tc2);
        }
        let kb = kill_btn_rect(w, h);
        let enabled = self.sel_pid != u64::MAX;
        if enabled {
            fb.round_rect(kb, 7, if kb.contains(mx, my) { 0xFF5A50 } else { 0xFF453A });
        } else {
            fb.round_rect(kb, 7, if th.dark { 0x3A3B41 } else { 0xE5E5EA });
        }
        fb.text_center(&FONT_UI_BOLD, kb, "Encerrar processo".as_bytes(), if enabled { 0xFFFFFF } else { th.text3 });
        let mut info = Buf::<64>::new();
        if self.msg.n > 0 { info.bytes(self.msg.get()); } else { info.u(self.np as u64).s(" processos · Delete encerra o selecionado"); }
        fb.text(&FONT_SMALL, 20, h - 38, info.get(), th.text2);
    }
}

fn kill_btn_rect(w: i32, h: i32) -> Rect { Rect::new(w - 186, h - 46, 170, 32) }
fn kill_btn(app: &App) -> Rect { kill_btn_rect(app.w, app.h) }
