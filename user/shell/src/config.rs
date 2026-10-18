//! Configuracao do ambiente, persistida em texto puro no arquivo
//! `pulsar.cfg` (estilo dotfile do Linux). Pode ser editada pelo app
//! Configuracoes ou a mao no Editor — o WM relê o arquivo sozinho.
use plib::*;

pub const CFG_NAME: &[u8] = b"pulsar.cfg";

pub const WALLPAPERS: [&str; 6] = ["Aurora", "Ondas", "Oceano", "Pôr do sol", "Grafite", "Nebulosa"];
const WALL_KEYS: [&str; 6] = ["aurora", "ondas", "oceano", "por-do-sol", "grafite", "nebulosa"];
const ACCENT_KEYS: [&str; 8] = ["azul", "roxo", "rosa", "vermelho", "laranja", "amarelo", "verde", "grafite"];
pub const DOCK_SIZES: [&str; 3] = ["Pequeno", "Médio", "Grande"];
const DOCK_SIZE_KEYS: [&str; 3] = ["small", "medium", "large"];

#[derive(Clone, Copy, PartialEq)]
pub struct Config {
    pub dark: bool,
    pub accent: u8,
    pub wallpaper: u8,
    pub dock_size: u8,
    pub dock_magnify: bool,
    pub dock_left: bool,
    pub clock24: bool,
    pub seconds: bool,
    pub cpu_meter: bool,
    pub glass: bool,
    pub layout: Layout,
    /// clicar no icone do app em foco minimiza (como a barra do Windows)
    pub dock_minimize: bool,
}

impl Config {
    pub const fn default() -> Config {
        Config {
            dark: false, accent: 0, wallpaper: 0, dock_size: 1, dock_magnify: true,
            dock_left: false, clock24: true, seconds: false, cpu_meter: true, glass: true, layout: Layout::Abnt2, dock_minimize: false,
        }
    }

    pub fn accent_rgb(&self) -> u32 {
        plib::theme::ACCENTS[self.accent as usize % 8].0
    }

    pub fn icon_size(&self) -> i32 {
        [44, 54, 64][self.dock_size as usize % 3]
    }

    fn parse(&mut self, text: &[u8]) {
        for line in text.split(|&c| c == b'\n') {
            let line = trim(line);
            if line.is_empty() || line[0] == b'#' { continue; }
            let Some(eq) = line.iter().position(|&c| c == b'=') else { continue };
            let (k, v) = (trim(&line[..eq]), trim(&line[eq + 1..]));
            let on = matches!(v, b"on" | b"1" | b"true" | b"sim");
            match k {
                b"theme" => self.dark = v == b"dark",
                b"accent" => if let Some(i) = find(&ACCENT_KEYS, v) { self.accent = i },
                b"wallpaper" => if let Some(i) = find(&WALL_KEYS, v) { self.wallpaper = i },
                b"dock.size" => if let Some(i) = find(&DOCK_SIZE_KEYS, v) { self.dock_size = i },
                b"dock.magnify" => self.dock_magnify = on,
                b"dock.position" => self.dock_left = v == b"left",
                b"dock.click_minimizes" => self.dock_minimize = on,
                b"clock.24h" => self.clock24 = on,
                b"clock.seconds" => self.seconds = on,
                b"menubar.cpu" => self.cpu_meter = on,
                b"glass" => self.glass = on,
                b"keyboard" => { if let Some(l) = Layout::from_name(v) { self.layout = l; } }
                _ => {}
            }
        }
    }

    pub fn serialize(&self, out: &mut Buf<1024>) {
        let b = |v: bool| if v { "on" } else { "off" };
        out.clear();
        out.s("# Pulsar OS — configuração do ambiente (pulsar.cfg)\n")
            .s("# Edite aqui ou em Configurações; o desktop aplica ao salvar.\n")
            .s("# theme: light|dark  accent: azul|roxo|rosa|vermelho|laranja|amarelo|verde|grafite\n")
            .s("# wallpaper: aurora|ondas|oceano|por-do-sol|grafite|nebulosa\n")
            .s("# dock.size: small|medium|large  dock.position: bottom|left  keyboard: abnt|abnt2|us|us-intl\n")
            .s("# dock.click_minimizes: on = clicar no app em foco minimiza (como no Windows)\n\n");
        out.s("theme=").s(if self.dark { "dark" } else { "light" }).s("\n");
        out.s("accent=").s(ACCENT_KEYS[self.accent as usize % 8]).s("\n");
        out.s("wallpaper=").s(WALL_KEYS[self.wallpaper as usize % 6]).s("\n");
        out.s("dock.size=").s(DOCK_SIZE_KEYS[self.dock_size as usize % 3]).s("\n");
        out.s("dock.magnify=").s(b(self.dock_magnify)).s("\n");
        out.s("dock.position=").s(if self.dock_left { "left" } else { "bottom" }).s("\n");
        out.s("dock.click_minimizes=").s(b(self.dock_minimize)).s("\n");
        out.s("clock.24h=").s(b(self.clock24)).s("\n");
        out.s("clock.seconds=").s(b(self.seconds)).s("\n");
        out.s("menubar.cpu=").s(b(self.cpu_meter)).s("\n");
        out.s("glass=").s(b(self.glass)).s("\n");
        out.s("keyboard=").s(self.layout.name()).s("\n");
    }
}

fn trim(s: &[u8]) -> &[u8] {
    let a = s.iter().position(|c| !c.is_ascii_whitespace()).unwrap_or(s.len());
    let b = s.iter().rposition(|c| !c.is_ascii_whitespace()).map(|p| p + 1).unwrap_or(a);
    &s[a..b.max(a)]
}

fn find(keys: &[&str], v: &[u8]) -> Option<u8> {
    keys.iter().position(|k| k.as_bytes() == v).map(|i| i as u8)
}

/// Le o arquivo de configuracao. Retorna (config, conteudo bruto, tamanho).
pub fn load(raw: &mut [u8; 2048]) -> (Config, usize) {
    let mut c = Config::default();
    let Some(idx) = find_file(CFG_NAME) else { return (c, 0) };
    let n = fs_read(idx, raw);
    if n <= 0 { return (c, 0); }
    c.parse(&raw[..n as usize]);
    (c, n as usize)
}

/// Grava a configuracao (cria o arquivo se preciso). Retorna o conteudo.
pub fn save(c: &Config, out: &mut Buf<1024>) -> bool {
    c.serialize(out);
    let idx = match find_file(CFG_NAME) {
        Some(i) => i as i64,
        None => fs_create(CFG_NAME),
    };
    idx >= 0 && fs_write(idx as usize, out.get()) == 0
}
