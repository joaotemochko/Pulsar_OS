//! Janelas internas do WM: Arquivos (Finder), Configuracoes, Sobre e
//! Atalhos. Sao desenhadas direto no back buffer; cada desenho registra
//! as areas clicaveis em `Hits`.
use crate::catalog::*;
use crate::config::{Config, DOCK_SIZES, WALLPAPERS};
use crate::ui::{self, Hits};
use plib::icons::{self, Icon, Sym};
use plib::*;

pub const TOOLBAR_H: i32 = 52;
pub const SIDEBAR_W: i32 = 214;
const ROW_H: i32 = 28;
pub const THUMB_W: i32 = 120;
pub const THUMB_H: i32 = 75;

/// Estado das janelas internas.
pub struct Panels {
    pub files_sel: i32,
    pub files_view: u8,
    pub files_filter: u8,
    pub files_scroll: i32,
    pub settings_page: u8,
}

impl Panels {
    pub const fn new() -> Panels {
        Panels { files_sel: -1, files_view: 0, files_filter: 0, files_scroll: 0, settings_page: 0 }
    }
}

/// Tudo que os paineis precisam para se desenhar.
pub struct Ctx<'a> {
    pub scr: &'a Fb,
    pub th: &'a Theme,
    pub cfg: &'a Config,
    pub focused: bool,
    pub mx: i32,
    pub my: i32,
    pub files: &'a [FileInfo],
    pub thumbs: &'a [u32],
    pub sys: &'a SysInfo,
    pub disk_free_kb: u32,
}

/// Icone de um arquivo segundo nome/extensao.
pub fn file_icon(f: &FileInfo) -> Icon {
    if let Some(a) = app_by_bin(f.name()) { return a.icon; }
    match f.ext() {
        b"pulse" => Icon::Program,
        b"cfg" => Icon::Config,
        _ => Icon::Document,
    }
}

pub fn file_kind_label(f: &FileInfo) -> &'static str {
    if app_by_bin(f.name()).is_some() { return "Aplicativo"; }
    match f.ext() {
        b"pulse" => "Programa",
        b"txt" => "Documento de texto",
        b"md" => "Documento Markdown",
        b"cfg" => "Configuração",
        _ => "Arquivo",
    }
}

/// Arquivos visiveis segundo o filtro (0 todos, 1 apps, 2 documentos).
pub fn visible_files(files: &[FileInfo], filter: u8, out: &mut [u16; 64]) -> usize {
    let mut n = 0;
    for (i, f) in files.iter().enumerate() {
        let is_app = f.ext() == b"pulse";
        let ok = match filter { 1 => is_app, 2 => !is_app, _ => true };
        if ok && n < 64 { out[n] = i as u16; n += 1; }
    }
    n
}

fn sidebar(c: &Ctx, r: Rect) {
    let sb = Rect::new(r.x, r.y, SIDEBAR_W + 12, r.h);
    c.scr.round_rect(sb, 10, c.th.side_bg);
    c.scr.vline(r.x + SIDEBAR_W, r.y, r.h, c.th.sep, 255);
}

fn sidebar_item(c: &Ctx, r: Rect, y: i32, sym: Sym, sym_bg: u32, label: &str, sel: bool, hits: &mut Hits, a: Action) {
    let ir = Rect::new(r.x + 10, y, SIDEBAR_W - 20, 30);
    if sel {
        c.scr.round_rect(ir, 6, if c.focused { c.th.accent } else { c.th.hover });
    } else if ir.contains(c.mx, c.my) {
        c.scr.round_rect_a(ir, 6, c.th.hover, 160);
    }
    let tc = if sel && c.focused { 0xFFFFFF } else { c.th.text };
    if sym_bg != 0 {
        c.scr.round_rect(Rect::new(ir.x + 6, ir.y + 5, 20, 20), 5, sym_bg);
        icons::sym(c.scr, sym, ir.x + 9, ir.y + 8, 14, 0xFFFFFF);
    } else {
        icons::sym(c.scr, sym, ir.x + 8, ir.y + 7, 16, if sel && c.focused { 0xFFFFFF } else { c.th.accent });
    }
    c.scr.text(&FONT_UI, ir.x + 34, ir.y + 5, label.as_bytes(), tc);
    hits.push(ir, a);
}

