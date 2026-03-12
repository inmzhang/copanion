use std::sync::{OnceLock, RwLock};

use clap::ValueEnum;
use ratatui::style::Color;
use two_face::theme::EmbeddedThemeName;

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum ThemeName {
    #[default]
    Dark,
    Light,
    OneDark,
    GruvboxDark,
    GruvboxLight,
    CatppuccinMocha,
    CatppuccinLatte,
    AyuLight,
}

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub bg: Color,
    pub panel: Color,
    pub border: Color,
    pub border_focus: Color,
    pub text: Color,
    pub muted: Color,
    pub accent: Color,
    pub note_bg: Color,
    pub note_border: Color,
    pub question_bg: Color,
    pub question_border: Color,
    pub cursor_line: Color,
    pub danger: Color,
    pub success: Color,
    pub syntect_theme: EmbeddedThemeName,
}

impl ThemeName {
    pub fn parse_config(value: &str) -> Option<Self> {
        <Self as clap::ValueEnum>::from_str(value, true).ok()
    }

    pub fn theme(self) -> Theme {
        match self {
            ThemeName::Dark => dark_theme(),
            ThemeName::Light => light_theme(),
            ThemeName::OneDark => onedark_theme(),
            ThemeName::GruvboxDark => gruvbox_dark_theme(),
            ThemeName::GruvboxLight => gruvbox_light_theme(),
            ThemeName::CatppuccinMocha => catppuccin_mocha_theme(),
            ThemeName::CatppuccinLatte => catppuccin_latte_theme(),
            ThemeName::AyuLight => ayu_light_theme(),
        }
    }
}

pub fn set_active(theme_name: ThemeName) {
    let lock = active_theme_name_lock();
    *lock.write().expect("theme lock poisoned") = theme_name;
}

pub fn active_name() -> ThemeName {
    *active_theme_name_lock()
        .read()
        .expect("theme lock poisoned")
}

pub fn active() -> Theme {
    active_name().theme()
}

fn active_theme_name_lock() -> &'static RwLock<ThemeName> {
    static ACTIVE_THEME: OnceLock<RwLock<ThemeName>> = OnceLock::new();
    ACTIVE_THEME.get_or_init(|| RwLock::new(ThemeName::default()))
}

#[derive(Clone, Copy)]
struct CatppuccinFlavor {
    base: Color,
    mantle: Color,
    surface1: Color,
    surface2: Color,
    text: Color,
    overlay0: Color,
    red: Color,
    yellow: Color,
    green: Color,
    teal: Color,
    blue: Color,
}

#[derive(Clone, Copy)]
struct GruvboxFlavor {
    dark: bool,
    bg0: Color,
    bg1: Color,
    bg4: Color,
    selected_bg: Color,
    fg0: Color,
    grey0: Color,
    red: Color,
    yellow: Color,
    green: Color,
    aqua: Color,
    blue: Color,
}

fn dark_theme() -> Theme {
    themed(
        rgb(30, 30, 30),
        rgb(24, 24, 28),
        rgb(110, 110, 110),
        rgb(90, 200, 255),
        Color::White,
        rgb(160, 160, 160),
        rgb(90, 220, 240),
        rgb(90, 170, 255),
        rgb(255, 210, 90),
        rgb(70, 70, 70),
        rgb(240, 90, 90),
        rgb(80, 220, 120),
        EmbeddedThemeName::Base16EightiesDark,
    )
}

fn light_theme() -> Theme {
    themed(
        rgb(210, 210, 220),
        rgb(245, 243, 232),
        rgb(100, 100, 100),
        rgb(0, 60, 140),
        rgb(0, 0, 0),
        rgb(80, 80, 80),
        rgb(0, 100, 120),
        rgb(0, 60, 140),
        rgb(140, 80, 0),
        rgb(200, 200, 220),
        rgb(160, 0, 0),
        rgb(0, 100, 0),
        EmbeddedThemeName::Base16OceanLight,
    )
}

fn ayu_light_theme() -> Theme {
    themed(
        rgb(255, 255, 255),
        rgb(250, 250, 250),
        rgb(217, 216, 215),
        rgb(54, 163, 217),
        rgb(92, 103, 115),
        rgb(171, 176, 182),
        rgb(54, 163, 217),
        rgb(54, 163, 217),
        rgb(231, 197, 71),
        rgb(240, 238, 228),
        rgb(240, 113, 120),
        rgb(134, 179, 0),
        EmbeddedThemeName::OneHalfLight,
    )
}

fn onedark_theme() -> Theme {
    themed(
        rgb(33, 37, 43),
        rgb(40, 44, 52),
        rgb(62, 68, 82),
        rgb(97, 175, 239),
        rgb(171, 178, 191),
        rgb(92, 99, 112),
        rgb(86, 182, 194),
        rgb(97, 175, 239),
        rgb(229, 192, 123),
        rgb(62, 68, 82),
        rgb(224, 108, 117),
        rgb(152, 195, 121),
        EmbeddedThemeName::OneHalfDark,
    )
}

fn catppuccin_latte_theme() -> Theme {
    let flavor = CatppuccinFlavor {
        base: rgb(239, 241, 245),
        mantle: rgb(230, 233, 239),
        surface1: rgb(188, 192, 204),
        surface2: rgb(172, 176, 190),
        text: rgb(76, 79, 105),
        overlay0: rgb(156, 160, 176),
        red: rgb(210, 15, 57),
        yellow: rgb(223, 142, 29),
        green: rgb(64, 160, 43),
        teal: rgb(23, 146, 153),
        blue: rgb(30, 102, 245),
    };
    catppuccin_theme(flavor, EmbeddedThemeName::CatppuccinLatte)
}

