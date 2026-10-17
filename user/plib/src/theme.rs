//! Paletas do sistema (claro/escuro) e cores de destaque.

pub const ACCENTS: [(u32, &str); 8] = [
    (0x0A84FF, "Azul"),
    (0xBF5AF2, "Roxo"),
    (0xFF375F, "Rosa"),
    (0xFF453A, "Vermelho"),
    (0xFF9F0A, "Laranja"),
    (0xFFC800, "Amarelo"),
    (0x30C552, "Verde"),
    (0x8E8E93, "Grafite"),
];

#[derive(Clone, Copy)]
pub struct Theme {
    pub dark: bool,
    pub accent: u32,
    /// fundo do conteudo das janelas
    pub win_bg: u32,
    /// barra lateral / areas secundarias
    pub side_bg: u32,
    /// barra de titulo (topo e base do gradiente)
    pub title_top: u32,
    pub title_bot: u32,
    pub text: u32,
    pub text2: u32,
    pub text3: u32,
    pub sep: u32,
    /// botoes e campos
    pub ctrl_bg: u32,
    pub ctrl_border: u32,
    pub field_bg: u32,
    /// menus suspensos / paineis flutuantes
    pub menu_bg: u32,
    pub menu_alpha: u32,
    /// tom e opacidade do vidro (barra de menus, dock)
    pub glass: u32,
    pub glass_alpha: u32,
    pub hover: u32,
    pub border: u32,
}

impl Theme {
    pub fn new(dark: bool, accent: u32) -> Theme {
        if dark {
            Theme {
                dark, accent,
                win_bg: 0x1E1F24, side_bg: 0x26272D,
                title_top: 0x2D2E34, title_bot: 0x27282D,
                text: 0xF2F2F7, text2: 0xA1A1A8, text3: 0x6C6C73,
                sep: 0x3A3A40,
                ctrl_bg: 0x3A3B41, ctrl_border: 0x4A4B52, field_bg: 0x2A2B30,
                menu_bg: 0x2A2B31, menu_alpha: 245,
                glass: 0x1C1D22, glass_alpha: 150,
                hover: 0x3A3B42,
                border: 0x505158,
            }
        } else {
            Theme {
                dark, accent,
                win_bg: 0xFFFFFF, side_bg: 0xF2F2F6,
                title_top: 0xF7F7F9, title_bot: 0xEDEDF0,
                text: 0x1D1D1F, text2: 0x6E6E73, text3: 0xA0A0A6,
                sep: 0xE2E2E7,
                ctrl_bg: 0xFFFFFF, ctrl_border: 0xD2D2D7, field_bg: 0xFFFFFF,
                menu_bg: 0xF6F6F8, menu_alpha: 245,
                glass: 0xF4F5F8, glass_alpha: 150,
                hover: 0xE8E8ED,
                border: 0xC8C8CE,
            }
        }
    }
}