fn section(c: &Ctx, x: i32, y: i32, label: &str) {
    c.scr.text(&FONT_SMALL, x, y, label.as_bytes(), c.th.text3);
}

// ------------------------------------------------------------- Arquivos

pub fn draw_files(c: &Ctx, r: Rect, st: &mut Panels, hits: &mut Hits) {
    let th = c.th;
    c.scr.round_rect(r, 10, th.win_bg);
    sidebar(c, r);
    section(c, r.x + 18, r.y + 52, "Favoritos");
    let items = [(Sym::Folder, "Todos os arquivos"), (Sym::Apps, "Aplicativos"), (Sym::Doc, "Documentos")];
    for (k, (s, l)) in items.iter().enumerate() {
        sidebar_item(c, r, r.y + 70 + k as i32 * 32, *s, 0, l, st.files_filter == k as u8, hits,
            Action::FilesFilter(k as u8));
    }
    section(c, r.x + 18, r.y + 178, "Locais");
    sidebar_item(c, r, r.y + 196, Sym::Disk, 0, "Nebular (disco)", false, hits, Action::FilesFilter(0));

    // barra de ferramentas
    let cx = r.x + SIDEBAR_W;
    let cw = r.w - SIDEBAR_W;
    c.scr.fill(Rect::new(cx, r.y + TOOLBAR_H - 1, cw, 1), th.sep);
    let title = ["Todos os arquivos", "Aplicativos", "Documentos"][st.files_filter as usize % 3];
    c.scr.text(&FONT_UI_BOLD, cx + 16, r.y + 9, title.as_bytes(), th.text);
    let mut vis = [0u16; 64];
    let n = visible_files(c.files, st.files_filter, &mut vis);
    let mut b = Buf::<48>::new();
    b.u(n as u64).s(if n == 1 { " item" } else { " itens" });
    c.scr.text(&FONT_SMALL, cx + 16, r.y + 29, b.get(), th.text2);
    // alternar visualizacao
    let seg = Rect::new(r.right() - 92, r.y + 12, 76, 28);
    c.scr.round_rect(seg, 7, if th.dark { 0x2C2D33 } else { 0xE9E9EE });
    for k in 0..2 {
        let sr = Rect::new(seg.x + k * 38, seg.y, 38, 28);
        if st.files_view == k as u8 {
            c.scr.round_rect(sr.inset(2), 6, if th.dark { 0x5A5B62 } else { 0xFFFFFF });
        }
        icons::sym(c.scr, if k == 0 { Sym::List } else { Sym::Grid }, sr.x + 12, sr.y + 7, 14, th.text2);
        hits.push(sr, Action::FilesView(k as u8));
    }

    // conteudo
    let body = Rect::new(cx, r.y + TOOLBAR_H, cw, r.h - TOOLBAR_H - 26);
    let mut bc = *c.scr;
    bc.set_clip(c.scr.clip.intersect(&body));
    if st.files_view == 0 {
        // cabecalho de colunas
        let hy = body.y;
        bc.text(&FONT_SMALL, cx + 44, hy + 6, b"Nome", th.text2);
        bc.text(&FONT_SMALL, cx + cw - 230, hy + 6, b"Tamanho", th.text2);
        bc.text(&FONT_SMALL, cx + cw - 140, hy + 6, b"Tipo", th.text2);
        bc.fill(Rect::new(cx, hy + 26, cw, 1), th.sep);
        let rows_top = hy + 28;
        let max_scroll = (n as i32 * ROW_H - (body.bottom() - rows_top)).max(0);
        st.files_scroll = st.files_scroll.clamp(0, max_scroll);
        let mut rc = bc;
        rc.set_clip(bc.clip.intersect(&Rect::new(cx, rows_top, cw, body.bottom() - rows_top)));
        for k in 0..n {
            let f = &c.files[vis[k] as usize];
            let y = rows_top + k as i32 * ROW_H - st.files_scroll;
            if y + ROW_H < rows_top || y > body.bottom() { continue; }
            let row = Rect::new(cx + 8, y, cw - 16, ROW_H);
            let sel = st.files_sel == f.idx as i32;
            if sel {
                rc.round_rect(row, 6, if c.focused { th.accent } else { th.hover });
            } else if k % 2 == 1 {
                rc.round_rect_a(row, 6, if th.dark { 0xFFFFFF } else { 0x000000 }, 8);
            }
            let tc = if sel && c.focused { 0xFFFFFF } else { th.text };
            let tc2 = if sel && c.focused { 0xEAF2FF } else { th.text2 };
            icons::draw(&rc, file_icon(f), cx + 16, y + 4, 20);
            rc.text_clip(&FONT_UI, cx + 44, y + 5, cw - 300, f.name(), tc);
            let mut sz = Buf::<24>::new();
            sz.size(f.size as u64);
            rc.text(&FONT_UI, cx + cw - 230, y + 5, sz.get(), tc2);
            rc.text_clip(&FONT_UI, cx + cw - 140, y + 5, 130, file_kind_label(f).as_bytes(), tc2);
            hits.push(row.intersect(&Rect::new(cx, rows_top, cw, body.bottom() - rows_top)),
                Action::FileSel(f.idx as u16));
        }
    } else {
        let cols = ((cw - 16) / 104).max(1);
        let rows = (n as i32 + cols - 1) / cols;
        let max_scroll = (rows * 104 - body.h + 8).max(0);
        st.files_scroll = st.files_scroll.clamp(0, max_scroll);
        for k in 0..n {
            let f = &c.files[vis[k] as usize];
            let (col, row) = (k as i32 % cols, k as i32 / cols);
            let cell = Rect::new(cx + 12 + col * 104, body.y + 10 + row * 104 - st.files_scroll, 96, 98);
            let sel = st.files_sel == f.idx as i32;
            if sel {
                bc.round_rect_a(Rect::new(cell.x + 16, cell.y, 64, 64), 10, th.text3, 90);
            }
            icons::draw(&bc, file_icon(f), cell.x + 22, cell.y + 6, 52);
            let lab = Rect::new(cell.x, cell.y + 66, 96, 22);
            let tw = FONT_SMALL.width(f.name()).min(92);
            let pill = Rect::new(cell.x + 48 - tw / 2 - 5, lab.y, tw + 10, 20);
            if sel { bc.round_rect(pill, 5, th.accent); }
            let mut tc = bc;
            tc.set_clip(bc.clip.intersect(&pill.inset(1)));
            tc.text(&FONT_SMALL, pill.x + 5, lab.y + 2, f.name(), if sel { 0xFFFFFF } else { th.text });
            hits.push(cell.intersect(&body), Action::FileSel(f.idx as u16));
        }
    }

    // barra de status
    let sb = Rect::new(cx, r.bottom() - 26, cw, 26);
    c.scr.fill(Rect::new(cx, sb.y, cw, 1), th.sep);
    let mut s = Buf::<64>::new();
    s.u(n as u64).s(" itens  ·  ").size(c.disk_free_kb as u64 * 1024).s(" disponíveis");
    c.scr.text_center(&FONT_SMALL, sb, s.get(), th.text2);
}