fn catppuccin_mocha_theme() -> Theme {
    let flavor = CatppuccinFlavor {
        base: rgb(30, 30, 46),
        mantle: rgb(24, 24, 37),
        surface1: rgb(69, 71, 90),
        surface2: rgb(88, 91, 112),
        text: rgb(205, 214, 244),
        overlay0: rgb(108, 112, 134),
        red: rgb(243, 139, 168),
        yellow: rgb(249, 226, 175),
        green: rgb(166, 227, 161),
        teal: rgb(148, 226, 213),
        blue: rgb(137, 180, 250),
    };
    catppuccin_theme(flavor, EmbeddedThemeName::CatppuccinMocha)
}

fn gruvbox_dark_theme() -> Theme {
    let flavor = GruvboxFlavor {
        dark: true,
        bg0: rgb(29, 32, 33),
        bg1: rgb(40, 40, 40),
        bg4: rgb(80, 73, 69),
        selected_bg: rgb(60, 56, 54),
        fg0: rgb(212, 190, 152),
        grey0: rgb(124, 111, 100),
        red: rgb(251, 73, 52),
        yellow: rgb(250, 189, 47),
        green: rgb(184, 187, 38),
        aqua: rgb(142, 192, 124),
        blue: rgb(131, 165, 152),
    };
    gruvbox_theme(flavor)
}

fn gruvbox_light_theme() -> Theme {
    let flavor = GruvboxFlavor {
        dark: false,
        bg0: rgb(249, 245, 215),
        bg1: rgb(245, 237, 202),
        bg4: rgb(221, 199, 161),
        selected_bg: rgb(235, 219, 178),
        fg0: rgb(101, 71, 53),
        grey0: rgb(168, 153, 132),
        red: rgb(157, 0, 6),
        yellow: rgb(181, 118, 20),
        green: rgb(121, 116, 14),
        aqua: rgb(66, 123, 88),
        blue: rgb(7, 102, 120),
    };
    gruvbox_theme(flavor)
}

fn catppuccin_theme(flavor: CatppuccinFlavor, syntect_theme: EmbeddedThemeName) -> Theme {
    themed(
        flavor.mantle,
        flavor.base,
        flavor.surface2,
        flavor.blue,
        flavor.text,
        flavor.overlay0,
        flavor.teal,
        flavor.blue,
        flavor.yellow,
        flavor.surface1,
        flavor.red,
        flavor.green,
        syntect_theme,
    )
}

fn gruvbox_theme(flavor: GruvboxFlavor) -> Theme {
    let syntect_theme = if flavor.dark {
        EmbeddedThemeName::GruvboxDark
    } else {
        EmbeddedThemeName::GruvboxLight
    };
    themed(
        flavor.bg1,
        flavor.bg0,
        flavor.bg4,
        flavor.aqua,
        flavor.fg0,
        flavor.grey0,
        flavor.aqua,
        flavor.blue,
        flavor.yellow,
        flavor.selected_bg,
        flavor.red,
        flavor.green,
        syntect_theme,
    )
}

fn themed(
    bg: Color,
    panel: Color,
    border: Color,
    border_focus: Color,
    text: Color,
    muted: Color,
    accent: Color,
    note_border: Color,
    question_border: Color,
    cursor_line: Color,
    danger: Color,
    success: Color,
    syntect_theme: EmbeddedThemeName,
) -> Theme {
    Theme {
        bg,
        panel,
        border,
        border_focus,
        text,
        muted,
        accent,
        note_bg: blend(panel, note_border, 20),
        note_border,
        question_bg: blend(panel, question_border, 20),
        question_border,
        cursor_line,
        danger,
        success,
        syntect_theme,
    }
}

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::Rgb(r, g, b)
}

fn blend(base: Color, accent: Color, accent_percent: u8) -> Color {
    debug_assert!(accent_percent <= 100);
    match (base, accent) {
        (Color::Rgb(br, bg, bb), Color::Rgb(ar, ag, ab)) => {
            let p = u16::from(accent_percent);
            let inv = 100_u16.saturating_sub(p);
            let mix = |base_component: u8, accent_component: u8| -> u8 {
                ((u16::from(base_component) * inv + u16::from(accent_component) * p) / 100) as u8
            };
            rgb(mix(br, ar), mix(bg, ag), mix(bb, ab))
        }
        _ => accent,
    }
}

#[cfg(test)]
mod tests {
    use super::{ThemeName, rgb};
    use two_face::theme::EmbeddedThemeName;

    #[test]
    fn gruvbox_dark_matches_tuicr_palette_and_theme_name() {
        let theme = ThemeName::GruvboxDark.theme();
        assert_eq!(theme.bg, rgb(40, 40, 40));
        assert_eq!(theme.panel, rgb(29, 32, 33));
        assert_eq!(theme.border_focus, rgb(142, 192, 124));
        assert_eq!(theme.note_border, rgb(131, 165, 152));
        assert_eq!(theme.syntect_theme, EmbeddedThemeName::GruvboxDark);
    }

    #[test]
    fn gruvbox_light_uses_embedded_gruvbox_syntax_theme() {
        let theme = ThemeName::GruvboxLight.theme();
        assert_eq!(theme.syntect_theme, EmbeddedThemeName::GruvboxLight);
        assert_eq!(theme.question_border, rgb(181, 118, 20));
    }
}
