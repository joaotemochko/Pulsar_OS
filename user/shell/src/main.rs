//! Pulsar Desktop — compositor e gerenciador de janelas do Pulsar OS.
//!
//! * aparencia macOS: barra de menus e dock de vidro fosco, semaforos,
//!   sombras suaves, ampliacao do dock, modo claro/escuro;
//! * facilidades do Windows: Alt+Tab, encaixe de janelas (Aero Snap),
//!   menus de contexto, gerenciador de tarefas (Ctrl+Shift+Esc),
//!   "forcar encerramento" de apps travados, busca universal;
//! * customizacao do Linux: tudo em `pulsar.cfg` (texto puro), editavel
//!   no Editor e aplicado ao vivo; papeis de parede, cores, layout.
//!
//! Desempenho: so a area danificada e recomposta (com descarte de janelas
//! totalmente cobertas); o cursor e de hardware (mover o mouse nao redesenha
//! nada); o present roda fora da trava do kernel; os apps rodam em paralelo
//! nos outros nucleos e so pedem quadros quando algo muda.
#![no_std]
#![no_main]

mod catalog;
mod config;
mod cursor;
mod panels;
mod sprites;
mod ui;
mod wallpaper;

use catalog::*;
use config::Config;
use panels::{Ctx, Panels, THUMB_H, THUMB_W, TOOLBAR_H};
use plib::icons::{self, Icon, Sym};
use plib::*;
use ui::Hits;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    write("[wm] PANIC\n");
    exit()
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let wm = unsafe { &mut *(&raw mut WM) };
    wm.run()
}

const MENU_H: i32 = 28;
const TITLE_H: i32 = 38;
const WIN_RAD: i32 = 10;
const MAXWIN: usize = 12;
const KQ: usize = 32;
const FRAME_MS: u64 = 16;
const MAX_FILES: usize = 64;
const RESIZE_M: i32 = 6;
/// Prazo maximo de um POLL estacionado (apps acordam ~4x/s quando ociosos).
const POLL_MAX_MS: u64 = 250;
/// Imprime na serial o custo medio de composicao/apresentacao.
const PERF_LOG: bool = false;
static mut PERF: (u64, u64, u64, u64) = (0, 0, 0, 0); // quadros, ms compondo, ms apresentando, px
static mut PHASE: [u64; 6] = [0; 6]; // ms: fundo, janelas, dock, resto; desenhos do dock, composicoes
/// Relogio das medicoes: so custa um syscall com PERF_LOG ligado.
#[inline(always)]
fn perf_ms() -> u64 { if PERF_LOG { uptime_ms() } else { 0 } }

#[derive(Clone, Copy, PartialEq, Eq)]
enum WState { Normal, Max, SnapL, SnapR }

#[derive(Clone, Copy)]
struct Win {
    used: bool,
    app: u8,
    internal: bool,
    r: Rect,
    restore: Rect,
    state: WState,
    minimized: bool,
    z: u32,
    pid: i64,
    slot: i8,
    title: [u8; 32],
    tlen: usize,
    hello: bool,
    committed: bool,
    commit_w: i32,
    commit_h: i32,
    want_close: bool,
    close_at: u64,
    keys: [u64; KQ],
    kh: usize,
    kn: usize,
    click: bool,
    rclick: bool,
    dbl: bool,
    wheel: i32,
    menu: u16,
    open_file: i32,
    /// ultimo estado entregue ao app (para saber se ha novidade): flags
    /// (foco/tema/tamanho/visivel) e posicao do mouse local
    rep_f: u64,
    rep_mx: i32,
    rep_my: i32,
}