// --------------------------------------------------------- Configuracoes

const PAGES: [(&str, Sym, u32); 5] = [
    ("Aparência", Sym::Sun, 0x0A84FF),
    ("Papel de Parede", Sym::Image, 0x30B0C7),
    ("Dock e Barra de Menus", Sym::Dock, 0x636366),
    ("Teclado", Sym::Keyboard, 0x8E8E93),
    ("Sobre", Sym::Info, 0x8E8E93),
];

fn card(c: &Ctx, r: Rect) {
    c.scr.round_rect(r, 10, if c.th.dark { 0x26272D } else { 0xFFFFFF });
    c.scr.round_border(r, 10, if c.th.dark { 0xFFFFFF } else { 0x000000 }, if c.th.dark { 18 } else { 22 });
}

fn row_label(c: &Ctx, r: Rect, y: i32, label: &str, sub: &str) {
    c.scr.text(&FONT_UI, r.x + 16, y + if sub.is_empty() { 0 } else { -8 }, label.as_bytes(), c.th.text);
    if !sub.is_empty() {
        c.scr.text(&FONT_SMALL, r.x + 16, y + 11, sub.as_bytes(), c.th.text2);
    }
}

fn row_sep(c: &Ctx, r: Rect, y: i32) {
    c.scr.hline(r.x + 16, y, r.w - 32, c.th.sep, 255);
}

