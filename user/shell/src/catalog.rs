//! Catalogo de aplicativos e menus da barra de menus.
use plib::icons::Icon;
use plib::*;

/// O que um item de menu (ou botao) faz.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    None,
    About,
    Settings(u8),
    Help,
    Launch(u8),
    Poweroff,
    NewTextDoc,
    OpenFileIdx(u16),
    OpenWithEditor(u16),
    FileInfo(u16),
    AppMenu(u16),
    CloseWin,
    QuitApp,
    ForceQuit,
    MinimizeWin,
    ZoomWin,
    SnapLeft,
    SnapRight,
    TileAll,
    Cascade,
    ShowDesktop,
    CloseAll,
    Spotlight,
    ToggleDark,
    ToggleMagnify,
    SetAccent(u8),
    SetWallpaper(u8),
    SetResolution(u16, u16),
    DockSize(u8),
    DockLeft(bool),
    SetClock24(bool),
    SetSeconds(bool),
    SetCpuMeter(bool),
    SetGlass(bool),
    SetLayout(Layout),
    SetDockMinimize(bool),
    SettingsPage(u8),
    EditConfig,
    FilesView(u8),
    FilesFilter(u8),
    DialogCancel,
    DialogConfirm,
    /// clique simples num arquivo do Finder
    FileSel(u16),
    /// "Abrir" no menu de contexto de um icone do dock
    DockOpen,
    /// abre a Central de Controle
    ControlCenter,
}

/// Item de menu: rotulo ("-" = separador), acao, atalho exibido.
#[derive(Clone, Copy)]
pub struct MI(pub &'static str, pub Action, pub &'static str);

pub const SEP: MI = MI("-", Action::None, "");

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// janela desenhada pelo proprio WM
    Internal,
    /// processo separado (.pulse)
    Process,
}

pub struct AppDef {
    pub id: u8,
    pub name: &'static str,
    pub bin: &'static str,
    pub icon: Icon,
    pub kind: Kind,
    /// area cliente preferida
    pub w: i32,
    pub h: i32,
    /// pode haver varias janelas (processos) ao mesmo tempo
    pub multi: bool,
    pub menus: &'static [(&'static str, &'static [MI])],
    pub keywords: &'static str,
    /// app_id Wayland dos programas Linux (xdg_toplevel.set_app_id): a
    /// janela que o wlclient abre com ele e deste app ("" = nenhum)
    pub wl_id: &'static str,
}

pub const APP_FILES: u8 = 0;
pub const APP_EDITOR: u8 = 1;
pub const APP_BALL: u8 = 2;
pub const APP_CALC: u8 = 3;
pub const APP_SETTINGS: u8 = 4;
pub const APP_TERMINAL: u8 = 5;
pub const APP_NET: u8 = 6;
pub const APP_VELA: u8 = 7;
pub const APP_MONITOR: u8 = 8;
pub const APP_ABOUT: u8 = 9;
pub const APP_HELP: u8 = 10;
/// Programa .pulse fora do catalogo (ex.: aberto pelo Terminal).
pub const APP_GENERIC: u8 = 11;
/// Firefox (programa Linux, /usr/bin/firefox): o navegador padrao por ora.
pub const APP_FIREFOX: u8 = 12;

const M_FILES: &[(&str, &[MI])] = &[
    ("Arquivo", &[
        MI("Novo documento de texto", Action::NewTextDoc, "Ctrl+N"),
        MI("Abrir Terminal", Action::Launch(APP_TERMINAL), "Ctrl+Alt+T"),
        SEP,
        MI("Fechar janela", Action::CloseWin, "Ctrl+W"),
    ]),
    ("Visualizar", &[
        MI("Como lista", Action::FilesView(0), ""),
        MI("Como ícones", Action::FilesView(1), ""),
    ]),
    ("Ir", &[
        MI("Todos os arquivos", Action::FilesFilter(0), ""),
        MI("Aplicativos", Action::FilesFilter(1), ""),
        MI("Documentos", Action::FilesFilter(2), ""),
    ]),
];