impl Win {
    const fn empty() -> Win {
        Win {
            used: false, app: 0, internal: false, r: Rect::empty(), restore: Rect::empty(),
            state: WState::Normal, minimized: false, z: 0, pid: -1, slot: -1,
            title: [0; 32], tlen: 0, hello: false, committed: false, commit_w: 0, commit_h: 0,
            want_close: false, close_at: 0, keys: [0; KQ], kh: 0, kn: 0,
            click: false, rclick: false, dbl: false, wheel: 0, menu: 0, open_file: -1,
            rep_f: u64::MAX, rep_mx: i32::MIN, rep_my: i32::MIN,
        }
    }
    fn title(&self) -> &[u8] { &self.title[..self.tlen] }
    fn set_title(&mut self, t: &[u8]) {
        let n = t.len().min(32);
        self.title[..n].copy_from_slice(&t[..n]);
        self.tlen = n;
    }
    fn client(&self) -> Rect {
        if self.internal { self.r } else { Rect::new(self.r.x, self.r.y + TITLE_H, self.r.w, self.r.h - TITLE_H) }
    }
    /// Area que a janela pode afetar (inclui a sombra).
    fn outer(&self) -> Rect {
        Rect::new(self.r.x - 32, self.r.y - 24, self.r.w + 64, self.r.h + 64)
    }
    /// Retangulo garantidamente opaco (sem os cantos arredondados).
    fn opaque(&self) -> Rect {
        Rect::new(self.r.x, self.r.y + WIN_RAD, self.r.w, self.r.h - 2 * WIN_RAD)
    }
    fn lights(&self) -> Rect { Rect::new(self.r.x + 10, self.r.y + 8, 62, 22) }
    fn push_key(&mut self, k: u64) {
        if self.kn < KQ {
            self.keys[(self.kh + self.kn) % KQ] = k;
            self.kn += 1;
        }
    }
    fn pop_key(&mut self) -> u64 {
        if self.kn == 0 { return 0; }
        let k = self.keys[self.kh];
        self.kh = (self.kh + 1) % KQ;
        self.kn -= 1;
        k
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Drag {
    None,
    Move { win: usize, dx: i32, dy: i32, moved: bool },
    Resize { win: usize, edges: u8, start: Rect, mx: i32, my: i32 },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MenuKind { Bar(usize), Context }

#[derive(Clone, Copy)]
struct Menu {
    kind: MenuKind,
    rect: Rect,
    items: &'static [MI],
    target_win: Option<usize>,
    target_app: u8,
}

#[derive(Clone, Copy)]
struct Toast {
    active: bool,
    icon: Icon,
    title: [u8; 48],
    tl: usize,
    body: [u8; 72],
    bl: usize,
    until: u64,
}

impl Toast {
    const fn empty() -> Toast {
        Toast { active: false, icon: Icon::Pulsar, title: [0; 48], tl: 0, body: [0; 72], bl: 0, until: 0 }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Dialog { None, Poweroff }

/// Resultado da busca (Spotlight).
#[derive(Clone, Copy)]
enum Hit { App(u8), File(u16), Cmd(Action, &'static str, Icon), Calc(i64) }

const SEARCH_CMDS: [(&str, Action, Icon); 6] = [
    ("Desligar", Action::Poweroff, Icon::Pulsar),
    ("Alternar modo escuro", Action::ToggleDark, Icon::Settings),
    ("Papel de parede", Action::Settings(1), Icon::Settings),
    ("Atalhos do teclado", Action::Help, Icon::Help),
    ("Mostrar área de trabalho", Action::ShowDesktop, Icon::Files),
    ("Novo documento de texto", Action::NewTextDoc, Icon::Document),
];

struct Wm {
    scr: Fb,
    wall: Fb,
    blur: Fb,
    w: i32,
    h: i32,
    cfg: Config,
    cfg_raw: [u8; 2048],
    cfg_len: usize,
    th: Theme,
    wins: [Win; MAXWIN],
    zc: u32,
    // mouse
    mx: i32,
    my: i32,
    down: bool,
    capture: Option<usize>,
    last_click: u64,
    last_cx: i32,
    last_cy: i32,
    wheel: i32,
    kb: Keyboard,
    meta_tap: bool,
    drag: Drag,
    snap: Option<(Rect, WState)>,
    cursor: cursor::Shape,
    cursor_img: [u32; 64 * 64],
    // overlays
    menu: Option<Menu>,
    menu_hits: Hits,
    cc_open: bool,
    cc_hits: Hits,
    dialog: Dialog,
    dialog_hits: Hits,
    spot: bool,
    query: Buf<64>,
    spot_sel: usize,
    spot_hits: [Hit; 8],
    spot_n: usize,
    switcher: bool,
    sw_list: [usize; MAXWIN],
    sw_n: usize,
    sw_sel: usize,
    toasts: [Toast; 3],
    // estado das janelas internas
    panels: Panels,
    phits: [Hits; 12],
    hover_hit: Rect,
    hover_light: Option<usize>,
    // dock
    dock_hover: i32,
    dock_rects: [(Rect, u8); 16],
    dock_n: usize,
    dock_area: Rect,
    bounce: [u64; 12],
    // dados do sistema
    files: [FileInfo; MAX_FILES],
    nfiles: usize,
    sys: SysInfo,
    loads: [u32; 8],
    clock: Buf<40>,
    net_up: bool,
    /// retangulos danificados neste quadro (fundidos quando se sobrepoem)
    dmg: [Rect; 8],
    ndmg: usize,
    /// COMMITs respondidos so depois da composicao (o app nao pode
    /// redesenhar a superficie enquanto o compositor a copia)
    pending: [(usize, usize); MAXWIN],
    npending: usize,
    /// POLLs estacionados ("long poll"): respondidos quando o app tiver
    /// novidade ou apos POLL_MAX_MS, para apps ociosos nao girarem em falso
    parked: [(usize, usize, u64); MAXWIN],
    nparked: usize,
    caret_on: bool,
}

static mut WM: Wm = Wm::new();

/// Miniaturas dos papeis de parede (na .bss, fora do estado do WM).
static mut THUMBS: [u32; 6 * (THUMB_W * THUMB_H) as usize] = [0; 6 * (THUMB_W * THUMB_H) as usize];

fn thumbs() -> &'static mut [u32] {
    unsafe { &mut *(&raw mut THUMBS) }
}

const fn hits_array() -> [Hits; 12] {
    [Hits::new(), Hits::new(), Hits::new(), Hits::new(), Hits::new(), Hits::new(),
     Hits::new(), Hits::new(), Hits::new(), Hits::new(), Hits::new(), Hits::new()]
}

impl Wm {
    const fn new() -> Wm {
        let nullfb = Fb { base: core::ptr::null_mut(), w: 0, h: 0, stride: 0, clip: Rect::empty() };
        Wm {
            scr: nullfb, wall: nullfb, blur: nullfb, w: 0, h: 0,
            cfg: Config::default(), cfg_raw: [0; 2048], cfg_len: 0,
            th: Theme {
                dark: false, accent: 0, win_bg: 0, side_bg: 0, title_top: 0, title_bot: 0, text: 0, text2: 0,
                text3: 0, sep: 0, ctrl_bg: 0, ctrl_border: 0, field_bg: 0, menu_bg: 0, menu_alpha: 0,
                glass: 0, glass_alpha: 0, hover: 0, border: 0,
            },
            wins: [Win::empty(); MAXWIN], zc: 1,
            mx: 640, my: 400, down: false, capture: None, last_click: 0, last_cx: 0, last_cy: 0, wheel: 0,
            kb: Keyboard::new(), meta_tap: false,
            drag: Drag::None, snap: None, cursor: cursor::Shape::Hand, cursor_img: [0; 64 * 64],
            menu: None, menu_hits: Hits::new(), cc_open: false, cc_hits: Hits::new(),
            dialog: Dialog::None, dialog_hits: Hits::new(),
            spot: false, query: Buf::new(), spot_sel: 0, spot_hits: [Hit::App(0); 8], spot_n: 0,
            switcher: false, sw_list: [0; MAXWIN], sw_n: 0, sw_sel: 0,
            toasts: [Toast::empty(); 3],
            panels: Panels::new(), phits: hits_array(), hover_hit: Rect::empty(), hover_light: None,
            dock_hover: -1, dock_rects: [(Rect::empty(), 0); 16], dock_n: 0, dock_area: Rect::empty(),
            bounce: [0; 12],
            files: [FileInfo::zero(); MAX_FILES], nfiles: 0,
            sys: SysInfo { arch_id: 0, ncpus: 0, mem_total_kb: 0, mem_free_kb: 0, fb_w: 0, fb_h: 0, uptime_ms: 0, arch_name: [0; 16] },
            loads: [0; 8], clock: Buf::new(), net_up: false,
            dmg: [Rect::empty(); 8], ndmg: 0, pending: [(0, 0); MAXWIN], npending: 0, parked: [(0, 0, 0); MAXWIN], nparked: 0, caret_on: true,
        }
    }

    // ================================================================ boot

    fn run(&mut self) -> ! {
        write("[wm] Pulsar Desktop iniciando\n");
        self.scr = screen_open();
        self.w = self.scr.w;
        self.h = self.scr.h;
        self.wall = Fb::new(WALL_ADDR as *mut u32, self.w, self.h, self.w);
        self.blur = Fb::new(WALLBLUR_ADDR as *mut u32, self.w, self.h, self.w);
        register_service(SVC_WM);
        set_focus(getpid());
        wait_service(SVC_FS);

        let (cfg, n) = config::load(&mut self.cfg_raw);
        self.cfg = cfg;
        self.cfg_len = n;
        if n == 0 { self.save_config(); }
        self.apply_config(true);
        for k in 0..6 {
            let t = &mut thumbs()[k * (THUMB_W * THUMB_H) as usize..(k + 1) * (THUMB_W * THUMB_H) as usize];
            wallpaper::render_thumb(k as u8, t, THUMB_W, THUMB_H, self.w, self.h);
        }
        self.refresh_files();
        self.sys = sysinfo();
        self.update_clock();
        self.net_up = net_status() != 0;
        self.mx = self.w / 2;
        self.my = self.h / 2;
        self.set_cursor(cursor::Shape::Arrow);
        self.toast(Icon::Pulsar, "Bem-vindo ao Pulsar OS",
            "Ctrl+Espaço busca apps e arquivos. F1 mostra os atalhos.");
        let mut b = Buf::<64>::new();
        b.s("[wm] pronto: ").u(self.sys.ncpus as u64).s(" CPU(s), ").bytes(self.sys.arch()).s("\n");
        write_bytes(b.get());
        self.damage_all();

        let mut last_slow = 0u64;
        loop {
            let t0 = uptime_ms();
            self.serve_ipc();
            self.handle_input();
            if t0 >= last_slow + 500 {
                last_slow = t0;
                self.slow_tick(t0);
            }
            self.fast_tick(t0);
            self.release_parked();
            self.render();
            // espera o proximo quadro OU uma mensagem: um COMMIT de app e
            // composto logo (apps animados nao perdem quadros por esperar
            // o "tique" do compositor)
            let deadline = t0 + FRAME_MS;
            let mut m = Msg::new(0);
            loop {
                let now = uptime_ms();
                if now >= deadline { break; }
                let Some(from) = recv_timeout(&mut m, deadline - now) else { break };
                let commit = m.tag == WM_OP_COMMIT;
                self.handle_msg(from, &m);
                if commit {
                    // no maximo ~125 quadros/s
                    let since = uptime_ms().saturating_sub(t0);
                    if since < 8 { sleep_ms(8 - since); }
                    break;
                }
            }
        }
    }

    // ============================================================ config

    fn apply_config(&mut self, wallpaper_changed: bool) {
        self.th = Theme::new(self.cfg.dark, self.cfg.accent_rgb());
        self.kb.layout = self.cfg.layout;
        if wallpaper_changed {
            let t0 = uptime_ms();
            wallpaper::render(self.cfg.wallpaper, &self.wall);
            wallpaper::make_blur(&self.wall, &self.blur);
            let mut b = Buf::<64>::new();
            b.s("[wm] papel de parede '").s(config::WALLPAPERS[self.cfg.wallpaper as usize % 6])
                .s("' gerado em ").u(uptime_ms() - t0).s(" ms\n");
            write_bytes(b.get());
            if PERF_LOG { self.bench(); }
        }
        self.damage_all();
    }

    /// Microbenchmark das primitivas (so com PERF_LOG): ms por 20 execucoes.
    fn bench(&mut self) {
        let scr = self.scr;
        let r = Rect::new(200, 150, 640, 420);
        let th = self.th;
        let mut b = Buf::<512>::new();
        b.s("[bench] 640x420 x20 (ms):");
        macro_rules! t { ($name:expr, $e:expr) => {{
            let t0 = uptime_ms();
            for _ in 0..20 { $e; }
            b.s(" ").s($name).s("=").u(uptime_ms() - t0);
        }}}
        t!("copy", scr.copy_from(&self.wall, r));
        t!("fill", scr.fill(r, 0x123456));
        t!("filla", scr.fill_a(r, 0x123456, 128));
        t!("rrect", scr.round_rect(r, WIN_RAD, th.win_bg));
        t!("grad", scr.round_rect_grad(Rect::new(r.x, r.y, r.w, 52), WIN_RAD, th.title_top, th.title_bot, 255));
        t!("shadow", scr.shadow(r, WIN_RAD, 30, 12, 95));
        t!("blitr", scr.blit_rounded(&self.wall, r.x, r.y, r.w, r.h, r.x, r.y, WIN_RAD, false, true));
        t!("border", scr.round_border(r, WIN_RAD, 0, 40));
        t!("text", scr.text(&FONT_UI, r.x, r.y, b"The quick brown fox jumps over the lazy dog 0123456789", 0));
        t!("lights", for k in 0..3 { scr.disc(r.x + 20 + k * 20, r.y + 19, 6, 0xFF5F57, 255); scr.ring(r.x + 20 + k * 20, r.y + 19, 6, 1, 0, 30); });
        t!("glass", ui::glass(&scr, &self.blur, Rect::new(300, 700, 600, 80), 22, &th, true));
        t!("icon", icons::draw(&scr, catalog::app(0).icon, 400, 300, 56));
        self.layout_dock();
        t!("dock", self.draw_dock(&scr));
        let bg = Rect::new(330, 714, 620, 76);
        t!("dshadow", scr.shadow(bg, 22, 20, 6, 60));
        t!("dglass", ui::glass(&scr, &self.blur, bg, 22, &th, true));
        t!("ishadow", for k in 0..9 { let r = Rect::new(340 + k * 64, 724, 56, 56); scr.shadow(r, 12, 7, 2, 60); });
        t!("icons", for k in 0..9 { icons::draw(&scr, catalog::app(k as u8).icon, 340 + k * 64, 724, 56); });
        t!("sprites", for k in 0..9 { sprites::dock_icon(&scr, catalog::app(k as u8).icon, Rect::new(340 + k * 64, 724, 56, 56)); });
        t!("icons96", for k in 0..9 { icons::draw(&scr, catalog::app(k as u8).icon, 340 + k * 64, 624, 96); });
        b.s("\n");
        write_bytes(b.get());
    }

    fn save_config(&mut self) {
        let mut out = Buf::<1024>::new();
        config::save(&self.cfg, &mut out);
        let n = out.n.min(2048);
        self.cfg_raw[..n].copy_from_slice(&out.b[..n]);
        self.cfg_len = n;
    }

    fn set_cfg(&mut self, f: impl FnOnce(&mut Config)) {
        let old = self.cfg;
        f(&mut self.cfg);
        if self.cfg != old {
            self.save_config();
            self.apply_config(old.wallpaper != self.cfg.wallpaper);
        }
    }

    /// Relê pulsar.cfg (editado a mao no Editor, por exemplo).
    fn watch_config(&mut self) {
        let mut raw = [0u8; 2048];
        let (cfg, n) = config::load(&mut raw);
        if n == 0 || (n == self.cfg_len && raw[..n] == self.cfg_raw[..n]) { return; }
        self.cfg_raw = raw;
        self.cfg_len = n;
        if cfg != self.cfg {
            let wall = cfg.wallpaper != self.cfg.wallpaper;
            self.cfg = cfg;
            self.apply_config(wall);
            self.toast(Icon::Config, "Configuração recarregada", "pulsar.cfg foi alterado e aplicado.");
        }
    }

    // ============================================================ helpers

    fn damage(&mut self, r: Rect) {
        let mut r = r.intersect(&Rect::new(0, 0, self.w, self.h));
        if r.is_empty() { return; }
        // funde com tudo que sobrepoe (retangulos so encostados ficam
        // separados: assim a area opaca de um app nao se mistura com as
        // bordas arredondadas e a oclusao continua valendo)
        let mut k = 0;
        while k < self.ndmg {
            if self.dmg[k].overlaps(&r) || self.dmg[k].contains_rect(&r) {
                r = r.union(&self.dmg[k]);
                self.ndmg -= 1;
                self.dmg[k] = self.dmg[self.ndmg];
                k = 0;
                continue;
            }
            k += 1;
        }
        if self.ndmg == self.dmg.len() {
            self.dmg[0] = self.dmg[0].union(&r);
        } else {
            self.dmg[self.ndmg] = r;
            self.ndmg += 1;
        }
    }

    fn damage_all(&mut self) {
        self.dmg[0] = Rect::new(0, 0, self.w, self.h);
        self.ndmg = 1;
    }

    /// Dano de um commit de app: parte opaca + faixa dos cantos inferiores.
    fn damage_client(&mut self, i: usize) {
        let c = self.wins[i].client();
        // totalmente coberta por uma janela acima? nada visivel mudou
        let z = self.wins[i].z;
        if self.wins.iter().any(|w| w.used && !w.minimized && w.z > z && w.opaque().contains_rect(&c)) {
            return;
        }
        if c.h > WIN_RAD * 2 {
            self.damage(Rect::new(c.x, c.y, c.w, c.h - WIN_RAD));
            self.damage(Rect::new(c.x, c.bottom() - WIN_RAD, c.w, WIN_RAD));
        } else {
            self.damage(c);
        }
    }

    /// Como `damage_client`, mas so o retangulo `d` (coordenadas da
    /// superficie) que o app informou no COMMIT.
    fn damage_client_rect(&mut self, i: usize, d: Rect) {
        let c = self.wins[i].client();
        let r = d.offset(c.x, c.y).intersect(&c);
        if r.is_empty() { return; }
        let z = self.wins[i].z;
        if self.wins.iter().any(|w| w.used && !w.minimized && w.z > z && w.opaque().contains_rect(&r)) {
            return;
        }
        // a faixa dos cantos inferiores fica num retangulo proprio (oclusao)
        let cut = c.bottom() - WIN_RAD;
        if r.y < cut && r.bottom() > cut {
            self.damage(Rect::new(r.x, r.y, r.w, cut - r.y));
            self.damage(Rect::new(r.x, cut, r.w, r.bottom() - cut));
        } else {
            self.damage(r);
        }
    }

    fn set_cursor(&mut self, s: cursor::Shape) {
        if s == self.cursor { return; }
        self.cursor = s;
        let (hx, hy) = cursor::make(s, &mut self.cursor_img);
        cursor_set(&self.cursor_img, hx, hy, self.mx, self.my);
    }

    fn work_area(&self) -> Rect {
        let s = self.cfg.icon_size();
        let reserve = s + s * 36 / 100 + 12;
        if self.cfg.dock_left {
            Rect::new(reserve, MENU_H, self.w - reserve, self.h - MENU_H)
        } else {
            Rect::new(0, MENU_H, self.w, self.h - MENU_H - reserve)
        }
    }

    /// Janela com foco: a mais alta visivel.
    fn focused(&self) -> Option<usize> {
        let mut best: Option<usize> = None;
        for i in 0..MAXWIN {
            let w = &self.wins[i];
            if w.used && !w.minimized && best.map(|b| w.z > self.wins[b].z).unwrap_or(true) {
                best = Some(i);
            }
        }
        best
    }

    fn z_order(&self, out: &mut [usize; MAXWIN]) -> usize {
        let mut n = 0;
        for i in 0..MAXWIN {
            if self.wins[i].used && !self.wins[i].minimized { out[n] = i; n += 1; }
        }
        // insercao por z crescente
        for a in 1..n {
            let mut b = a;
            while b > 0 && self.wins[out[b - 1]].z > self.wins[out[b]].z {
                out.swap(b - 1, b);
                b -= 1;
            }
        }
        n
    }

    /// Janela visivel sob o ponto (de cima para baixo).
    fn win_at(&self, x: i32, y: i32) -> Option<usize> {
        let mut order = [0usize; MAXWIN];
        let n = self.z_order(&mut order);
        for k in (0..n).rev() {
            let w = &self.wins[order[k]];
            if w.r.inset(-RESIZE_M).contains(x, y) { return Some(order[k]); }
        }
        None
    }

    fn raise(&mut self, i: usize) {
        // ja e a janela do topo (e visivel)? nada a fazer
        if self.wins[i].z == self.zc && !self.wins[i].minimized { return; }
        let prev = self.focused();
        self.zc += 1;
        self.wins[i].z = self.zc;
        self.wins[i].minimized = false;
        self.damage(self.wins[i].outer());
        if let Some(p) = prev { if p != i { self.damage(self.wins[p].outer()); } }
        self.damage(Rect::new(0, 0, self.w, MENU_H));
        self.damage(self.dock_area);
    }

    fn find_app_win(&self, app: u8) -> Option<usize> {
        (0..MAXWIN).find(|&i| self.wins[i].used && self.wins[i].app == app)
    }

    fn toast(&mut self, icon: Icon, title: &str, body: &str) {
        self.toast_b(icon, title.as_bytes(), body.as_bytes());
    }

    fn toast_b(&mut self, icon: Icon, title: &[u8], body: &[u8]) {
        let k = self.toasts.iter().position(|t| !t.active).unwrap_or_else(|| {
            self.toasts.rotate_left(1);
            2
        });
        let t = &mut self.toasts[k];
        t.active = true;
        t.icon = icon;
        t.tl = title.len().min(48);
        t.title[..t.tl].copy_from_slice(&title[..t.tl]);
        t.bl = body.len().min(72);
        t.body[..t.bl].copy_from_slice(&body[..t.bl]);
        t.until = uptime_ms() + 6000;
        self.damage(self.toast_area());
    }

    fn toast_area(&self) -> Rect {
        Rect::new(self.w - 360, MENU_H, 360, 3 * 78 + 30)
    }

    fn refresh_files(&mut self) {
        let n = fs_count().min(MAX_FILES);
        for i in 0..n {
            if let Some(f) = file_info(i) { self.files[i] = f; }
        }
        self.nfiles = n;
    }

    fn update_clock(&mut self) -> bool {
        let (_, mo, d, hh, mm, ss, wd) = civil_time(rtc_seconds());
        let mut b = Buf::<40>::new();
        b.s(WEEKDAYS[wd as usize % 7]).s(" ").u(d as u64).s(" ").s(MONTHS[(mo as usize + 11) % 12]).s("  ");
        let (h12, suf) = if self.cfg.clock24 { (hh, "") } else {
            (if hh % 12 == 0 { 12 } else { hh % 12 }, if hh < 12 { " AM" } else { " PM" })
        };
        b.u0(h12 as u64, if self.cfg.clock24 { 2 } else { 1 }).s(":").u0(mm as u64, 2);
        if self.cfg.seconds { b.s(":").u0(ss as u64, 2); }
        b.s(suf);
        let changed = b.get() != self.clock.get();
        self.clock = b;
        changed
    }

    // ============================================================== IPC

    fn serve_ipc(&mut self) {
        let mut m = Msg::new(0);
        for _ in 0..64 {
            let Some(from) = try_recv(&mut m) else { break };
            self.handle_msg(from, &m);
        }
    }

    /// Atende uma mensagem de app. COMMIT so e respondido apos a composicao.
    fn handle_msg(&mut self, from: usize, m: &Msg) {
        {
            let mut r = Msg::new(m.tag);
            let wi = (0..MAXWIN).find(|&i| self.wins[i].used && self.wins[i].pid == from as i64);
            match m.tag {
                WM_OP_HELLO => {
                    let wi = match wi {
                        Some(i) => Some(i),
                        // processo lancado fora do WM (ex.: pelo Terminal)
                        None => {
                            let mut t = [0u8; 24];
                            let n = unpack_title(&m.data[3..6], &mut t);
                            let kind = m.data[0] as usize;
                            let app = APPS.iter().find(|a| a.kind == Kind::Process && a.name.as_bytes() == &t[..n])
                                .map(|a| a.id)
                                .unwrap_or(if kind < APPS.len() && APPS[kind].kind == Kind::Process && kind != APP_GENERIC as usize
                                    { kind as u8 } else { APP_GENERIC });
                            self.new_window(app, from as i64, -1)
                        }
                    };
                    if let Some(i) = wi {
                        let want_w = (m.data[1] as i32).clamp(200, SURF_W);
                        let want_h = (m.data[2] as i32).clamp(120, SURF_H);
                        let mut t = [0u8; 24];
                        let n = unpack_title(&m.data[3..6], &mut t);
                        let w = &mut self.wins[i];
                        w.hello = true;
                        if n > 0 { w.set_title(&t[..n]); }
                        if !w.committed && w.state == WState::Normal {
                            let old = w.outer();
                            w.r.w = want_w;
                            w.r.h = want_h + TITLE_H;
                            let wa = self.work_area();
                            let w = &mut self.wins[i];
                            w.r.x = w.r.x.min(wa.right() - w.r.w).max(wa.x);
                            w.r.y = w.r.y.min(wa.bottom() - w.r.h).max(wa.y);
                            w.restore = w.r;
                            self.damage(old);
                        }
                        let w = &self.wins[i];
                        let c = w.client();
                        r.data[0] = w.slot as u64;
                        r.data[1] = c.w.min(SURF_W) as u64;
                        r.data[2] = c.h.min(SURF_H) as u64;
                        r.data[3] = if w.open_file >= 0 { w.open_file as u64 + 1 } else { 0 };
                        self.damage(self.wins[i].outer());
                    }
                }
                WM_OP_COMMIT => {
                    if let Some(i) = wi {
                        let w = &mut self.wins[i];
                        let first = !w.committed;
                        w.committed = true;
                        let c = w.client();
                        let (cw, ch) = ((m.data[1] >> 16) as i32 & 0xFFFF, m.data[1] as i32 & 0xFFFF);
                        // (c so serve de padrao quando o app nao informa o tamanho)
                        let (ow, oh) = (w.commit_w, w.commit_h);
                        w.commit_w = if cw > 0 { cw } else { c.w };
                        w.commit_h = if ch > 0 { ch } else { c.h };
                        let same = !first && ow == w.commit_w && oh == w.commit_h;
                        if !w.minimized {
                            // data[2] (opcional): so este retangulo mudou
                            match commit_damage(m.data[2]) {
                                Some(d) if same => self.damage_client_rect(i, d),
                                _ => self.damage_client(i),
                            }
                            if self.npending < MAXWIN {
                                self.pending[self.npending] = (from, i);
                                self.npending += 1;
                                return; // resposta depois de compor
                            }
                        }
                        self.fill_poll(i, &mut r);
                    }
                }
                WM_OP_POLL => {
                    if let Some(i) = wi {
                        // data[0] bit 0 = "peek": responde ja, sem estacionar
                        // (apps com trabalho de fundo, como o Vela carregando)
                        if m.data[0] & 1 == 0 && !self.has_news(i) && self.nparked < MAXWIN {
                            self.parked[self.nparked] = (from, i, uptime_ms());
                            self.nparked += 1;
                            return; // resposta quando houver novidade
                        }
                        self.fill_poll(i, &mut r);
                    }
                }
                WM_OP_TITLE => {
                    if let Some(i) = wi {
                        let mut t = [0u8; 24];
                        let n = unpack_title(&m.data[0..3], &mut t);
                        self.wins[i].set_title(&t[..n]);
                        let tb = Rect::new(self.wins[i].r.x, self.wins[i].r.y, self.wins[i].r.w, TITLE_H);
                        self.damage(tb);
                        if self.focused() == Some(i) { self.damage(Rect::new(0, 0, self.w, MENU_H)); }
                    }
                }
                WM_OP_BYE => {
                    // o app fecha a janela de proposito (sem aviso de encerramento)
                    if let Some(i) = wi {
                        let w = self.wins[i];
                        self.wins[i].used = false;
                        if self.capture == Some(i) { self.capture = None; }
                        if let Drag::Move { win, .. } | Drag::Resize { win, .. } = self.drag { if win == i { self.drag = Drag::None; } }
                        self.damage(w.outer());
                        self.damage(Rect::new(0, 0, self.w, MENU_H));
                        self.damage(self.dock_area);
                    }
                }
                _ => {}
            }
            reply(from, &r);
        }
    }

    /// Flags de estado do app `i` (sem os eventos de mouse/tecla).
    fn state_flags(&self, i: usize) -> u64 {
        let w = &self.wins[i];
        let c = w.client();
        let mut f = (c.w.min(SURF_W) as u64) << 16 | (c.h.min(SURF_H) as u64) << 32;
        if w.want_close { f |= WM_FLAG_CLOSE; }
        if self.focused() == Some(i) { f |= WM_FLAG_FOCUS; }
        if self.th.dark { f |= WM_FLAG_DARK; }
        if !w.minimized { f |= WM_FLAG_VISIBLE; }
        // (bits so de comparacao: botao preso e cor de destaque)
        if self.down && self.capture == Some(i) { f |= 1 << 62; }
        f | ((self.th.accent as u64 & 0x3FFF) << 48)
    }

    /// O app `i` tem algo novo para receber?
    fn has_news(&self, i: usize) -> bool {
        let w = &self.wins[i];
        if w.kn > 0 || w.click || w.rclick || w.dbl || w.wheel != 0 || w.menu != 0 || w.want_close {
            return true;
        }
        if self.state_flags(i) != w.rep_f { return true; }
        // mouse andou sobre a janela (ou acabou de sair dela): realces
        let c = w.client();
        let (lx, ly) = (self.mx - c.x, self.my - c.y);
        let inside = c.contains(self.mx, self.my) && self.win_at(self.mx, self.my) == Some(i);
        let was_inside = w.rep_mx >= 0 && w.rep_my >= 0 && w.rep_mx < c.w && w.rep_my < c.h;
        let captured = self.capture == Some(i);
        (inside || was_inside || captured) && (lx != w.rep_mx || ly != w.rep_my)
    }

    /// Responde os POLLs estacionados que ja tem novidade ou expiraram.
    fn release_parked(&mut self) {
        let now = uptime_ms();
        let mut k = 0;
        while k < self.nparked {
            let (pid, i, since) = self.parked[k];
            let alive = self.wins[i].used && self.wins[i].pid == pid as i64;
            if !alive || self.has_news(i) || now - since >= POLL_MAX_MS {
                let mut r = Msg::new(WM_OP_POLL);
                if alive { self.fill_poll(i, &mut r); }
                reply(pid, &r);
                self.nparked -= 1;
                self.parked[k] = self.parked[self.nparked];
                continue;
            }
            k += 1;
        }
    }

    fn fill_poll(&mut self, i: usize, r: &mut Msg) {
        let state = self.state_flags(i);
        let focused = self.focused() == Some(i);
        let (mx, my, down, cap) = (self.mx, self.my, self.down, self.capture);
        let dark = self.th.dark;
        let accent = self.th.accent;
        let w = &mut self.wins[i];
        let c = w.client();
        let key = w.pop_key();
        let mut f = (c.w.min(SURF_W) as u64) << 16 | (c.h.min(SURF_H) as u64) << 32;
        if w.want_close { f |= WM_FLAG_CLOSE; }
        if focused { f |= WM_FLAG_FOCUS; }
        if dark { f |= WM_FLAG_DARK; }
        if w.kn > 0 { f |= WM_FLAG_MORE; }
        if !w.minimized { f |= WM_FLAG_VISIBLE; }
        r.data[0] = f;
        r.data[1] = (mx - c.x) as i64 as u64;
        r.data[2] = (my - c.y) as i64 as u64;
        let mut b = 0u64;
        if down && cap == Some(i) { b |= WM_MOUSE_DOWN; }
        if w.click { b |= WM_MOUSE_CLICK; }
        if w.rclick { b |= WM_MOUSE_RCLICK; }
        if w.dbl { b |= WM_MOUSE_DOUBLE; }
        b |= ((w.wheel.clamp(-127, 127) as i8 as u8) as u64) << 8;
        r.data[3] = b;
        r.data[4] = key;
        r.data[5] = w.menu as u64 | ((accent as u64) << 32);
        w.rep_f = state;
        w.rep_mx = mx - c.x;
        w.rep_my = my - c.y;
        w.click = false;
        w.rclick = false;
        w.dbl = false;
        w.wheel = 0;
        w.menu = 0;
    }

    // ============================================================ janelas

    fn free_slot(&self) -> Option<i8> {
        (0..SURF_SLOTS as i8).find(|&s| !self.wins.iter().any(|w| w.used && w.slot == s))
    }

    fn new_window(&mut self, app: u8, pid: i64, open_file: i32) -> Option<usize> {
        let idx = (0..MAXWIN).find(|&i| !self.wins[i].used)?;
        let def = catalog::app(app);
        let internal = def.kind == Kind::Internal;
        let slot = if internal { -1 } else { self.free_slot()? };
        let count = self.wins.iter().filter(|w| w.used).count() as i32;
        let wa = self.work_area();
        let (ww, wh) = if internal { (def.w, def.h) } else { (def.w, def.h + TITLE_H) };
        let x = (wa.x + (wa.w - ww) / 2 + (count % 6) * 26 - 60).clamp(wa.x, (wa.right() - ww).max(wa.x));
        let y = (wa.y + (wa.h - wh) / 2 + (count % 6) * 24 - 50).clamp(wa.y, (wa.bottom() - wh).max(wa.y));
        let mut w = Win::empty();
        w.used = true;
        w.app = app;
        w.internal = internal;
        w.r = Rect::new(x, y, ww.min(wa.w), wh.min(wa.h));
        w.restore = w.r;
        w.pid = pid;
        w.slot = slot;
        w.open_file = open_file;
        w.set_title(def.name.as_bytes());
        self.wins[idx] = w;
        self.raise(idx);
        self.damage(self.wins[idx].outer());
        Some(idx)
    }

    /// Abre (ou traz para frente) um aplicativo do catalogo.
    fn launch(&mut self, app: u8, open_file: i32) {
        let def = catalog::app(app);
        if !def.multi || open_file < 0 {
            if let Some(i) = self.find_app_win(app) {
                self.raise(i);
                return;
            }
        }
        if def.kind == Kind::Internal {
            self.new_window(app, -1, -1);
        } else {
            if self.free_slot().is_none() || !self.wins.iter().any(|w| !w.used) {
                self.toast(def.icon, "Janelas demais", "Feche uma janela para abrir outra.");
                return;
            }
            let pid = spawn(def.bin);
            if pid < 0 {
                let mut b = Buf::<72>::new();
                b.s(def.bin).s(" não foi encontrado no disco.");
                self.toast_b(def.icon, "Não foi possível abrir".as_bytes(), b.get());
                return;
            }
            self.new_window(app, pid, open_file);
        }
        self.bounce[app as usize] = uptime_ms() + 700;
    }

    fn close_window(&mut self, i: usize) {
        let w = &mut self.wins[i];
        if !w.used { return; }
        if w.internal {
            let o = w.outer();
            w.used = false;
            self.damage(o);
            self.damage(Rect::new(0, 0, self.w, MENU_H));
            self.damage(self.dock_area);
        } else if !w.want_close {
            w.want_close = true;
            w.close_at = uptime_ms() + 3000;
        }
    }

    fn force_quit(&mut self, i: usize) {
        let w = self.wins[i];
        if w.internal { return self.close_window(i); }
        if w.pid >= 0 { kill(w.pid as usize); }
    }

    fn set_state(&mut self, i: usize, st: WState) {
        let wa = self.work_area();
        let old = self.wins[i].outer();
        let w = &mut self.wins[i];
        if w.state == WState::Normal && st != WState::Normal { w.restore = w.r; }
        w.state = st;
        w.r = match st {
            WState::Normal => w.restore,
            WState::Max => wa,
            WState::SnapL => Rect::new(wa.x, wa.y, wa.w / 2, wa.h),
            WState::SnapR => Rect::new(wa.x + wa.w / 2, wa.y, wa.w - wa.w / 2, wa.h),
        };
        let new = self.wins[i].outer();
        self.damage(old);
        self.damage(new);
    }

    fn toggle_zoom(&mut self, i: usize) {
        let st = if self.wins[i].state == WState::Normal { WState::Max } else { WState::Normal };
        self.set_state(i, st);
    }

    fn minimize(&mut self, i: usize) {
        self.wins[i].minimized = true;
        self.damage(self.wins[i].outer());
        self.damage(Rect::new(0, 0, self.w, MENU_H));
        self.damage(self.dock_area);
    }

    fn tile_all(&mut self) {
        let wa = self.work_area();
        let ids: [usize; MAXWIN] = core::array::from_fn(|i| i);
        let vis: usize = ids.iter().filter(|&&i| self.wins[i].used && !self.wins[i].minimized).count();
        if vis == 0 { return; }
        let cols = if vis <= 2 { vis } else if vis <= 4 { 2 } else { 3 } as i32;
        let rows = ((vis as i32) + cols - 1) / cols;
        let mut k = 0;
        for i in 0..MAXWIN {
            if !self.wins[i].used || self.wins[i].minimized { continue; }
            let (c, r) = (k % cols, k / cols);
            let cell = Rect::new(wa.x + c * wa.w / cols, wa.y + r * wa.h / rows, wa.w / cols, wa.h / rows);
            let w = &mut self.wins[i];
            w.state = WState::Normal;
            w.r = cell.inset(4);
            w.restore = w.r;
            k += 1;
        }
        self.damage_all();
    }

    fn cascade(&mut self) {
        let wa = self.work_area();
        let mut order = [0usize; MAXWIN];
        let n = self.z_order(&mut order);
        for k in 0..n {
            let w = &mut self.wins[order[k]];
            w.state = WState::Normal;
            w.r = Rect::new(wa.x + 40 + k as i32 * 30, wa.y + 20 + k as i32 * 28, w.restore.w.min(wa.w - 80), w.restore.h.min(wa.h - 40));
            w.restore = w.r;
        }
        self.damage_all();
    }

    fn show_desktop(&mut self) {
        let any = self.wins.iter().any(|w| w.used && !w.minimized);
        for w in self.wins.iter_mut() {
            if w.used { w.minimized = any; }
        }
        self.damage_all();
    }

    // ============================================================ acoes

    fn target(&self) -> Option<usize> {
        self.menu.and_then(|m| m.target_win).or_else(|| self.focused())
    }

    fn do_action(&mut self, a: Action) {
        let tgt = self.target();
        match a {
            Action::None => {}
            Action::About => self.launch(APP_ABOUT, -1),
            Action::Help => self.launch(APP_HELP, -1),
            Action::Settings(p) => {
                self.panels.settings_page = p.min(4);
                self.launch(APP_SETTINGS, -1);
                if let Some(i) = self.find_app_win(APP_SETTINGS) { self.damage(self.wins[i].outer()); }
            }
            Action::SettingsPage(p) => {
                self.panels.settings_page = p;
                if let Some(i) = self.find_app_win(APP_SETTINGS) { self.damage(self.wins[i].outer()); }
            }
            Action::Launch(id) => self.launch(id, -1),
            Action::Poweroff => {
                self.dialog = Dialog::Poweroff;
                self.damage_all();
            }
            Action::DialogCancel => {
                self.dialog = Dialog::None;
                self.damage_all();
            }
            Action::DialogConfirm => {
                write("[wm] desligando...\n");
                self.scr.fill(Rect::new(0, 0, self.w, self.h), 0x000000);
                fb_present(Rect::new(0, 0, self.w, self.h));
                poweroff();
            }
            Action::NewTextDoc => self.new_text_doc(),
            Action::OpenFileIdx(i) => {
                let i = if i == u16::MAX { self.panels.files_sel } else { i as i32 };
                if i >= 0 { self.open_file(i as usize, false); }
            }
            Action::OpenWithEditor(i) => {
                let i = if i == u16::MAX { self.panels.files_sel } else { i as i32 };
                if i >= 0 { self.open_file(i as usize, true); }
            }
            Action::FileInfo(i) => {
                let i = if i == u16::MAX { self.panels.files_sel } else { i as i32 };
                if i >= 0 && (i as usize) < self.nfiles {
                    let f = self.files[i as usize];
                    let mut b = Buf::<72>::new();
                    b.s(panels::file_kind_label(&f)).s(" · ").size(f.size as u64);
                    self.toast_b(panels::file_icon(&f), f.name(), b.get());
                }
            }
            Action::FileSel(i) => {
                self.panels.files_sel = i as i32;
                if let Some(w) = self.find_app_win(APP_FILES) { self.damage(self.wins[w].r); }
            }
            Action::FilesView(v) => {
                self.panels.files_view = v;
                self.panels.files_scroll = 0;
                if let Some(w) = self.find_app_win(APP_FILES) { self.damage(self.wins[w].r); }
            }
            Action::FilesFilter(f) => {
                self.panels.files_filter = f;
                self.panels.files_scroll = 0;
                if let Some(w) = self.find_app_win(APP_FILES) { self.damage(self.wins[w].r); }
            }
            Action::AppMenu(code) => {
                if let Some(i) = tgt { self.wins[i].menu = code; }
            }
            Action::CloseWin | Action::QuitApp => { if let Some(i) = tgt { self.close_window(i); } }
            Action::ForceQuit => { if let Some(i) = tgt { self.force_quit(i); } }
            Action::MinimizeWin => { if let Some(i) = tgt { self.minimize(i); } }
            Action::ZoomWin => { if let Some(i) = tgt { self.toggle_zoom(i); } }
            Action::SnapLeft => { if let Some(i) = tgt { self.set_state(i, WState::SnapL); } }
            Action::SnapRight => { if let Some(i) = tgt { self.set_state(i, WState::SnapR); } }
            Action::TileAll => self.tile_all(),
            Action::Cascade => self.cascade(),
            Action::ShowDesktop => self.show_desktop(),
            Action::CloseAll => {
                for i in 0..MAXWIN { if self.wins[i].used { self.close_window(i); } }
            }
            Action::Spotlight => self.open_spotlight(),
            Action::ControlCenter => {
                self.cc_open = !self.cc_open;
                self.damage(self.cc_rect());
            }
            Action::ToggleDark => self.set_cfg(|c| c.dark = !c.dark),
            Action::ToggleMagnify => self.set_cfg(|c| c.dock_magnify = !c.dock_magnify),
            Action::SetAccent(k) => self.set_cfg(|c| c.accent = k),
            Action::SetWallpaper(k) => self.set_cfg(|c| c.wallpaper = k),
            Action::DockSize(k) => self.set_cfg(|c| c.dock_size = k),
            Action::DockLeft(v) => self.set_cfg(|c| c.dock_left = v),
            Action::SetClock24(v) => { self.set_cfg(|c| c.clock24 = v); self.update_clock(); }
            Action::SetSeconds(v) => { self.set_cfg(|c| c.seconds = v); self.update_clock(); }
            Action::SetCpuMeter(v) => self.set_cfg(|c| c.cpu_meter = v),
            Action::SetGlass(v) => self.set_cfg(|c| c.glass = v),
            Action::SetLayout(v) => self.set_cfg(|c| c.layout = v),
            Action::SetDockMinimize(v) => self.set_cfg(|c| c.dock_minimize = v),
            Action::EditConfig => {
                if let Some(i) = find_file(config::CFG_NAME) { self.launch(APP_EDITOR, i as i32); }
            }
            Action::DockOpen => {
                if let Some(m) = self.menu { self.launch(m.target_app, -1); }
            }
        }
    }

    fn new_text_doc(&mut self) {
        let mut name = Buf::<28>::new();
        for k in 1..100u64 {
            name.clear().s("Sem titulo");
            if k > 1 { name.s(" ").u(k); }
            name.s(".txt");
            if find_file(name.get()).is_none() { break; }
        }
        let idx = fs_create(name.get());
        if idx < 0 {
            self.toast(Icon::Document, "Não foi possível criar", "O disco está cheio?");
            return;
        }
        self.refresh_files();
        self.panels.files_sel = idx as i32;
        if let Some(w) = self.find_app_win(APP_FILES) { self.damage(self.wins[w].r); }
        self.launch(APP_EDITOR, idx as i32);
    }

    fn open_file(&mut self, idx: usize, force_editor: bool) {
        if idx >= self.nfiles { return; }
        let f = self.files[idx];
        if !force_editor && f.ext() == b"pulse" {
            if let Some(a) = app_by_bin(f.name()) {
                self.launch(a.id, -1);
            } else {
                let name = unsafe { core::str::from_utf8_unchecked(f.name()) };
                if spawn(name) < 0 {
                    self.toast(Icon::Program, "Programa não iniciou", "O executável é inválido para esta arquitetura.");
                }
            }
            return;
        }
        if f.ext() == b"pulse" {
            self.toast(Icon::Program, "Arquivo binário", "Executáveis .pulse não podem ser editados.");
            return;
        }
        self.launch(APP_EDITOR, idx as i32);
    }

    // ============================================================ input

    fn handle_input(&mut self) {
        let (ox, oy) = (self.mx, self.my);
        for _ in 0..256 {
            let Some(ev) = input_poll() else { break };
            match ev.ev_type {
                EV_ABS => {
                    let v = ev.value.min(32767) as i32;
                    if ev.code == ABS_X { self.mx = v * (self.w - 1) / 32767; }
                    if ev.code == ABS_Y { self.my = v * (self.h - 1) / 32767; }
                }
                EV_REL if ev.code == REL_WHEEL => self.wheel += ev.value as i32,
                EV_KEY => match ev.code {
                    BTN_LEFT => {
                        if self.mx != ox || self.my != oy { self.on_move(); }
                        if ev.value != 0 { self.on_press(); } else { self.on_release(); }
                    }
                    BTN_RIGHT => { if ev.value != 0 { self.on_rclick(); } }
                    BTN_MIDDLE => {}
                    code => {
                        if (code == key::LMETA || code == key::RMETA) && ev.value == 0 && self.meta_tap {
                            self.meta_tap = false;
                            self.kb.feed(code, 0);
                            self.toggle_spotlight();
                            continue;
                        }
                        if (code == key::LALT || code == key::RALT) && ev.value == 0 && self.switcher {
                            self.kb.feed(code, 0);
                            self.finish_switcher();
                            continue;
                        }
                        if let Some(k) = self.kb.feed(code, ev.value) {
                            self.meta_tap = (code == key::LMETA || code == key::RMETA) && k.mods == MOD_META;
                            if !(code == key::LMETA || code == key::RMETA) { self.on_key(k); }
                        }
                    }
                },
                _ => {}
            }
        }
        if self.mx != ox || self.my != oy { self.on_move(); }
        if self.wheel != 0 {
            let w = self.wheel;
            self.wheel = 0;
            self.on_wheel(w);
        }
    }

    fn on_move(&mut self) {
        cursor_move(self.mx, self.my);
        let (mx, my) = (self.mx, self.my);
        match self.drag {
            Drag::Move { win, dx, dy, .. } => {
                let old = self.wins[win].outer();
                let wa = self.work_area();
                let w = &mut self.wins[win];
                if w.state != WState::Normal {
                    // sair do maximizado/encaixado arrastando: restaura sob o mouse
                    let rw = w.restore.w;
                    w.state = WState::Normal;
                    w.r = Rect::new(mx - rw / 2, my - 16, rw, w.restore.h);
                    self.drag = Drag::Move { win, dx: rw / 2, dy: 16, moved: true };
                } else {
                    w.r.x = (mx - dx).clamp(-w.r.w + 80, self.w - 80);
                    w.r.y = (my - dy).clamp(MENU_H, self.h - 40);
                    self.drag = Drag::Move { win, dx, dy, moved: true };
                }
                let new = self.wins[win].outer();
                self.damage(old);
                self.damage(new);
                // pre-visualizacao do encaixe (Aero Snap)
                let snap = if my <= MENU_H + 2 { Some((wa, WState::Max)) }
                    else if mx <= wa.x + 2 { Some((Rect::new(wa.x, wa.y, wa.w / 2, wa.h), WState::SnapL)) }
                    else if mx >= self.w - 3 { Some((Rect::new(wa.x + wa.w / 2, wa.y, wa.w - wa.w / 2, wa.h), WState::SnapR)) }
                    else { None };
                if snap.map(|s| s.0) != self.snap.map(|s| s.0) {
                    if let Some(s) = self.snap { self.damage(s.0.inset(-4)); }
                    if let Some(s) = snap { self.damage(s.0.inset(-4)); }
                    self.snap = snap;
                }
                return;
            }
            Drag::Resize { win, edges, start, mx: sx, my: sy } => {
                let old = self.wins[win].outer();
                let w = &mut self.wins[win];
                let (dx, dy) = (mx - sx, my - sy);
                let minw = if w.internal { 420 } else { 220 };
                if edges & 1 != 0 { w.r.w = (start.w + dx).clamp(minw, SURF_W); }
                if edges & 2 != 0 { w.r.h = (start.h + dy).clamp(160, SURF_H + TITLE_H); }
                if edges & 4 != 0 {
                    let nw = (start.w - dx).clamp(minw, SURF_W);
                    w.r.x = start.x + start.w - nw;
                    w.r.w = nw;
                }
                w.restore = w.r;
                let new = self.wins[win].outer();
                self.damage(old);
                self.damage(new);
                return;
            }
            Drag::None => {}
        }
        // menu aberto: troca de menu da barra ao passar o mouse; destaque
        if let Some(m) = self.menu {
            if my < MENU_H {
                if let MenuKind::Bar(cur) = m.kind {
                    if let Some(k) = self.menubar_hit(mx) {
                        if k != cur { self.open_bar_menu(k); }
                    }
                }
            }
            if m.rect.inset(-2).contains(mx, my) || m.rect.inset(-2).contains(self.last_cx, self.last_cy) {
                self.damage(m.rect.inset(-2));
            }
        }
        if self.cc_open { self.damage(self.cc_rect()); }
        if self.spot { self.damage(self.spot_rect().inset(-64)); }
        // dock (ampliacao e rotulo)
        let dz = self.dock_zone();
        if dz.contains(mx, my) || self.dock_hover >= 0 {
            self.damage(self.dock_area.inset(-4));
            self.layout_dock();
            self.damage(self.dock_area.inset(-4));
        }
        // semaforos e realces das janelas internas
        let lights = self.win_at(mx, my).filter(|&i| self.wins[i].lights().contains(mx, my));
        if lights != self.hover_light {
            if let Some(i) = self.hover_light { self.damage(self.wins[i].lights()); }
            if let Some(i) = lights { self.damage(self.wins[i].lights()); }
            self.hover_light = lights;
        }
        if let Some(i) = self.win_at(mx, my) {
            if self.wins[i].internal {
                let hr = self.phits[self.wins[i].app as usize].at(mx, my).map(|h| h.0).unwrap_or(Rect::empty());
                if hr != self.hover_hit {
                    let o = self.hover_hit;
                    self.damage(o.inset(-2));
                    self.damage(hr.inset(-2));
                    self.hover_hit = hr;
                }
            }
        }
        // forma do cursor nas bordas de redimensionamento
        let shape = match self.win_at(mx, my).map(|i| self.resize_edges(i, mx, my)) {
            Some(3) => cursor::Shape::ResizeDiag,
            Some(1) | Some(4) => cursor::Shape::ResizeH,
            Some(2) => cursor::Shape::ResizeV,
            _ => cursor::Shape::Arrow,
        };
        self.set_cursor(shape);
        self.last_cx = mx;
        self.last_cy = my;
    }

    /// Bordas de redimensionamento sob o ponto: 1 direita, 2 base, 4 esquerda.
    fn resize_edges(&self, i: usize, x: i32, y: i32) -> u8 {
        let w = &self.wins[i];
        if w.state != WState::Normal { return 0; }
        let r = w.r;
        let mut e = 0;
        if x >= r.right() - RESIZE_M && x < r.right() + RESIZE_M && y > r.y + TITLE_H { e |= 1; }
        if y >= r.bottom() - RESIZE_M && y < r.bottom() + RESIZE_M { e |= 2; }
        if x < r.x + RESIZE_M && x >= r.x - RESIZE_M && y > r.y + TITLE_H { e |= 4; }
        if e & 2 != 0 && e & 1 == 0 && x >= r.right() - 14 { e |= 1; }
        e
    }

    fn close_overlays(&mut self) -> bool {
        let mut any = false;
        if let Some(m) = self.menu.take() {
            self.damage(m.rect.inset(-24));
            self.damage(Rect::new(0, 0, self.w, MENU_H));
            any = true;
        }
        if self.cc_open {
            self.cc_open = false;
            self.damage(self.cc_rect().inset(-24));
            any = true;
        }
        if self.spot {
            self.spot = false;
            self.damage_all();
            any = true;
        }
        any
    }

    fn on_press(&mut self) {
        self.down = true;
        let (mx, my) = (self.mx, self.my);
        let now = uptime_ms();
        let double = now - self.last_click < 450 && (mx - self.last_cx).abs() < 6 && (my - self.last_cy).abs() < 6;
        self.last_click = if double { 0 } else { now };
        self.last_cx = mx;
        self.last_cy = my;

        // 1) dialogo modal
        if self.dialog != Dialog::None {
            if let Some((_, a)) = self.dialog_hits.at(mx, my) { self.do_action(a); }
            return;
        }
        // 2) busca
        if self.spot {
            let r = self.spot_rect();
            if r.contains(mx, my) {
                let k = (my - r.y - 60) / 40;
                if k >= 0 && (k as usize) < self.spot_n {
                    self.spot_sel = k as usize;
                    self.spot_activate();
                }
                return;
            }
            self.close_overlays();
            return;
        }
        // 3) menus e central de controle
        if let Some(m) = self.menu {
            if let Some((_, a)) = self.menu_hits.at(mx, my) {
                if m.rect.contains(mx, my) {
                    self.do_action(a);
                    self.close_overlays();
                    return;
                }
            }
            if m.rect.contains(mx, my) { return; }
            let on_bar = my < MENU_H;
            self.close_overlays();
            if !on_bar { return; }
        }
        if self.cc_open {
            if self.cc_rect().contains(mx, my) {
                if let Some((_, a)) = self.cc_hits.at(mx, my) {
                    self.do_action(a);
                    self.damage(self.cc_rect());
                }
                return;
            }
            self.close_overlays();
            if my >= MENU_H { return; }
        }
        // 4) barra de menus
        if my < MENU_H {
            if let Some(k) = self.menubar_hit(mx) {
                self.open_bar_menu(k);
            } else if mx > self.w - 320 {
                match self.status_hit(mx) {
                    1 => self.do_action(Action::ControlCenter),
                    2 => self.open_spotlight(),
                    3 => self.launch(APP_MONITOR, -1),
                    4 => self.launch(APP_NET, -1),
                    _ => self.do_action(Action::ControlCenter),
                }
            }
            return;
        }
        // 5) dock
        if let Some(app) = self.dock_hit(mx, my) {
            match self.find_app_win(app) {
                Some(i) if self.wins[i].minimized => self.raise(i),
                Some(i) if self.focused() == Some(i) && self.cfg.dock_minimize => self.minimize(i),
                _ => self.launch(app, -1),
            }
            return;
        }
        // 6) janelas
        if let Some(i) = self.win_at(mx, my) {
            self.raise(i);
            let w = self.wins[i];
            let edges = self.resize_edges(i, mx, my);
            if edges != 0 {
                self.drag = Drag::Resize { win: i, edges, start: w.r, mx, my };
                return;
            }
            // semaforos
            let ly = w.r.y + 19;
            for (k, cx) in [w.r.x + 20, w.r.x + 40, w.r.x + 60].iter().enumerate() {
                if (mx - cx).abs() <= 8 && (my - ly).abs() <= 8 {
                    match k {
                        0 => self.close_window(i),
                        1 => self.minimize(i),
                        _ => self.toggle_zoom(i),
                    }
                    return;
                }
            }
            if w.internal {
                if let Some((_, a)) = self.phits[w.app as usize].at(mx, my) {
                    if let Action::FileSel(f) = a {
                        self.do_action(a);
                        if double { self.open_file(f as usize, false); }
                    } else {
                        self.do_action(a);
                    }
                    return;
                }
                // area livre do topo arrasta a janela
                if my < w.r.y + TOOLBAR_H {
                    if double { self.toggle_zoom(i); return; }
                    self.drag = Drag::Move { win: i, dx: mx - w.r.x, dy: my - w.r.y, moved: false };
                }
                return;
            }
            if my < w.r.y + TITLE_H {
                if double { self.toggle_zoom(i); return; }
                self.drag = Drag::Move { win: i, dx: mx - w.r.x, dy: my - w.r.y, moved: false };
                return;
            }
            // clique no conteudo de um app
            self.capture = Some(i);
            let w = &mut self.wins[i];
            w.click = true;
            if double { w.dbl = true; }
            return;
        }
        // 7) area de trabalho
        if self.panels.files_sel >= 0 {
            self.panels.files_sel = -1;
            if let Some(w) = self.find_app_win(APP_FILES) { self.damage(self.wins[w].r); }
        }
    }

    fn on_release(&mut self) {
        self.down = false;
        self.capture = None;
        if let Drag::Move { win, moved, .. } = self.drag {
            if let Some((r, st)) = self.snap.take() {
                self.damage(r.inset(-4));
                if moved { self.set_state(win, st); }
            }
        }
        self.drag = Drag::None;
    }

    fn on_rclick(&mut self) {
        let (mx, my) = (self.mx, self.my);
        if self.dialog != Dialog::None || self.spot { return; }
        self.close_overlays();
        if let Some(app) = self.dock_hit(mx, my) {
            let running = self.find_app_win(app);
            let items = if running.is_some() { M_DOCK_RUNNING } else { M_DOCK_IDLE };
            let r = ui::menu_rect(mx - 20, my - 10 - ui::menu_rect(0, 0, items, self.w).h, items, self.w);
            self.menu = Some(Menu { kind: MenuKind::Context, rect: r, items, target_win: running, target_app: app });
            self.damage(r.inset(-24));
            return;
        }
        if my < MENU_H { return; }
        if let Some(i) = self.win_at(mx, my) {
            self.raise(i);
            let w = self.wins[i];
            if !w.internal && my >= w.r.y + TITLE_H {
                self.wins[i].rclick = true;
                return;
            }
            if w.app == APP_FILES {
                if let Some((_, Action::FileSel(f))) = self.phits[APP_FILES as usize].at(mx, my) {
                    self.do_action(Action::FileSel(f));
                    self.open_menu_at(mx, my, M_FILE, Some(i));
                    return;
                }
            }
            if my < w.r.y + TITLE_H.max(if w.internal { TOOLBAR_H } else { 0 }) {
                self.open_menu_at(mx, my, M_TITLEBAR, Some(i));
            }
            return;
        }
        self.open_menu_at(mx, my, M_DESKTOP, None);
    }

    fn open_menu_at(&mut self, x: i32, y: i32, items: &'static [MI], target: Option<usize>) {
        let mut r = ui::menu_rect(x, y, items, self.w);
        if r.bottom() > self.h - 4 { r.y = (y - r.h).max(MENU_H); }
        self.menu = Some(Menu { kind: MenuKind::Context, rect: r, items, target_win: target, target_app: 0 });
        self.damage(r.inset(-24));
    }

    fn on_wheel(&mut self, d: i32) {
        let (mx, my) = (self.mx, self.my);
        if let Some(i) = self.win_at(mx, my) {
            if self.wins[i].internal {
                if self.wins[i].app == APP_FILES {
                    self.panels.files_scroll -= d * 3 * 28;
                    self.damage(self.wins[i].r);
                }
            } else {
                self.wins[i].wheel += d;
            }
        }
    }

    fn on_key(&mut self, k: Key) {
        let code = k.code;
        // ---- atalhos globais ----
        if k.ctrl() && code == key::SPACE { return self.toggle_spotlight(); }
        if k.alt() && code == key::TAB { return self.cycle_switcher(k.shift()); }
        if self.switcher && code == key::ESC {
            self.switcher = false;
            self.damage_all();
            return;
        }
        if self.dialog != Dialog::None {
            match code {
                key::ENTER => self.do_action(Action::DialogConfirm),
                key::ESC => self.do_action(Action::DialogCancel),
                _ => {}
            }
            return;
        }
        if self.spot { return self.spotlight_key(k); }
        if code == key::ESC && self.close_overlays() { return; }
        if k.ctrl() && k.shift() && code == key::ESC { return self.launch(APP_MONITOR, -1); }
        if k.ctrl() && k.alt() && code == key::T { return self.launch(APP_TERMINAL, -1); }
        if (k.alt() && code == key::F4) || (k.ctrl() && (code == key::W || code == key::Q)) {
            if let Some(i) = self.focused() { self.close_window(i); }
            return;
        }
        if code == key::F1 { return self.launch(APP_HELP, -1); }
        if k.meta() {
            let f = self.focused();
            match code {
                key::UP => { if let Some(i) = f { self.set_state(i, WState::Max); } }
                key::DOWN => {
                    if let Some(i) = f {
                        if self.wins[i].state != WState::Normal { self.set_state(i, WState::Normal); } else { self.minimize(i); }
                    }
                }
                key::LEFT => { if let Some(i) = f { self.set_state(i, WState::SnapL); } }
                key::RIGHT => { if let Some(i) = f { self.set_state(i, WState::SnapR); } }
                key::D => self.show_desktop(),
                key::E => self.launch(APP_FILES, -1),
                key::I => self.do_action(Action::Settings(0)),
                _ => {}
            }
            return;
        }
        if code == key::F11 {
            if let Some(i) = self.focused() { self.toggle_zoom(i); }
            return;
        }
        // ---- janela com foco ----
        let Some(i) = self.focused() else { return };
        if self.wins[i].internal {
            if self.wins[i].app == APP_FILES { self.files_key(k); }
            if code == key::ESC && self.wins[i].app != APP_FILES { self.close_window(i); }
            return;
        }
        if k.code == key::LSHIFT || k.code == key::RSHIFT || k.code == key::LCTRL || k.code == key::RCTRL
            || k.code == key::LALT || k.code == key::RALT { return; }
        self.wins[i].push_key(k.pack());
    }

    fn files_key(&mut self, k: Key) {
        let mut vis = [0u16; 64];
        let n = panels::visible_files(&self.files[..self.nfiles], self.panels.files_filter, &mut vis);
        if n == 0 { return; }
        let cur = (0..n).find(|&j| vis[j] as i32 == self.panels.files_sel);
        let step = if self.panels.files_view == 1 { 4 } else { 1 };
        let next = match k.code {
            key::DOWN => Some(cur.map(|c| (c + step).min(n - 1)).unwrap_or(0)),
            key::UP => Some(cur.map(|c| c.saturating_sub(step)).unwrap_or(0)),
            key::RIGHT if step > 1 => Some(cur.map(|c| (c + 1).min(n - 1)).unwrap_or(0)),
            key::LEFT if step > 1 => Some(cur.map(|c| c.saturating_sub(1)).unwrap_or(0)),
            key::ENTER => {
                if self.panels.files_sel >= 0 { self.open_file(self.panels.files_sel as usize, false); }
                None
            }
            _ if k.ctrl() && k.code == key::N => { self.new_text_doc(); None }
            _ => None,
        };
        if let Some(j) = next {
            self.panels.files_sel = vis[j] as i32;
            if let Some(w) = self.find_app_win(APP_FILES) { self.damage(self.wins[w].r); }
        }
    }

    // ===================================================== busca (Spotlight)

    fn spot_rect(&self) -> Rect {
        let w = 640;
        Rect::new((self.w - w) / 2, self.h / 5, w, 56 + if self.query.n > 0 { 8 + 40 * self.spot_n.max(1) as i32 } else { 0 })
    }

    fn open_spotlight(&mut self) {
        self.close_overlays();
        self.spot = true;
        self.query.clear();
        self.spot_sel = 0;
        self.spot_n = 0;
        self.damage_all();
    }

    fn toggle_spotlight(&mut self) {
        if self.spot { self.close_overlays(); } else { self.open_spotlight(); }
    }

    fn spotlight_key(&mut self, k: Key) {
        let old = self.spot_rect();
        match k.code {
            key::ESC => { self.close_overlays(); return; }
            key::ENTER | key::KPENTER => { self.spot_activate(); return; }
            key::DOWN => { if self.spot_n > 0 { self.spot_sel = (self.spot_sel + 1) % self.spot_n; } }
            key::UP => { if self.spot_n > 0 { self.spot_sel = (self.spot_sel + self.spot_n - 1) % self.spot_n; } }
            key::BACKSPACE => {
                // remove um caractere UTF-8 inteiro
                while self.query.n > 0 {
                    self.query.n -= 1;
                    if self.query.b[self.query.n] & 0xC0 != 0x80 { break; }
                }
                self.search();
            }
            _ => {
                if k.ch >= 32 && self.query.n < 60 {
                    self.query.ch(k.ch);
                    self.search();
                }
            }
        }
        self.damage(old.inset(-64));
        self.damage(self.spot_rect().inset(-64));
    }

    fn search(&mut self) {
        self.spot_n = 0;
        self.spot_sel = 0;
        let q = self.query;
        let q = q.get();
        if q.is_empty() { return; }
        if let Some(v) = eval(q) {
            self.spot_hits[0] = Hit::Calc(v);
            self.spot_n = 1;
        }
        for a in APPS.iter() {
            if self.spot_n < 8 && (contains_ci(a.name.as_bytes(), q) || contains_ci(a.keywords.as_bytes(), q)) {
                self.spot_hits[self.spot_n] = Hit::App(a.id);
                self.spot_n += 1;
            }
        }
        for (s, a, i) in SEARCH_CMDS.iter() {
            if self.spot_n < 8 && contains_ci(s.as_bytes(), q) {
                self.spot_hits[self.spot_n] = Hit::Cmd(*a, s, *i);
                self.spot_n += 1;
            }
        }
        for f in 0..self.nfiles {
            if self.spot_n < 8 && contains_ci(self.files[f].name(), q) {
                self.spot_hits[self.spot_n] = Hit::File(f as u16);
                self.spot_n += 1;
            }
        }
    }

    fn spot_activate(&mut self) {
        if self.spot_n == 0 { return; }
        let h = self.spot_hits[self.spot_sel];
        self.close_overlays();
        match h {
            Hit::App(id) => self.launch(id, -1),
            Hit::File(f) => self.open_file(f as usize, false),
            Hit::Cmd(a, _, _) => self.do_action(a),
            Hit::Calc(_) => {}
        }
    }

    // ===================================================== Alt+Tab

    fn cycle_switcher(&mut self, back: bool) {
        if !self.switcher {
            // lista por ordem de uso (z decrescente), inclusive minimizadas
            let mut ids = [0usize; MAXWIN];
            let mut n = 0;
            for i in 0..MAXWIN { if self.wins[i].used { ids[n] = i; n += 1; } }
            for a in 1..n {
                let mut b = a;
                while b > 0 && self.wins[ids[b - 1]].z < self.wins[ids[b]].z { ids.swap(b - 1, b); b -= 1; }
            }
            if n == 0 { return; }
            self.sw_list = ids;
            self.sw_n = n;
            self.sw_sel = if n > 1 { 1 } else { 0 };
            self.switcher = true;
        } else if self.sw_n > 0 {
            self.sw_sel = if back { (self.sw_sel + self.sw_n - 1) % self.sw_n } else { (self.sw_sel + 1) % self.sw_n };
        }
        self.damage(self.switcher_rect().inset(-30));
    }

    fn switcher_rect(&self) -> Rect {
        let w = self.sw_n as i32 * 104 + 32;
        Rect::new((self.w - w) / 2, (self.h - 150) / 2, w, 150)
    }

    fn finish_switcher(&mut self) {
        self.switcher = false;
        if self.sw_sel < self.sw_n {
            let i = self.sw_list[self.sw_sel];
            if self.wins[i].used { self.raise(i); }
        }
        self.damage_all();
    }

    // ===================================================== barra de menus

    fn bar_titles(&self) -> ([i32; 8], [&'static str; 8], usize) {
        let mut xs = [0i32; 8];
        let mut names = [""; 8];
        let f = self.focused().map(|i| self.wins[i].app);
        let app_name = f.map(|a| catalog::app(a).name).unwrap_or("Pulsar");
        let menus = f.map(|a| catalog::app(a).menus).unwrap_or(&[]);
        let mut x = 12;
        xs[0] = x;
        names[0] = "";
        x += 30;
        xs[1] = x;
        names[1] = app_name;
        x += FONT_UI_BOLD.width(app_name.as_bytes()) + 22;
        let mut n = 2;
        for (title, _) in menus.iter() {
            if n >= 6 { break; }
            xs[n] = x;
            names[n] = title;
            x += FONT_UI.width(title.as_bytes()) + 22;
            n += 1;
        }
        for t in ["Janela", "Ajuda"] {
            xs[n] = x;
            names[n] = t;
            x += FONT_UI.width(t.as_bytes()) + 22;
            n += 1;
        }
        (xs, names, n)
    }

    fn menubar_hit(&self, mx: i32) -> Option<usize> {
        let (xs, names, n) = self.bar_titles();
        for k in 0..n {
            let w = if k == 0 { 26 } else if k == 1 { FONT_UI_BOLD.width(names[1].as_bytes()) + 16 } else { FONT_UI.width(names[k].as_bytes()) + 16 };
            if mx >= xs[k] - 8 && mx < xs[k] - 8 + w { return Some(k); }
        }
        None
    }

    fn open_bar_menu(&mut self, k: usize) {
        if let Some(m) = self.menu.take() { self.damage(m.rect.inset(-24)); }
        let (xs, names, n) = self.bar_titles();
        let f = self.focused();
        let fa = f.map(|i| self.wins[i].app);
        let items: &'static [MI] = match k {
            0 => M_PULSAR,
            1 => match fa {
                Some(a) if catalog::app(a).kind == Kind::Process => &APP_MENU_PROC,
                Some(_) => &APP_MENU_INTERNAL,
                None => M_PULSAR,
            },
            _ if k == n - 2 => M_WINDOW,
            _ if k == n - 1 => M_HELP,
            _ => {
                let menus = fa.map(|a| catalog::app(a).menus).unwrap_or(&[]);
                menus.iter().find(|(t, _)| *t == names[k]).map(|m| m.1).unwrap_or(&[])
            }
        };
        let r = ui::menu_rect(xs[k] - 8, MENU_H, items, self.w);
        self.menu = Some(Menu { kind: MenuKind::Bar(k), rect: r, items, target_win: f, target_app: 0 });
        self.damage(r.inset(-24));
        self.damage(Rect::new(0, 0, self.w, MENU_H));
    }

    /// Itens de status a direita: 1 central de controle, 2 busca, 3 CPU, 4 rede.
    fn status_hit(&self, mx: i32) -> u8 {
        let x = self.w - 14 - FONT_UI.width(self.clock.get());
        if mx >= x { return 1; }
        let x = x - 30;
        if mx >= x { return 1; }
        let x = x - 28;
        if mx >= x { return 2; }
        let x = x - 28;
        if mx >= x { return 4; }
        3
    }

    // ===================================================== dock

    fn dock_zone(&self) -> Rect {
        let s = self.cfg.icon_size();
        let depth = s * 2 + 30;
        if self.cfg.dock_left { Rect::new(0, MENU_H, depth, self.h - MENU_H) }
        else { Rect::new(0, self.h - depth, self.w, depth) }
    }

    fn dock_items(&self, out: &mut [u8; 16]) -> (usize, usize) {
        let mut n = 0;
        for &a in DOCK.iter() { out[n] = a; n += 1; }
        let sep = n;
        out[n] = DOCK_TAIL;
        n += 1;
        for w in self.wins.iter() {
            if w.used && !DOCK.contains(&w.app) && w.app != DOCK_TAIL && !out[..n].contains(&w.app) && n < 16 {
                out[n] = w.app;
                n += 1;
            }
        }
        (n, sep)
    }

    /// Calcula a posicao (ampliada) de cada icone e o retangulo do dock.
    fn layout_dock(&mut self) {
        let s = self.cfg.icon_size();
        let pad = s * 18 / 100;
        let gap = s * 14 / 100;
        let sepw = gap * 2 + 1;
        let mut items = [0u8; 16];
        let (n, sep) = self.dock_items(&mut items);
        let horiz = !self.cfg.dock_left;
        let along_m = if horiz { self.mx } else { self.my };
        let near = self.dock_zone().contains(self.mx, self.my) && self.dialog == Dialog::None && !self.spot;
        // posicoes base (sem ampliacao)
        let total0 = n as i32 * s + (n as i32 - 1) * gap + sepw;
        let screen_len = if horiz { self.w } else { self.h - MENU_H };
        let start0 = (if horiz { 0 } else { MENU_H }) + (screen_len - total0) / 2;
        let mut sizes = [s; 16];
        let mut centers = [0i32; 16];
        let mut p = start0;
        for k in 0..n {
            if k == sep { p += sepw; }
            centers[k] = p + s / 2;
            p += s + gap;
        }
        if near && self.cfg.dock_magnify {
            let range = s * 5 / 2;
            for k in 0..n {
                let d = (along_m - centers[k]).abs();
                if d < range {
                    let f = 256 - d * d * 256 / (range * range); // 0..256
                    // tamanhos pares: metade das variantes no cache de sprites
                    sizes[k] = (s + s * 6 / 10 * f / 256) & !1;
                }
            }
        }
        let total: i32 = sizes[..n].iter().sum::<i32>() + (n as i32 - 1) * gap + sepw;
        let start = (if horiz { 0 } else { MENU_H }) + (screen_len - total) / 2;
        let thick = s + pad * 2;
        let margin = 6;
        let base = if horiz { Rect::new(start - pad, self.h - margin - thick, total + pad * 2, thick) }
            else { Rect::new(margin, start - pad, thick, total + pad * 2) };
        let mut p = start;
        self.dock_n = n;
        self.dock_hover = -1;
        let now = uptime_ms();
        for k in 0..n {
            if k == sep { p += sepw; }
            let sz = sizes[k];
            let mut lift = 0;
            let b = self.bounce[items[k] as usize];
            if b > now {
                let t = ((b - now) * 256 / 700) as i32; // 256 -> 0
                lift = gfx::sin256(256 - t).abs() * (s / 2) / 1024 * t / 256;
            }
            let r = if horiz { Rect::new(p, base.bottom() - pad - sz - lift, sz, sz) }
                else { Rect::new(base.x + pad + lift, p, sz, sz) };
            self.dock_rects[k] = (r, items[k]);
            if near && r.contains(self.mx, self.my) { self.dock_hover = k as i32; }
            p += sz + gap;
        }
        let mut area = base;
        for k in 0..n { area = area.union(&self.dock_rects[k].0); }
        self.dock_area = area.inset(-12).union(&Rect::new(area.x, area.y - 40, area.w, 40));
    }

    fn dock_hit(&self, x: i32, y: i32) -> Option<u8> {
        (0..self.dock_n).find(|&k| self.dock_rects[k].0.contains(x, y)).map(|k| self.dock_rects[k].1)
    }

    // ===================================================== periodico

    fn fast_tick(&mut self, now: u64) {
        // apps que morreram
        for i in 0..MAXWIN {
            let w = self.wins[i];
            if !w.used || w.internal || w.pid < 0 { continue; }
            if !is_alive(w.pid as usize) {
                self.wins[i].used = false;
                if self.capture == Some(i) { self.capture = None; }
                if let Drag::Move { win, .. } | Drag::Resize { win, .. } = self.drag { if win == i { self.drag = Drag::None; } }
                self.damage(w.outer());
                self.damage(Rect::new(0, 0, self.w, MENU_H));
                self.damage(self.dock_area);
                if !w.want_close {
                    let crashed = last_crash() == Some(w.pid as usize);
                    let def = catalog::app(w.app);
                    self.toast(def.icon, if crashed { "O aplicativo travou" } else { "O aplicativo foi encerrado" },
                        if crashed { "O kernel isolou a falha; o resto do sistema segue normal." } else { "O processo terminou." });
                }
            } else if w.want_close && now > w.close_at {
                // nao respondeu ao pedido de fechar: "finalizar tarefa"
                kill(w.pid as usize);
                self.wins[i].close_at = now + 100_000;
                let def = catalog::app(w.app);
                self.toast(def.icon, "O aplicativo não respondia", "Ele foi encerrado à força.");
            }
        }
        // animacao do dock (pulo ao abrir um app)
        if self.bounce.iter().any(|&b| b != 0) {
            for b in self.bounce.iter_mut() { if *b != 0 && *b <= now { *b = 0; } }
            self.damage(self.dock_area.inset(-4));
            self.layout_dock();
            self.damage(self.dock_area.inset(-4));
        }
        // avisos que expiraram
        for k in 0..3 {
            if self.toasts[k].active && now > self.toasts[k].until {
                self.toasts[k].active = false;
                self.damage(self.toast_area());
            }
        }
        // cursor piscando na busca
        let caret = (now / 530) % 2 == 0;
        if self.spot && caret != self.caret_on {
            self.caret_on = caret;
            self.damage(self.spot_rect());
        }
    }

    fn slow_tick(&mut self, now: u64) {
        if self.update_clock() { self.damage(Rect::new(self.w - 340, 0, 340, MENU_H)); }
        let mut changed = false;
        for c in 0..8 {
            let l = cpu_load(c).unwrap_or(0);
            if l != self.loads[c] { self.loads[c] = l; changed = true; }
        }
        if changed {
            if self.cfg.cpu_meter { self.damage(Rect::new(self.w - 400, 0, 400, MENU_H)); }
            if self.cc_open { self.damage(self.cc_rect()); }
        }
        if (now / 500) % 4 == 0 {
            let n = fs_count().min(MAX_FILES);
            if n != self.nfiles {
                self.refresh_files();
                if let Some(w) = self.find_app_win(APP_FILES) { self.damage(self.wins[w].r); }
            }
            self.watch_config();
            self.sys = sysinfo();
            for app in [APP_ABOUT, APP_SETTINGS] {
                if let Some(w) = self.find_app_win(app) { self.damage(self.wins[w].r); }
            }
        }
    }

    // ===================================================== composicao

    fn render(&mut self) {
        // o dock muda de tamanho quando apps entram/saem: repinta as duas areas
        let old_dock = self.dock_area;
        self.layout_dock();
        if self.dock_area != old_dock {
            self.damage(old_dock.union(&self.dock_area));
        }
        if self.ndmg == 0 { return self.flush_commits(); }
        let rects = self.dmg;
        let n = self.ndmg;
        self.ndmg = 0;
        let t0 = perf_ms();
        for k in 0..n {
            self.compose_split(rects[k], 0);
        }
        let t1 = perf_ms();
        for k in 0..n {
            fb_present(rects[k]);
        }
        let t2 = perf_ms();
        self.flush_commits();
        if PERF_LOG {
            let p = unsafe { &mut *(&raw mut PERF) };
            p.0 += 1;
            p.1 += t1 - t0;
            p.2 += t2 - t1;
            for k in 0..n { p.3 += (rects[k].w * rects[k].h) as u64; }
            if p.0 == 60 {
                let mut b = Buf::<160>::new();
                b.s("[wm] 60 quadros: compor ").u(p.1 * 10 / p.0).s("/10 ms, apresentar ")
                    .u(p.2 * 10 / p.0).s("/10 ms, ").u(p.3 / p.0 / 1000).s(" kpx/quadro");
                let ph = unsafe { &mut *(&raw mut PHASE) };
                b.s(" [nd ").u(ph[4]).s(" nc ").u(ph[5]).s(" fundo ").u(ph[0]).s(" jan ").u(ph[1]).s(" dock ").u(ph[2]).s(" resto ").u(ph[3]).s("]\n");
                *ph = [0; 6];
                write_bytes(b.get());
                *p = (0, 0, 0, 0);
            }
        }
    }

    /// Libera os apps que aguardavam a composicao do seu quadro.
    fn flush_commits(&mut self) {
        for k in 0..self.npending {
            let (pid, i) = self.pending[k];
            let mut r = Msg::new(WM_OP_COMMIT);
            if self.wins[i].used && self.wins[i].pid == pid as i64 { self.fill_poll(i, &mut r); }
            reply(pid, &r);
        }
        self.npending = 0;
    }

    /// Divide o dano em volta da area opaca da janela mais alta que o
    /// cruza: a parte coberta comeca nessa janela (sem papel de parede nem
    /// janelas de baixo) e so as sobras passam pelo caminho completo.
    /// Arrastar uma janela deixa de repintar tudo que fica por baixo dela.
    fn compose_split(&mut self, dmg: Rect, depth: u32) {
        if depth < 3 {
            let mut order = [0usize; MAXWIN];
            let n = self.z_order(&mut order);
            for k in (0..n).rev() {
                let w = &self.wins[order[k]];
                let o = w.opaque().intersect(&dmg);
                if o.is_empty() { continue; }
                if !(w.internal || w.committed) || o.contains_rect(&dmg) { break; }
                if (o.w * o.h) * 4 < dmg.w * dmg.h { break; } // nao compensa
                self.compose(o);
                let pieces = [
                    Rect::new(dmg.x, dmg.y, dmg.w, o.y - dmg.y),
                    Rect::new(dmg.x, o.bottom(), dmg.w, dmg.bottom() - o.bottom()),
                    Rect::new(dmg.x, o.y, o.x - dmg.x, o.h),
                    Rect::new(o.right(), o.y, dmg.right() - o.right(), o.h),
                ];
                for p in pieces {
                    if !p.is_empty() { self.compose_split(p, depth + 1); }
                }
                return;
            }
        }
        self.compose(dmg);
    }

    fn compose(&mut self, dmg: Rect) {
        if PERF_LOG { unsafe { PHASE[5] += 1; } }
        let mut scr = self.scr;
        scr.set_clip(dmg);
        // janelas: comeca pela mais alta que cobre todo o dano (oclusao)
        let mut order = [0usize; MAXWIN];
        let n = self.z_order(&mut order);
        let mut start = 0;
        let mut covered = false;
        for k in (0..n).rev() {
            let w = &self.wins[order[k]];
            if w.opaque().contains_rect(&dmg) && (w.internal || w.committed) {
                start = k;
                covered = true;
                break;
            }
        }
        let t0 = perf_ms();
        if !covered { scr.copy_from(&self.wall, dmg); }
        let t1 = perf_ms();
        let focus = self.focused();
        for k in start..n {
            let i = order[k];
            if self.wins[i].outer().overlaps(&dmg) {
                self.draw_window(&scr, i, focus == Some(i));
            }
        }
        let t2 = perf_ms();
        if let Some((r, _)) = self.snap {
            scr.round_rect_a(r.inset(6), 14, self.th.accent, 50);
            scr.round_border(r.inset(6), 14, self.th.accent, 200);
        }
        let t3 = perf_ms();
        if self.dock_area.overlaps(&dmg) { self.draw_dock(&scr); if PERF_LOG { unsafe { PHASE[4] += 1; } } }
        let t4 = perf_ms();
        if dmg.y < MENU_H { self.draw_menubar(&scr); }
        if let Some(m) = self.menu {
            if m.rect.inset(-24).overlaps(&dmg) {
                let th = self.th;
                let cfg = self.cfg;
                let checks = |a: Action| match a {
                    Action::FilesView(v) => self.panels.files_view == v,
                    Action::FilesFilter(f) => self.panels.files_filter == f,
                    Action::ToggleDark => cfg.dark,
                    _ => false,
                };
                let mut hits = Hits::new();
                ui::menu(&scr, m.rect, m.items, self.mx, self.my, &th, &mut hits, &checks);
                self.menu_hits = hits;
            }
        }
        if self.cc_open { self.draw_control_center(&scr); }
        self.draw_toasts(&scr);
        if self.spot { self.draw_spotlight(&scr); }
        if self.switcher { self.draw_switcher(&scr); }
        if self.dialog != Dialog::None { self.draw_dialog(&scr); }
        if PERF_LOG {
            let ph = unsafe { &mut *(&raw mut PHASE) };
            ph[0] += t1 - t0; ph[1] += t2 - t1; ph[2] += t4 - t3;
            ph[3] += perf_ms() - t4 + t3 - t2;
        }
    }

    fn draw_window(&mut self, scr: &Fb, i: usize, focused: bool) {
        let w = self.wins[i];
        let th = self.th;
        let r = w.r;
        let dark = th.dark;
        scr.shadow(r, WIN_RAD, if focused { 30 } else { 18 }, if focused { 12 } else { 6 },
            if focused { if dark { 160 } else { 95 } } else if dark { 110 } else { 55 });
        if w.internal {
            let ctx = Ctx {
                scr, th: &th, cfg: &self.cfg, focused, mx: self.mx, my: self.my,
                files: &self.files[..self.nfiles], thumbs: thumbs(), sys: &self.sys,
                disk_free_kb: 8192u32.saturating_sub(self.files[..self.nfiles].iter().map(|f| f.size.div_ceil(1024)).sum::<u32>()),
            };
            let hits = &mut self.phits[w.app as usize];
            hits.clear();
            match w.app {
                APP_FILES => panels::draw_files(&ctx, r, &mut self.panels, hits),
                APP_SETTINGS => panels::draw_settings(&ctx, r, &self.panels, hits),
                APP_ABOUT => panels::draw_about(&ctx, r, hits),
                _ => panels::draw_help(&ctx, r),
            }
            if w.app == APP_ABOUT || w.app == APP_HELP {
                let tr = Rect::new(r.x + 80, r.y, r.w - 160, TITLE_H);
                scr.text_center(&FONT_UI_BOLD, tr, w.title(), if focused { th.text } else { th.text3 });
            }
        } else {
            let c0 = w.client();
            let covers = w.committed && w.slot >= 0 && w.commit_w >= c0.w && w.commit_h >= c0.h;
            if !covers { scr.round_rect(r, WIN_RAD, th.win_bg); }
            let tb = Rect::new(r.x, r.y, r.w, TITLE_H + WIN_RAD);
            scr.round_rect_grad(tb, WIN_RAD, th.title_top, th.title_bot, 255);
            scr.fill(Rect::new(r.x, r.y + TITLE_H - 1, r.w, 1), th.sep);
            let c = w.client();
            if w.committed && w.slot >= 0 {
                let src = Fb::new(surface_ptr(w.slot as u64), SURF_W, SURF_H, SURF_W);
                let (bw, bh) = (c.w.min(w.commit_w), c.h.min(w.commit_h));
                let mut cc = *scr;
                cc.set_clip(scr.clip.intersect(&c));
                cc.blit_rounded(&src, 0, 0, bw, bh, c.x, c.y, WIN_RAD, false, bh >= c.h);
            } else {
                // ainda carregando: indicador discreto
                let cx = c.x + c.w / 2;
                let cy = c.y + c.h / 2;
                let t = (uptime_ms() / 80) as i32;
                for k in 0..8 {
                    let a = k * 32;
                    let alpha = 40 + ((k + t) % 8) as u32 * 25;
                    scr.line16(cx * 16 + 9 * gfx::sin256(a) / 64, cy * 16 - 9 * gfx::cos256(a) / 64,
                        cx * 16 + 16 * gfx::sin256(a) / 64, cy * 16 - 16 * gfx::cos256(a) / 64, 40, th.text2, alpha);
                }
                self.damage(Rect::new(cx - 20, cy - 20, 40, 40));
            }
            let tr = Rect::new(r.x + 76, r.y, r.w - 152, TITLE_H);
            let tw = FONT_UI_BOLD.width(w.title()).min(tr.w);
            scr.text_clip(&FONT_UI_BOLD, tr.x + (tr.w - tw) / 2, r.y + (TITLE_H - FONT_UI_BOLD.height) / 2, tr.w,
                w.title(), if focused { th.text } else { th.text3 });
        }
        // semaforos
        let hover = self.hover_light == Some(i);
        let cols = [0xFF5F57, 0xFEBC2E, 0x28C840];
        for (k, col) in cols.iter().enumerate() {
            let (cx, cy) = (r.x + 20 + k as i32 * 20, r.y + 19);
            let c = if focused || hover { *col } else if dark { 0x4A4B52 } else { 0xD3D3D8 };
            scr.disc(cx, cy, 6, c, 255);
            scr.ring(cx, cy, 6, 1, 0x000000, if focused || hover { 30 } else { 15 });
            if hover {
                let g = 0x5A1A10;
                match k {
                    0 => { scr.line(cx - 2, cy - 2, cx + 2, cy + 2, 1, g, 200); scr.line(cx + 2, cy - 2, cx - 2, cy + 2, 1, g, 200); }
                    1 => scr.line(cx - 3, cy, cx + 3, cy, 1, 0x7A4A00, 220),
                    _ => { scr.line(cx - 3, cy, cx + 3, cy, 1, 0x0A4A10, 220); scr.line(cx, cy - 3, cx, cy + 3, 1, 0x0A4A10, 220); }
                }
            }
        }
        scr.round_border(r, WIN_RAD, if dark { 0xFFFFFF } else { 0x000000 }, if dark { 34 } else { 40 });
    }

    fn draw_menubar(&mut self, scr: &Fb) {
        let th = self.th;
        let bar = Rect::new(0, 0, self.w, MENU_H);
        if self.cfg.glass {
            scr.copy_from(&self.blur, bar);
            scr.fill_a(bar, if th.dark { 0x101114 } else { 0xF6F7FA }, if th.dark { 130 } else { 150 });
        } else {
            scr.fill(bar, th.title_top);
        }
        scr.hline(0, MENU_H - 1, self.w, 0x000000, if th.dark { 90 } else { 30 });
        let (xs, names, n) = self.bar_titles();
        let open = match self.menu { Some(Menu { kind: MenuKind::Bar(k), .. }) => Some(k), _ => None };
        for k in 0..n {
            let font = if k == 1 { &FONT_UI_BOLD } else { &FONT_UI };
            let w = if k == 0 { 26 } else { font.width(names[k].as_bytes()) + 16 };
            if open == Some(k) {
                scr.round_rect_a(Rect::new(xs[k] - 8, 3, w, MENU_H - 6), 5, th.text, 40);
            }
            if k == 0 {
                icons::logo(scr, (xs[0] + 5) * 16, (MENU_H / 2) * 16, 7 * 16, th.text);
            } else {
                scr.text(font, xs[k], (MENU_H - FONT_UI.height) / 2, names[k].as_bytes(), th.text);
            }
        }
        // area de status (direita para a esquerda)
        let mut x = self.w - 14;
        let cw = FONT_UI.width(self.clock.get());
        x -= cw;
        scr.text(&FONT_UI, x, (MENU_H - FONT_UI.height) / 2, self.clock.get(), th.text);
        x -= 30;
        icons::sym(scr, Sym::Toggles, x + 6, 7, 15, th.text);
        x -= 28;
        icons::sym(scr, Sym::Search, x + 6, 7, 14, th.text);
        x -= 28;
        if self.net_up { icons::sym(scr, Sym::Network, x + 5, 6, 16, th.text); }
        if self.cfg.cpu_meter {
            let nc = (self.sys.ncpus as usize).clamp(1, 8) as i32;
            x -= nc * 5 + 10;
            for c in 0..nc {
                let bx = x + c * 5;
                let full = 14;
                let hgt = (self.loads[c as usize] as i32 * full / 100).max(1);
                scr.fill_a(Rect::new(bx, 7, 3, full), th.text, 50);
                scr.fill(Rect::new(bx, 7 + full - hgt, 3, hgt), if self.loads[c as usize] > 80 { 0xFF453A } else { th.accent });
            }
        }
    }

    fn draw_dock(&mut self, scr: &Fb) {
        let th = self.th;
        let s = self.cfg.icon_size();
        let pad = s * 18 / 100;
        let n = self.dock_n;
        if n == 0 { return; }
        let mut items = [0u8; 16];
        let (_, sep) = self.dock_items(&mut items);
        let horiz = !self.cfg.dock_left;
        // fundo de vidro
        let first = self.dock_rects[0].0;
        let last = self.dock_rects[n - 1].0;
        let thick = s + pad * 2;
        let bg = if horiz {
            Rect::new(first.x - pad, self.h - 6 - thick, last.right() - first.x + pad * 2, thick)
        } else {
            Rect::new(6, first.y - pad, thick, last.bottom() - first.y + pad * 2)
        };
        let rad = (thick * 30 / 100).min(22);
        scr.shadow(bg, rad, 20, 6, if th.dark { 120 } else { 60 });
        ui::glass(scr, &self.blur, bg, rad, &th, self.cfg.glass);
        for k in 0..n {
            let (r, app) = self.dock_rects[k];
            if k == sep {
                if horiz {
                    let x = r.x - s * 14 / 100 - 1;
                    scr.vline(x, bg.y + pad, s, th.text, 50);
                } else {
                    let y = r.y - s * 14 / 100 - 1;
                    scr.hline(bg.x + pad, y, s, th.text, 50);
                }
            }
            let def = catalog::app(app);
            sprites::dock_icon(scr, def.icon, r);
            if self.dock_hover == k as i32 && self.down && self.dock_area.contains(self.mx, self.my) {
                scr.round_rect_a(r, r.w * 23 / 100, 0x000000, 70);
            }
            let running = self.wins.iter().any(|w| w.used && w.app == app);
            if running {
                if horiz { scr.disc(r.x + r.w / 2, bg.bottom() - 4, 2, th.text, 200); }
                else { scr.disc(bg.x + 4, r.y + r.h / 2, 2, th.text, 200); }
            }
        }
        if self.dock_hover >= 0 && self.menu.is_none() {
            let (r, app) = self.dock_rects[self.dock_hover as usize];
            let name = catalog::app(app).name.as_bytes();
            if horiz {
                ui::label_pill(scr, r.x + r.w / 2, r.y - 8, name, &th);
            } else {
                let w = FONT_UI.width(name) + 22;
                ui::label_pill(scr, r.right() + 18 + w / 2, r.y + r.h / 2 + 13, name, &th);
            }
        }
    }

    fn cc_rect(&self) -> Rect {
        Rect::new(self.w - 344, MENU_H + 6, 334, 420)
    }

    fn draw_control_center(&mut self, scr: &Fb) {
        let th = self.th;
        let r = self.cc_rect();
        let mut hits = Hits::new();
        scr.shadow(r, 16, 24, 8, if th.dark { 140 } else { 80 });
        ui::glass(scr, &self.blur, r, 16, &th, self.cfg.glass);
        if self.cfg.glass { scr.round_rect_a(r, 16, th.menu_bg, 90); }
        let tile = |x: i32, y: i32, w: i32, h: i32| -> Rect {
            let t = Rect::new(r.x + x, r.y + y, w, h);
            scr.round_rect_a(t, 12, if th.dark { 0xFFFFFF } else { 0xFFFFFF }, if th.dark { 26 } else { 170 });
            t
        };
        // rede
        let t = tile(12, 12, 150, 64);
        scr.disc(t.x + 26, t.y + 32, 15, if self.net_up { th.accent } else { th.text3 }, 255);
        icons::sym(scr, Sym::Network, t.x + 17, t.y + 22, 18, 0xFFFFFF);
        scr.text(&FONT_UI_BOLD, t.x + 50, t.y + 13, b"Rede", th.text);
        scr.text(&FONT_SMALL, t.x + 50, t.y + 34, if self.net_up { b"10.0.2.15" as &[u8] } else { b"Desconectado" }, th.text2);
        hits.push(t, Action::Launch(APP_NET));
        // modo escuro
        let t = tile(172, 12, 150, 64);
        scr.disc(t.x + 26, t.y + 32, 15, if self.cfg.dark { th.accent } else { th.text3 }, 255);
        icons::sym(scr, Sym::Moon, t.x + 17, t.y + 23, 18, 0xFFFFFF);
        scr.text(&FONT_UI_BOLD, t.x + 50, t.y + 13, b"Modo escuro", th.text);
        scr.text(&FONT_SMALL, t.x + 50, t.y + 34, if self.cfg.dark { b"Ativado" as &[u8] } else { b"Desativado" }, th.text2);
        hits.push(t, Action::ToggleDark);
        // CPUs
        let nc = (self.sys.ncpus as i32).clamp(1, 8);
        let t = tile(12, 86, 310, 40 + nc * 18);
        let mut b = Buf::<48>::new();
        b.s("Processadores · ").u(nc as u64).s(" núcleos");
        scr.text(&FONT_UI_BOLD, t.x + 14, t.y + 10, b.get(), th.text);
        for c in 0..nc {
            let y = t.y + 36 + c * 18;
            let l = self.loads[c as usize] as i32;
            let mut lb = Buf::<16>::new();
            lb.s("CPU ").u(c as u64);
            scr.text(&FONT_SMALL, t.x + 14, y - 1, lb.get(), th.text2);
            let bar = Rect::new(t.x + 64, y + 3, 190, 8);
            scr.round_rect(bar, 4, if th.dark { 0x3A3B41 } else { 0xE2E2E8 });
            scr.round_rect(Rect::new(bar.x, bar.y, (bar.w * l / 100).max(8), 8), 4, if l > 80 { 0xFF453A } else { th.accent });
            lb.clear().u(l as u64).s("%");
            scr.text_right(&FONT_SMALL, t.right() - 12, y - 1, lb.get(), th.text2);
        }
        hits.push(t, Action::Launch(APP_MONITOR));
        // papel de parede
        let ty = t.bottom() + 10 - r.y;
        let t = tile(12, ty, 310, 92);
        scr.text(&FONT_UI_BOLD, t.x + 14, t.y + 8, b"Papel de parede", th.text);
        for k in 0..6 {
            let tr = Rect::new(t.x + 14 + k * 48, t.y + 36, 42, 42);
            let src = Fb::new(thumbs().as_ptr().wrapping_add(k as usize * (THUMB_W * THUMB_H) as usize) as *mut u32,
                THUMB_W, THUMB_H, THUMB_W);
            scr.blit_rounded(&src, (THUMB_W - 42) / 2, (THUMB_H - 42) / 2, 42, 42, tr.x, tr.y, 8, true, true);
            if self.cfg.wallpaper == k as u8 { scr.round_border(tr.inset(-2), 6, th.accent, 255); }
            hits.push(tr, Action::SetWallpaper(k as u8));
        }
        // destaque
        let ty = t.bottom() + 10 - r.y;
        let t = tile(12, ty, 310, 56);
        scr.text(&FONT_UI_BOLD, t.x + 14, t.y + 8, b"Destaque", th.text);
        for (k, (col, _)) in theme::ACCENTS.iter().enumerate() {
            let sr = ui::swatch(scr, t.x + 120 + k as i32 * 24, t.y + 28, *col, self.cfg.accent == k as u8, &th);
            hits.push(sr, Action::SetAccent(k as u8));
        }
        // botao
        let b = Rect::new(r.x + 12, t.bottom() + 10, 310, 30);
        ui::button(scr, b, "Configurações do Sistema...", &th, false, b.contains(self.mx, self.my));
        hits.push(b, Action::Settings(0));
        self.cc_hits = hits;
    }

    fn draw_toasts(&mut self, scr: &Fb) {
        let th = self.th;
        let mut y = MENU_H + 10;
        for t in self.toasts.iter() {
            if !t.active { continue; }
            let r = Rect::new(self.w - 350, y, 336, 68);
            scr.shadow(r, 14, 18, 6, if th.dark { 130 } else { 70 });
            ui::glass(scr, &self.blur, r, 14, &th, self.cfg.glass);
            if self.cfg.glass { scr.round_rect_a(r, 14, th.menu_bg, 120); }
            icons::draw(scr, t.icon, r.x + 12, r.y + 14, 40);
            scr.text_clip(&FONT_UI_BOLD, r.x + 64, r.y + 12, r.w - 76, &t.title[..t.tl], th.text);
            scr.text_clip(&FONT_SMALL, r.x + 64, r.y + 36, r.w - 76, &t.body[..t.bl], th.text2);
            y += 78;
        }
    }

    fn draw_spotlight(&mut self, scr: &Fb) {
        let th = self.th;
        let r = self.spot_rect();
        scr.shadow(r, 16, 34, 14, if th.dark { 170 } else { 100 });
        scr.round_rect_a(r, 16, th.menu_bg, 250);
        scr.round_border(r, 16, if th.dark { 0xFFFFFF } else { 0x000000 }, 34);
        let f = Rect::new(r.x + 14, r.y + 10, r.w - 28, 36);
        icons::sym(scr, Sym::Search, f.x + 4, f.y + 8, 20, th.text2);
        let tx = f.x + 36;
        let ty = f.y + (36 - FONT_TITLE.height) / 2;
        if self.query.n == 0 {
            scr.text(&FONT_TITLE, tx, ty, "Buscar apps, arquivos e comandos".as_bytes(), th.text3);
            if self.caret_on { scr.fill(Rect::new(tx - 2, f.y + 6, 2, 24), th.accent); }
        } else {
            let w = scr.text(&FONT_TITLE, tx, ty, self.query.get(), th.text);
            if self.caret_on { scr.fill(Rect::new(tx + w + 1, f.y + 6, 2, 24), th.accent); }
        }
        if self.query.n == 0 { return; }
        scr.hline(r.x, r.y + 54, r.w, th.sep, 255);
        if self.spot_n == 0 {
            scr.text(&FONT_UI, r.x + 24, r.y + 70, "Nenhum resultado".as_bytes(), th.text2);
            return;
        }
        for k in 0..self.spot_n {
            let row = Rect::new(r.x + 8, r.y + 60 + k as i32 * 40, r.w - 16, 38);
            let sel = k == self.spot_sel;
            if sel { scr.round_rect(row, 8, th.accent); }
            let tc = if sel { 0xFFFFFF } else { th.text };
            let tc2 = if sel { 0xE6EEFF } else { th.text2 };
            let (icon, name, kind): (Icon, Buf<40>, &str) = match self.spot_hits[k] {
                Hit::App(id) => { let mut b = Buf::new(); b.s(catalog::app(id).name); (catalog::app(id).icon, b, "Aplicativo") }
                Hit::File(f) => { let fi = self.files[f as usize]; let mut b = Buf::new(); b.bytes(fi.name()); (panels::file_icon(&fi), b, panels::file_kind_label(&fi)) }
                Hit::Cmd(_, s, i) => { let mut b = Buf::new(); b.s(s); (i, b, "Comando") }
                Hit::Calc(v) => { let mut b = Buf::new(); b.s("= ").i(v); (Icon::Calc, b, "Calculadora") }
            };
            icons::draw(scr, icon, row.x + 8, row.y + 5, 28);
            scr.text(&FONT_UI, row.x + 46, row.y + 9, name.get(), tc);
            scr.text_right(&FONT_SMALL, row.right() - 12, row.y + 11, kind.as_bytes(), tc2);
        }
    }

    fn draw_switcher(&mut self, scr: &Fb) {
        let th = self.th;
        let r = self.switcher_rect();
        scr.shadow(r, 20, 30, 10, if th.dark { 160 } else { 90 });
        scr.round_rect_a(r, 20, th.menu_bg, 252);
        scr.round_border(r, 20, if th.dark { 0xFFFFFF } else { 0x000000 }, 30);
        for k in 0..self.sw_n {
            let w = self.wins[self.sw_list[k]];
            let cell = Rect::new(r.x + 16 + k as i32 * 104, r.y + 16, 96, 96);
            if k == self.sw_sel { scr.round_rect_a(cell, 14, th.text, 40); }
            icons::draw(scr, catalog::app(w.app).icon, cell.x + 14, cell.y + 12, 68);
            if w.minimized { scr.disc(cell.x + 48, cell.bottom() - 6, 2, th.text2, 255); }
        }
        if self.sw_sel < self.sw_n {
            let w = self.wins[self.sw_list[self.sw_sel]];
            scr.text_center(&FONT_UI_BOLD, Rect::new(r.x, r.bottom() - 34, r.w, 24), w.title(), th.text);
        }
    }

    fn draw_dialog(&mut self, scr: &Fb) {
        let th = self.th;
        scr.fill_a(Rect::new(0, 0, self.w, self.h), 0x000000, 70);
        let r = Rect::new((self.w - 420) / 2, (self.h - 190) / 2, 420, 190);
        scr.shadow(r, 14, 40, 16, 150);
        scr.round_rect(r, 14, th.menu_bg);
        scr.round_border(r, 14, if th.dark { 0xFFFFFF } else { 0x000000 }, 34);
        icons::draw(scr, Icon::Pulsar, r.x + 24, r.y + 26, 56);
        scr.text(&FONT_UI_BOLD, r.x + 98, r.y + 30, "Deseja desligar o computador agora?".as_bytes(), th.text);
        scr.text(&FONT_UI, r.x + 98, r.y + 56, "Os aplicativos abertos serão fechados".as_bytes(), th.text2);
        scr.text(&FONT_UI, r.x + 98, r.y + 76, "e o QEMU será encerrado.".as_bytes(), th.text2);
        let mut hits = Hits::new();
        let b1 = Rect::new(r.right() - 250, r.bottom() - 50, 110, 30);
        let b2 = Rect::new(r.right() - 130, r.bottom() - 50, 110, 30);
        ui::button(scr, b1, "Cancelar", &th, false, b1.contains(self.mx, self.my));
        ui::button(scr, b2, "Desligar", &th, true, b2.contains(self.mx, self.my));
        hits.push(b1, Action::DialogCancel);
        hits.push(b2, Action::DialogConfirm);
        self.dialog_hits = hits;
    }
}

/// Menu com o nome do app (segundo item da barra).
static APP_MENU_PROC: [MI; 4] = [
    MI("Sobre o Pulsar OS", Action::About, ""),
    SEP,
    MI("Encerrar", Action::QuitApp, "Ctrl+Q"),
    MI("Forçar encerramento", Action::ForceQuit, ""),
];
static APP_MENU_INTERNAL: [MI; 3] = [
    MI("Sobre o Pulsar OS", Action::About, ""),
    SEP,
    MI("Fechar", Action::CloseWin, "Ctrl+W"),
];

/// Busca sem diferenciar maiusculas (ASCII) nem acentos comuns.
fn contains_ci(hay: &[u8], needle: &[u8]) -> bool {
    let fold = |s: &[u8], out: &mut [u8; 96]| -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < s.len() && n < 96 {
            let cp = text::next_char(s, &mut i);
            let c = match cp {
                0xE0..=0xE5 | 0xC0..=0xC5 => b'a',
                0xE8..=0xEB | 0xC8..=0xCB => b'e',
                0xEC..=0xEF | 0xCC..=0xCF => b'i',
                0xF2..=0xF6 | 0xD2..=0xD6 => b'o',
                0xF9..=0xFC | 0xD9..=0xDC => b'u',
                0xE7 | 0xC7 => b'c',
                c if c < 128 => (c as u8).to_ascii_lowercase(),
                _ => b'?',
            };
            out[n] = c;
            n += 1;
        }
        n
    };
    let (mut h, mut q) = ([0u8; 96], [0u8; 96]);
    let hn = fold(hay, &mut h);
    let qn = fold(needle, &mut q);
    if qn == 0 { return false; }
    (0..=hn.saturating_sub(qn)).any(|p| hn >= qn && h[p..p + qn] == q[..qn])
}

/// Avalia expressoes inteiras simples (+ - * / e parenteses) da busca.
fn eval(s: &[u8]) -> Option<i64> {
    if !s.iter().any(|c| b"+-*/x".contains(c)) || !s.iter().any(|c| c.is_ascii_digit()) { return None; }
    if !s.iter().all(|c| c.is_ascii_digit() || b"+-*/x() ".contains(c)) { return None; }
    let mut p = 0;
    let v = expr(s, &mut p)?;
    skip_ws(s, &mut p);
    if p == s.len() { Some(v) } else { None }
}
fn skip_ws(s: &[u8], p: &mut usize) { while *p < s.len() && s[*p] == b' ' { *p += 1; } }
fn expr(s: &[u8], p: &mut usize) -> Option<i64> {
    let mut v = term(s, p)?;
    loop {
        skip_ws(s, p);
        match s.get(*p) {
            Some(b'+') => { *p += 1; v = v.checked_add(term(s, p)?)?; }
            Some(b'-') => { *p += 1; v = v.checked_sub(term(s, p)?)?; }
            _ => return Some(v),
        }
    }
}
fn term(s: &[u8], p: &mut usize) -> Option<i64> {
    let mut v = atom(s, p)?;
    loop {
        skip_ws(s, p);
        match s.get(*p) {
            Some(b'*') | Some(b'x') => { *p += 1; v = v.checked_mul(atom(s, p)?)?; }
            Some(b'/') => { *p += 1; let d = atom(s, p)?; if d == 0 { return None; } v /= d; }
            _ => return Some(v),
        }
    }
}
fn atom(s: &[u8], p: &mut usize) -> Option<i64> {
    skip_ws(s, p);
    match s.get(*p)? {
        b'(' => { *p += 1; let v = expr(s, p)?; skip_ws(s, p); if s.get(*p) == Some(&b')') { *p += 1; Some(v) } else { None } }
        b'-' => { *p += 1; Some(-atom(s, p)?) }
        c if c.is_ascii_digit() => {
            let mut v: i64 = 0;
            while let Some(c) = s.get(*p).filter(|c| c.is_ascii_digit()) {
                v = v.checked_mul(10)?.checked_add((c - b'0') as i64)?;
                *p += 1;
            }
            Some(v)
        }
        _ => None,
    }
}