pub fn draw_settings(c: &Ctx, r: Rect, st: &Panels, hits: &mut Hits) {
    let th = c.th;
    let cfg = c.cfg;
    c.scr.round_rect(r, 10, if th.dark { 0x1C1D21 } else { 0xF5F5F7 });
    sidebar(c, r);
    for (k, (l, s, col)) in PAGES.iter().enumerate() {
        sidebar_item(c, r, r.y + 52 + k as i32 * 34, *s, *col, l, st.settings_page == k as u8, hits,
            Action::SettingsPage(k as u8));
    }
    let x = r.x + SIDEBAR_W + 24;
    let w = r.w - SIDEBAR_W - 48;
    let page = PAGES[st.settings_page as usize % 5].0;
    c.scr.text(&FONT_TITLE, x, r.y + 14, page.as_bytes(), th.text);
    let mut y = r.y + 62;
    match st.settings_page {
        0 => {
            let cr = Rect::new(x, y, w, 150);
            card(c, cr);
            c.scr.text(&FONT_UI, cr.x + 16, cr.y + 12, "Aparência".as_bytes(), th.text);
            for k in 0..2 {
                let dark = k == 1;
                let tile = Rect::new(cr.x + 16 + k * 150, cr.y + 40, 132, 80);
                let pv = Theme::new(dark, th.accent);
                c.scr.round_rect(tile, 8, if dark { 0x2A2F3A } else { 0xC9D8F0 });
                let win = Rect::new(tile.x + 18, tile.y + 14, 96, 56);
                c.scr.round_rect(win, 6, pv.win_bg);
                c.scr.round_rect_grad(Rect::new(win.x, win.y, win.w, 16), 6, pv.title_top, pv.title_bot, 255);
                for (j, col) in [0xFF5F57u32, 0xFEBC2E, 0x28C840].iter().enumerate() {
                    c.scr.disc(win.x + 8 + j as i32 * 9, win.y + 8, 3, *col, 255);
                }
                c.scr.round_rect(Rect::new(win.x + 8, win.y + 24, 50, 6), 3, pv.accent);
                c.scr.round_rect(Rect::new(win.x + 8, win.y + 36, 70, 6), 3, pv.sep);
                if cfg.dark == dark {
                    c.scr.round_border(tile.inset(-3), 10, th.accent, 255);
                    c.scr.round_border(tile.inset(-2), 9, th.accent, 255);
                }
                c.scr.text_center(&FONT_SMALL, Rect::new(tile.x, tile.bottom() + 4, tile.w, 16),
                    if dark { b"Escuro" } else { b"Claro" }, th.text);
                hits.push(tile.inset(-4), if cfg.dark != dark { Action::ToggleDark } else { Action::None });
            }
            y += 166;
            let cr = Rect::new(x, y, w, 112);
            card(c, cr);
            row_label(c, cr, cr.y + 18, "Cor de destaque", "");
            for (k, (col, _)) in theme::ACCENTS.iter().enumerate() {
                let sr = ui::swatch(c.scr, cr.right() - 30 - (7 - k as i32) * 30, cr.y + 26, *col,
                    cfg.accent == k as u8, th);
                hits.push(sr, Action::SetAccent(k as u8));
            }
            row_sep(c, cr, cr.y + 56);
            row_label(c, cr, cr.y + 76, "Efeito vidro", "Barra de menus e dock translúcidos");
            let t = ui::toggle(c.scr, cr.right() - 56, cr.y + 72, cfg.glass, th);
            hits.push(t, Action::SetGlass(!cfg.glass));
        }
        1 => {
            let cr = Rect::new(x, y, w, 120);
            card(c, cr);
            let cur = cfg.wallpaper as usize % 6;
            let big = Rect::new(cr.x + 16, cr.y + 16, 140, 88);
            blit_thumb(c, cur, big);
            c.scr.text(&FONT_UI_BOLD, big.right() + 20, cr.y + 30, WALLPAPERS[cur].as_bytes(), th.text);
            c.scr.text(&FONT_SMALL, big.right() + 20, cr.y + 52, "Papel de parede procedural,".as_bytes(), th.text2);
            c.scr.text(&FONT_SMALL, big.right() + 20, cr.y + 68, "gerado na resolução da tela.".as_bytes(), th.text2);
            y += 136;
            let cr = Rect::new(x, y, w, 2 * (THUMB_H + 36) + 16);
            card(c, cr);
            let per_row = 3;
            let gap = (cr.w - 32 - per_row * THUMB_W) / (per_row - 1);
            for k in 0..6 {
                let (col, row) = (k % per_row, k / per_row);
                let t = Rect::new(cr.x + 16 + col * (THUMB_W + gap), cr.y + 16 + row * (THUMB_H + 36), THUMB_W, THUMB_H);
                blit_thumb(c, k as usize, t);
                if cur == k as usize {
                    c.scr.round_border(t.inset(-3), 9, th.accent, 255);
                    c.scr.round_border(t.inset(-2), 8, th.accent, 255);
                }
                c.scr.text_center(&FONT_SMALL, Rect::new(t.x, t.bottom() + 4, t.w, 18), WALLPAPERS[k as usize].as_bytes(), th.text);
                hits.push(t.inset(-3), Action::SetWallpaper(k as u8));
            }
        }
        2 => {
            let cr = Rect::new(x, y, w, 4 * 50 + 4);
            card(c, cr);
            row_label(c, cr, cr.y + 18, "Tamanho", "");
            ui::segmented(c.scr, Rect::new(cr.right() - 256, cr.y + 11, 240, 28), &DOCK_SIZES,
                cfg.dock_size as usize, th, hits, &[Action::DockSize(0), Action::DockSize(1), Action::DockSize(2)]);
            row_sep(c, cr, cr.y + 50);
            row_label(c, cr, cr.y + 68, "Ampliação", "");
            let t = ui::toggle(c.scr, cr.right() - 56, cr.y + 63, cfg.dock_magnify, th);
            hits.push(t, Action::ToggleMagnify);
            row_sep(c, cr, cr.y + 100);
            row_label(c, cr, cr.y + 118, "Posição na tela", "");
            ui::segmented(c.scr, Rect::new(cr.right() - 196, cr.y + 111, 180, 28), &["Embaixo", "Esquerda"],
                cfg.dock_left as usize, th, hits, &[Action::DockLeft(false), Action::DockLeft(true)]);
            row_sep(c, cr, cr.y + 150);
            row_label(c, cr, cr.y + 168, "Clique no app em foco minimiza", "");
            let t = ui::toggle(c.scr, cr.right() - 56, cr.y + 163, cfg.dock_minimize, th);
            hits.push(t, Action::SetDockMinimize(!cfg.dock_minimize));
            y += cr.h + 16;
            let cr = Rect::new(x, y, w, 3 * 50 + 4);
            card(c, cr);
            let rows = [("Relógio de 24 horas", cfg.clock24, Action::SetClock24(!cfg.clock24)),
                ("Mostrar segundos", cfg.seconds, Action::SetSeconds(!cfg.seconds)),
                ("Uso das CPUs na barra de menus", cfg.cpu_meter, Action::SetCpuMeter(!cfg.cpu_meter))];
            for (k, (l, on, a)) in rows.iter().enumerate() {
                let ry = cr.y + 18 + k as i32 * 50;
                if k > 0 { row_sep(c, cr, ry - 18); }
                row_label(c, cr, ry, l, "");
                let t = ui::toggle(c.scr, cr.right() - 56, ry - 5, *on, th);
                hits.push(t, *a);
            }
        }
        3 => {
            let cr = Rect::new(x, y, w, 60);
            card(c, cr);
            row_label(c, cr, cr.y + 30, "Layout do teclado", "");
            ui::segmented(c.scr, Rect::new(cr.right() - 236, cr.y + 16, 220, 28), &["ABNT2 (Brasil)", "US"],
                if cfg.abnt2 { 0 } else { 1 }, th, hits, &[Action::SetLayout(true), Action::SetLayout(false)]);
            y += 76;
            let cr = Rect::new(x, y, w, 186);
            card(c, cr);
            let tips = [("Buscar apps e arquivos", "Ctrl+Espaço"), ("Alternar janelas", "Alt+Tab"),
                ("Fechar janela", "Ctrl+W / Alt+F4"), ("Encaixar janela", "Super+Esq./Dir."),
                ("Gerenciador de tarefas", "Ctrl+Shift+Esc")];
            for (k, (l, s)) in tips.iter().enumerate() {
                let ry = cr.y + 14 + k as i32 * 28;
                c.scr.text(&FONT_UI, cr.x + 16, ry, l.as_bytes(), th.text);
                c.scr.text_right(&FONT_UI, cr.right() - 16, ry, s.as_bytes(), th.text2);
            }
            let b = Rect::new(cr.x + 16, cr.bottom() - 40, 200, 28);
            ui::button(c.scr, b, "Ver todos os atalhos", th, false, b.contains(c.mx, c.my));
            hits.push(b, Action::Help);
        }
        _ => {
            let cr = Rect::new(x, y, w, 300);
            card(c, cr);
            icons::draw(c.scr, Icon::Pulsar, cr.x + 20, cr.y + 20, 72);
            c.scr.text(&FONT_TITLE, cr.x + 110, cr.y + 26, b"Pulsar OS", th.text);
            c.scr.text(&FONT_UI, cr.x + 110, cr.y + 58, b"Vers\xc3\xa3o 0.2 \xc2\xb7 Nebulosa", th.text2);
            sys_rows(c, cr.x + 20, cr.y + 112, cr.w - 40);
            let b1 = Rect::new(cr.x + 20, cr.bottom() - 44, 180, 28);
            ui::button(c.scr, b1, "Editar pulsar.cfg", th, false, b1.contains(c.mx, c.my));
            hits.push(b1, Action::EditConfig);
            let b2 = Rect::new(b1.right() + 12, b1.y, 190, 28);
            ui::button(c.scr, b2, "Monitor do Sistema", th, false, b2.contains(c.mx, c.my));
            hits.push(b2, Action::Launch(APP_MONITOR));
        }
    }
}

