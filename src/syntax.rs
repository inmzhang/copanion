use std::path::Path;
use std::sync::OnceLock;

use ratatui::style::{Color, Modifier, Style};
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, Theme, ThemeSet};
use syntect::parsing::SyntaxSet;

pub type StyledSegments = Vec<(Style, String)>;

pub fn highlight_file(path: &str, lines: &[String]) -> Vec<StyledSegments> {
    let syntax_set = syntax_set();
    let syntax = syntax_set
        .find_syntax_for_file(Path::new(path))
        .ok()
        .flatten()
        .unwrap_or_else(|| syntax_set.find_syntax_plain_text());
    let mut highlighter = HighlightLines::new(syntax, theme());

    lines
        .iter()
        .map(|line| {
            highlighter
                .highlight_line(line, syntax_set)
                .map(|segments| {
                    segments
                        .into_iter()
                        .map(|(style, text)| (to_ratatui_style(style), text.to_string()))
                        .collect()
                })
                .unwrap_or_else(|_| vec![(Style::default(), line.clone())])
        })
        .collect()
}

fn syntax_set() -> &'static SyntaxSet {
    static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
    SYNTAX_SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn theme() -> &'static Theme {
    static THEME: OnceLock<Theme> = OnceLock::new();
    THEME.get_or_init(|| {
        let themes = ThemeSet::load_defaults();
        themes
            .themes
            .get("base16-ocean.dark")
            .cloned()
            .unwrap_or_else(|| themes.themes.values().next().cloned().unwrap_or_default())
    })
}

fn to_ratatui_style(style: syntect::highlighting::Style) -> Style {
    let mut ratatui_style = Style::default()
        .fg(Color::Rgb(
            style.foreground.r,
            style.foreground.g,
            style.foreground.b,
        ))
        .bg(Color::Rgb(
            style.background.r,
            style.background.g,
            style.background.b,
        ));
    if style.font_style.contains(FontStyle::BOLD) {
        ratatui_style = ratatui_style.add_modifier(Modifier::BOLD);
    }
    if style.font_style.contains(FontStyle::ITALIC) {
        ratatui_style = ratatui_style.add_modifier(Modifier::ITALIC);
    }
    if style.font_style.contains(FontStyle::UNDERLINE) {
        ratatui_style = ratatui_style.add_modifier(Modifier::UNDERLINED);
    }
    ratatui_style
}

#[cfg(test)]
mod tests {
    use super::highlight_file;

    #[test]
    fn rust_sources_receive_styled_segments() {
        let highlighted = highlight_file(
            "src/main.rs",
            &[String::from("fn main() { println!(\"hi\"); }")],
        );
        assert_eq!(highlighted.len(), 1);
        assert!(!highlighted[0].is_empty());
    }
}