const M_EDITOR: &[(&str, &[MI])] = &[
    ("Arquivo", &[
        MI("Novo", Action::AppMenu(APP_MENU_NEW), "Ctrl+N"),
        MI("Salvar", Action::AppMenu(APP_MENU_SAVE), "Ctrl+S"),
        SEP,
        MI("Fechar", Action::CloseWin, "Ctrl+W"),
    ]),
    ("Editar", &[
        MI("Desfazer", Action::AppMenu(APP_MENU_UNDO), "Ctrl+Z"),
        SEP,
        MI("Copiar linha", Action::AppMenu(APP_MENU_COPY), "Ctrl+C"),
        MI("Colar", Action::AppMenu(APP_MENU_PASTE), "Ctrl+V"),
        MI("Selecionar tudo", Action::AppMenu(APP_MENU_SELECT_ALL), "Ctrl+A"),
    ]),
    ("Visualizar", &[
        MI("Aumentar fonte", Action::AppMenu(APP_MENU_ZOOM_IN), "Ctrl++"),
        MI("Diminuir fonte", Action::AppMenu(APP_MENU_ZOOM_OUT), "Ctrl+-"),
    ]),
];

const M_TERMINAL: &[(&str, &[MI])] = &[
    ("Shell", &[
        MI("Limpar tela", Action::AppMenu(APP_MENU_CLEAR), "Ctrl+L"),
        SEP,
        MI("Fechar", Action::CloseWin, "Ctrl+W"),
    ]),
    ("Editar", &[
        MI("Copiar", Action::AppMenu(APP_MENU_COPY), ""),
        MI("Colar", Action::AppMenu(APP_MENU_PASTE), ""),
    ]),
];

const M_CALC: &[(&str, &[MI])] = &[
    ("Calcular", &[MI("Limpar", Action::AppMenu(APP_MENU_CLEAR), "Esc")]),
];

const M_BALL: &[(&str, &[MI])] = &[
    ("Animação", &[
        MI("Pausar / continuar", Action::AppMenu(APP_MENU_PAUSE), "Espaço"),
        MI("Adicionar bola", Action::AppMenu(APP_MENU_ADD), "+"),
        MI("Reiniciar", Action::AppMenu(APP_MENU_RESET), "R"),
    ]),
];

const M_NET: &[(&str, &[MI])] = &[
    ("Rede", &[
        MI("Atualizar", Action::AppMenu(APP_MENU_RELOAD), "Ctrl+R"),
        MI("Enviar ping UDP", Action::AppMenu(APP_MENU_ADD), ""),
    ]),
];

const M_VELA: &[(&str, &[MI])] = &[
    ("Arquivo", &[
        MI("Recarregar", Action::AppMenu(APP_MENU_RELOAD), "Ctrl+R"),
        MI("Limpar página", Action::AppMenu(APP_MENU_CLEAR), ""),
        SEP,
        MI("Fechar", Action::CloseWin, "Ctrl+W"),
    ]),
];

const M_MONITOR: &[(&str, &[MI])] = &[
    ("Processo", &[
        MI("Encerrar processo", Action::AppMenu(APP_MENU_CLEAR), "Del"),
        MI("Atualizar", Action::AppMenu(APP_MENU_RELOAD), ""),
    ]),
];

const M_NONE: &[(&str, &[MI])] = &[];