fn blit_thumb(c: &Ctx, k: usize, r: Rect) {
    let src = Fb::new(c.thumbs.as_ptr().wrapping_add(k * (THUMB_W * THUMB_H) as usize) as *mut u32,
        THUMB_W, THUMB_H, THUMB_W);
    // miniatura (escala inteira mais proxima) com cantos arredondados
    if r.w == THUMB_W && r.h == THUMB_H {
        c.scr.blit_rounded(&src, 0, 0, r.w, r.h, r.x, r.y, 8, true, true);
    } else {
        let cr = r.intersect(&c.scr.clip);
        for y in cr.y..cr.bottom() {
            for x in cr.x..cr.right() {
                let p = src.get((x - r.x) * THUMB_W / r.w, (y - r.y) * THUMB_H / r.h);
                c.scr.pixel(x, y, p);
            }
        }
        c.scr.round_border(r, 8, 0x000000, 30);
    }
}

fn sys_rows(c: &Ctx, x: i32, y: i32, w: i32) {
    let s = c.sys;
    let mut lines: [(&str, Buf<64>); 5] = [
        ("Processador", Buf::new()), ("Memória", Buf::new()), ("Gráficos", Buf::new()),
        ("Kernel", Buf::new()), ("Ativo há", Buf::new()),
    ];
    lines[0].1.u(s.ncpus as u64).s(if s.ncpus == 1 { " núcleo " } else { " núcleos " }).bytes(s.arch());
    lines[1].1.size(s.mem_total_kb as u64 * 1024).s(" (").size(s.mem_free_kb as u64 * 1024).s(" livres)");
    lines[2].1.s("virtio-gpu ").u(s.fb_w as u64).s(" x ").u(s.fb_h as u64);
    lines[3].1.s("Microkernel Pulsar SMP");
    let up = s.uptime_ms / 1000;
    lines[4].1.u(up / 3600).s(" h ").u0((up / 60) % 60, 2).s(" min");
    for (k, (l, v)) in lines.iter().enumerate() {
        let ry = y + k as i32 * 24;
        c.scr.text(&FONT_UI, x, ry, l.as_bytes(), c.th.text2);
        c.scr.text_clip(&FONT_UI, x + 130, ry, w - 130, v.get(), c.th.text);
    }
}

