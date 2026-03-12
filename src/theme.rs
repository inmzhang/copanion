use std::sync::{OnceLock, RwLock};

use clap::ValueEnum;
use ratatui::style::Color;

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
    pub syntect_theme: &'static str,
}

impl ThemeName {
    pub fn theme(self) -> Theme {
        match self {
            ThemeName::Dark => Theme {
                bg: rgb(14, 20, 28),
                panel: rgb(18, 28, 38),
                border: rgb(62, 86, 108),
                border_focus: rgb(129, 194, 255),
                text: rgb(228, 237, 245),
                muted: rgb(138, 159, 178),
                accent: rgb(102, 214, 201),
                note_bg: rgb(22, 45, 58),
                note_border: rgb(102, 214, 201),
                question_bg: rgb(59, 42, 21),
                question_border: rgb(236, 194, 83),
                cursor_line: rgb(39, 55, 71),
                danger: rgb(234, 106, 108),
                success: rgb(144, 203, 104),
                syntect_theme: "base16-ocean.dark",
            },
            ThemeName::Light => Theme {
                bg: rgb(244, 241, 235),
                panel: rgb(252, 250, 245),
                border: rgb(171, 162, 151),
                border_focus: rgb(35, 102, 140),
                text: rgb(33, 37, 41),
                muted: rgb(105, 112, 119),
                accent: rgb(0, 135, 122),
                note_bg: rgb(224, 242, 237),
                note_border: rgb(0, 135, 122),
                question_bg: rgb(252, 236, 209),
                question_border: rgb(182, 128, 32),
                cursor_line: rgb(226, 234, 241),
                danger: rgb(190, 57, 61),
                success: rgb(53, 122, 74),
                syntect_theme: "InspiredGitHub",
            },
            ThemeName::OneDark => Theme {
                bg: rgb(24, 28, 35),
                panel: rgb(34, 39, 46),
                border: rgb(81, 90, 99),
                border_focus: rgb(97, 175, 239),
                text: rgb(220, 223, 228),
                muted: rgb(146, 152, 161),
                accent: rgb(86, 182, 194),
                note_bg: rgb(30, 51, 60),
                note_border: rgb(86, 182, 194),
                question_bg: rgb(66, 48, 29),
                question_border: rgb(229, 192, 123),
                cursor_line: rgb(44, 52, 63),
                danger: rgb(224, 108, 117),
                success: rgb(152, 195, 121),
                syntect_theme: "base16-eighties.dark",
            },
            ThemeName::GruvboxDark => Theme {
                bg: rgb(29, 32, 33),
                panel: rgb(40, 40, 40),
                border: rgb(102, 92, 84),
                border_focus: rgb(131, 165, 152),
                text: rgb(235, 219, 178),
                muted: rgb(168, 153, 132),
                accent: rgb(142, 192, 124),
                note_bg: rgb(50, 73, 48),
                note_border: rgb(142, 192, 124),
                question_bg: rgb(78, 58, 36),
                question_border: rgb(250, 189, 47),
                cursor_line: rgb(60, 56, 54),
                danger: rgb(251, 73, 52),
                success: rgb(184, 187, 38),
                syntect_theme: "Solarized (dark)",
            },
            ThemeName::GruvboxLight => Theme {
                bg: rgb(251, 241, 199),
                panel: rgb(249, 245, 215),
                border: rgb(189, 174, 147),
                border_focus: rgb(69, 133, 136),
                text: rgb(60, 56, 54),
                muted: rgb(124, 111, 100),
                accent: rgb(104, 157, 106),
                note_bg: rgb(228, 238, 212),
                note_border: rgb(104, 157, 106),
                question_bg: rgb(246, 227, 185),
                question_border: rgb(215, 153, 33),
                cursor_line: rgb(235, 228, 194),
                danger: rgb(204, 36, 29),
                success: rgb(121, 116, 14),
                syntect_theme: "Solarized (light)",
            },
            ThemeName::CatppuccinMocha => Theme {
                bg: rgb(24, 24, 37),
                panel: rgb(30, 30, 46),
                border: rgb(88, 91, 112),
                border_focus: rgb(137, 180, 250),
                text: rgb(205, 214, 244),
                muted: rgb(166, 173, 200),
                accent: rgb(148, 226, 213),
                note_bg: rgb(31, 54, 62),
                note_border: rgb(148, 226, 213),
                question_bg: rgb(70, 57, 35),
                question_border: rgb(249, 226, 175),
                cursor_line: rgb(49, 50, 68),
                danger: rgb(243, 139, 168),
                success: rgb(166, 227, 161),
                syntect_theme: "base16-mocha.dark",
            },
            ThemeName::CatppuccinLatte => Theme {
                bg: rgb(239, 241, 245),
                panel: rgb(230, 233, 239),
                border: rgb(156, 160, 176),
                border_focus: rgb(30, 102, 245),
                text: rgb(76, 79, 105),
                muted: rgb(108, 111, 133),
                accent: rgb(23, 146, 153),
                note_bg: rgb(213, 237, 236),
                note_border: rgb(23, 146, 153),
                question_bg: rgb(248, 234, 209),
                question_border: rgb(223, 142, 29),
                cursor_line: rgb(220, 224, 232),
                danger: rgb(210, 15, 57),
                success: rgb(64, 160, 43),
                syntect_theme: "base16-ocean.light",
            },
            ThemeName::AyuLight => Theme {
                bg: rgb(250, 248, 240),
                panel: rgb(255, 253, 245),
                border: rgb(204, 199, 178),
                border_focus: rgb(36, 116, 144),
                text: rgb(77, 84, 86),
                muted: rgb(112, 121, 123),
                accent: rgb(85, 149, 77),
                note_bg: rgb(233, 244, 228),
                note_border: rgb(85, 149, 77),
                question_bg: rgb(252, 239, 214),
                question_border: rgb(250, 173, 37),
                cursor_line: rgb(236, 233, 222),
                danger: rgb(242, 117, 113),
                success: rgb(134, 179, 0),
                syntect_theme: "InspiredGitHub",
            },
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

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::Rgb(r, g, b)
}