pub static APPS: [AppDef; NAPPS] = [
    AppDef { id: APP_FILES, name: "Arquivos", bin: "", icon: Icon::Files, kind: Kind::Internal,
        w: 720, h: 420, multi: false, menus: M_FILES, keywords: "finder explorer disco documentos", wl_id: "" },
    AppDef { id: APP_EDITOR, name: "Editor", bin: "editor.pulse", icon: Icon::Editor, kind: Kind::Process,
        w: 620, h: 440, multi: true, menus: M_EDITOR, keywords: "texto notepad bloco notas", wl_id: "" },
    AppDef { id: APP_BALL, name: "Bolhas", bin: "ball.pulse", icon: Icon::Ball, kind: Kind::Process,
        w: 480, h: 360, multi: false, menus: M_BALL, keywords: "ball demo animacao fisica", wl_id: "" },
    AppDef { id: APP_CALC, name: "Calculadora", bin: "calc.pulse", icon: Icon::Calc, kind: Kind::Process,
        w: 260, h: 380, multi: false, menus: M_CALC, keywords: "calc contas", wl_id: "" },
    AppDef { id: APP_SETTINGS, name: "Configurações", bin: "", icon: Icon::Settings, kind: Kind::Internal,
        w: 780, h: 480, multi: false, menus: M_NONE, keywords: "preferencias ajustes tema papel parede tela resolucao monitor display dock painel controle", wl_id: "" },
    AppDef { id: APP_TERMINAL, name: "Terminal", bin: "terminal.pulse", icon: Icon::Terminal, kind: Kind::Process,
        w: 600, h: 380, multi: false, menus: M_TERMINAL, keywords: "shell console bash cmd prompt", wl_id: "" },
    AppDef { id: APP_NET, name: "Rede", bin: "netmon.pulse", icon: Icon::Network, kind: Kind::Process,
        w: 420, h: 330, multi: false, menus: M_NET, keywords: "network internet ip wifi", wl_id: "" },
    AppDef { id: APP_VELA, name: "Vela", bin: "vela.pulse", icon: Icon::Browser, kind: Kind::Process,
        w: 1040, h: 700, multi: false, menus: M_VELA, keywords: "navegador browser web http internet", wl_id: "" },
    AppDef { id: APP_MONITOR, name: "Monitor do Sistema", bin: "sysmon.pulse", icon: Icon::Monitor, kind: Kind::Process,
        w: 620, h: 440, multi: false, menus: M_MONITOR, keywords: "gerenciador tarefas task manager processos cpu", wl_id: "" },
    AppDef { id: APP_ABOUT, name: "Sobre o Pulsar OS", bin: "", icon: Icon::Pulsar, kind: Kind::Internal,
        w: 520, h: 300, multi: false, menus: M_NONE, keywords: "sobre versao sistema", wl_id: "" },
    AppDef { id: APP_HELP, name: "Atalhos do Teclado", bin: "", icon: Icon::Help, kind: Kind::Internal,
        w: 560, h: 456, multi: false, menus: M_NONE, keywords: "ajuda help atalhos teclado", wl_id: "" },
    AppDef { id: APP_GENERIC, name: "Aplicativo", bin: "", icon: Icon::Program, kind: Kind::Process,
        w: 400, h: 300, multi: true, menus: M_NONE, keywords: "", wl_id: "" },
    // bin com caminho: programa Linux, aberto com o ambiente padrao; a janela
    // vem pelo servico Wayland e e reconhecida pelo app_id
    AppDef { id: APP_FIREFOX, name: "Firefox", bin: "/usr/bin/firefox", icon: Icon::Firefox, kind: Kind::Process,
        w: 1040, h: 700, multi: false, menus: M_NONE, keywords: "navegador browser web http internet mozilla",
        wl_id: "firefox" },
];
pub const NAPPS: usize = 13;

/// Ordem dos icones no dock (APP_SETTINGS fica depois do separador).
pub const DOCK: [u8; 8] = [APP_FILES, APP_FIREFOX, APP_TERMINAL, APP_EDITOR, APP_CALC, APP_MONITOR, APP_NET, APP_BALL];
pub const DOCK_TAIL: u8 = APP_SETTINGS;

pub fn app(id: u8) -> &'static AppDef {
    &APPS[id as usize]
}

pub fn app_by_bin(bin: &[u8]) -> Option<&'static AppDef> {
    APPS.iter().find(|a| !a.bin.is_empty() && a.bin.as_bytes() == bin)
}