// ------------------------------------------------------------------ Sobre

pub fn draw_about(c: &Ctx, r: Rect, hits: &mut Hits) {
    let th = c.th;
    c.scr.round_rect(r, 10, th.win_bg);
    icons::draw(c.scr, Icon::Pulsar, r.x + 40, r.y + 70, 120);
    let x = r.x + 190;
    c.scr.text(&FONT_TITLE, x, r.y + 48, b"Pulsar OS", th.text);
    c.scr.text(&FONT_UI, x, r.y + 80, b"Vers\xc3\xa3o 0.2 \xc2\xb7 Nebulosa", th.text2);
    sys_rows(c, x, r.y + 112, r.w - 210);
    let b1 = Rect::new(x, r.bottom() - 46, 160, 28);
    ui::button(c.scr, b1, "Monitor do Sistema", th, false, b1.contains(c.mx, c.my));
    hits.push(b1, Action::Launch(APP_MONITOR));
    let b2 = Rect::new(b1.right() + 10, b1.y, 140, 28);
    ui::button(c.scr, b2, "Configurações", th, true, b2.contains(c.mx, c.my));
    hits.push(b2, Action::Settings(4));
}

// ---------------------------------------------------------------- Atalhos

const SHORTCUTS: [(&str, &str); 16] = [
    ("Ctrl+Espaço", "Buscar apps, arquivos e comandos"),
    ("Super (sozinha)", "Buscar"),
    ("Alt+Tab", "Alternar entre janelas"),
    ("Ctrl+W / Alt+F4", "Fechar a janela ativa"),
    ("Ctrl+Q", "Encerrar o aplicativo"),
    ("Super+Cima / F11", "Maximizar / restaurar"),
    ("Super+Baixo", "Minimizar"),
    ("Super+Esq. / Dir.", "Encaixar na metade da tela"),
    ("Super+D", "Mostrar a área de trabalho"),
    ("Super+E", "Abrir Arquivos"),
    ("Super+I", "Abrir Configurações"),
    ("Ctrl+Alt+T", "Abrir o Terminal"),
    ("Ctrl+Shift+Esc", "Monitor do Sistema (tarefas)"),
    ("F1", "Esta lista de atalhos"),
    ("Clique direito", "Menus de contexto"),
    ("Duplo clique no título", "Maximizar / restaurar"),
];

pub fn draw_help(c: &Ctx, r: Rect) {
    let th = c.th;
    c.scr.round_rect(r, 10, th.win_bg);
    c.scr.text(&FONT_UI_BOLD, r.x + 24, r.y + 50, b"Atalhos do teclado e do mouse", th.text);
    for (k, (key, what)) in SHORTCUTS.iter().enumerate() {
        let y = r.y + 82 + k as i32 * 21;
        let kw = FONT_SMALL.width(key.as_bytes()) + 14;
        let kr = Rect::new(r.x + 24, y, kw, 19);
        c.scr.round_rect(kr, 5, if th.dark { 0x3A3B41 } else { 0xEDEDF2 });
        c.scr.text(&FONT_SMALL, kr.x + 7, y + 2, key.as_bytes(), th.text);
        c.scr.text(&FONT_UI, r.x + 220, y, what.as_bytes(), th.text2);
    }
}