/// App do catalogo com este app_id Wayland ("firefox" tambem casa
/// "firefox-esr", o do pacote do Debian).
pub fn app_by_wl_id(id: &[u8]) -> Option<&'static AppDef> {
    APPS.iter().find(|a| {
        let w = a.wl_id.as_bytes();
        !w.is_empty() && id.starts_with(w) && (id.len() == w.len() || id[w.len()] == b'-')
    })
}

/// Programa Linux (bin e um caminho): aberto por spawn_path, sem janela
/// reservada (ela chega pelo servico Wayland).
pub fn is_linux(a: &AppDef) -> bool {
    a.bin.starts_with('/')
}

/// Menu do logo do Pulsar (canto esquerdo da barra).
pub const M_PULSAR: &[MI] = &[
    MI("Sobre o Pulsar OS", Action::About, ""),
    SEP,
    MI("Configurações do Sistema...", Action::Settings(0), "Super+I"),
    MI("Monitor do Sistema", Action::Launch(APP_MONITOR), "Ctrl+Shift+Esc"),
    SEP,
    MI("Forçar encerramento...", Action::Launch(APP_MONITOR), ""),
    SEP,
    MI("Desligar...", Action::Poweroff, ""),
];

pub const M_WINDOW: &[MI] = &[
    MI("Minimizar", Action::MinimizeWin, "Super+Baixo"),
    MI("Maximizar", Action::ZoomWin, "Super+Cima"),
    MI("Encaixar à esquerda", Action::SnapLeft, "Super+Esq."),
    MI("Encaixar à direita", Action::SnapRight, "Super+Dir."),
    SEP,
    MI("Lado a lado", Action::TileAll, ""),
    MI("Organizar em cascata", Action::Cascade, ""),
    MI("Mostrar área de trabalho", Action::ShowDesktop, "Super+D"),
    SEP,
    MI("Fechar todas", Action::CloseAll, ""),
];

pub const M_HELP: &[MI] = &[
    MI("Atalhos do teclado", Action::Help, "F1"),
    MI("Buscar...", Action::Spotlight, "Ctrl+Espaço"),
    SEP,
    MI("Sobre o Pulsar OS", Action::About, ""),
];

/// Menu da area de trabalho (clique direito no papel de parede).
pub const M_DESKTOP: &[MI] = &[
    MI("Novo documento de texto", Action::NewTextDoc, ""),
    MI("Abrir Terminal", Action::Launch(APP_TERMINAL), ""),
    SEP,
    MI("Resolução da tela...", Action::Settings(2), ""),
    MI("Alterar papel de parede...", Action::Settings(1), ""),
    MI("Aparência...", Action::Settings(0), ""),
    MI("Alternar modo escuro", Action::ToggleDark, ""),
    SEP,
    MI("Organizar em cascata", Action::Cascade, ""),
    MI("Mostrar área de trabalho", Action::ShowDesktop, ""),
];

pub const M_DOCK_IDLE: &[MI] = &[MI("Abrir", Action::DockOpen, "")];

pub const M_DOCK_RUNNING: &[MI] = &[
    MI("Mostrar", Action::DockOpen, ""),
    MI("Minimizar", Action::MinimizeWin, ""),
    SEP,
    MI("Encerrar", Action::QuitApp, ""),
    MI("Forçar encerramento", Action::ForceQuit, ""),
];

/// Clique direito num arquivo do Finder (age sobre a selecao).
pub const M_FILE: &[MI] = &[
    MI("Abrir", Action::OpenFileIdx(u16::MAX), "Enter"),
    MI("Abrir com o Editor", Action::OpenWithEditor(u16::MAX), ""),
    SEP,
    MI("Informações", Action::FileInfo(u16::MAX), ""),
    SEP,
    MI("Novo documento de texto", Action::NewTextDoc, ""),
];

pub const M_TITLEBAR: &[MI] = &[
    MI("Minimizar", Action::MinimizeWin, ""),
    MI("Maximizar / restaurar", Action::ZoomWin, ""),
    MI("Encaixar à esquerda", Action::SnapLeft, ""),
    MI("Encaixar à direita", Action::SnapRight, ""),
    SEP,
    MI("Fechar", Action::CloseWin, ""),
];
