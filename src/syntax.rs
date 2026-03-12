use std::path::Path;
use std::sync::OnceLock;

use ratatui::style::{Color, Modifier, Style};
use syntect::easy::HighlightLines;
use syntect::highlighting::FontStyle;
use syntect::parsing::{SyntaxReference, SyntaxSet};
use two_face::theme::{EmbeddedLazyThemeSet, EmbeddedThemeName};

use crate::theme;

pub type StyledSegments = Vec<(Style, String)>;

type HighlightedLines = Vec<Option<StyledSegments>>;

const EXTENSION_FALLBACKS: &[(&str, &str)] = &[
    ("jsx", "js"),
    ("mjs", "js"),
    ("cjs", "js"),
    ("hbs", "html"),
    ("handlebars", "html"),
    ("mustache", "html"),
    ("ejs", "html"),
    ("pug", "html"),
    ("jade", "html"),
    ("njk", "html"),
    ("mdx", "md"),
    ("jsonc", "json"),
    ("json5", "json"),
    ("prisma", "json"),
    ("heex", "rb"),
];

const FILENAME_FALLBACKS: &[(&str, &str)] = &[
    ("Containerfile", "sh"),
    ("Justfile", "sh"),
    ("justfile", "sh"),
];

pub fn highlight_file(path: &str, lines: &[String]) -> Vec<StyledSegments> {
    let highlighter = SyntaxHighlighter::new(theme::active().syntect_theme);
    highlighter
        .highlight_file_lines(Path::new(path), lines)
        .unwrap_or_else(|| {
            lines
                .iter()
                .map(|line| Some(default_segments(line)))
                .collect()
        })
        .into_iter()
        .zip(lines.iter())
        .map(|(segments, line)| segments.unwrap_or_else(|| default_segments(line)))
        .collect()
}

struct SyntaxHighlighter {
    theme_name: EmbeddedThemeName,
}

impl Default for SyntaxHighlighter {
    fn default() -> Self {
        Self::new(EmbeddedThemeName::Base16EightiesDark)
    }
}

impl SyntaxHighlighter {
    fn new(theme_name: EmbeddedThemeName) -> Self {
        Self { theme_name }
    }

    fn highlight_file_lines(&self, file_path: &Path, lines: &[String]) -> Option<HighlightedLines> {
        let syntax = self.get_syntax(file_path).or_else(|| {
            lines
                .first()
                .and_then(|line| syntax_set().find_syntax_by_first_line(line))
        })?;
        let mut highlighter = HighlightLines::new(syntax, theme_set().get(self.theme_name));

        Some(Self::collect_line_highlights(lines, |line| {
            highlighter
                .highlight_line(line, syntax_set())
                .ok()
                .map(|ranges| {
                    ranges
                        .into_iter()
                        .map(|(style, text)| (to_ratatui_style(style), text.to_string()))
                        .collect()
                })
        }))
    }

    fn collect_line_highlights<F>(lines: &[String], mut highlight_line: F) -> HighlightedLines
    where
        F: FnMut(&str) -> Option<StyledSegments>,
    {
        let mut result = Vec::with_capacity(lines.len());
        for line in lines {
            result.push(highlight_line(line));
        }
        result
    }

    fn get_syntax(&self, file_path: &Path) -> Option<&'static SyntaxReference> {
        if let Some(ext) = file_path.extension().and_then(|ext| ext.to_str())
            && let Some(syntax) = find_syntax_by_extension(ext)
        {
            return Some(syntax);
        }

        if let Some(filename) = file_path.file_name().and_then(|name| name.to_str()) {
            if let Some(syntax) = syntax_set().find_syntax_by_token(filename) {
                return Some(syntax);
            }

            if let Some(syntax) = syntax_set().find_syntax_by_name(filename) {
                return Some(syntax);
            }

            if let Some(fallback) = lookup_fallback(filename, FILENAME_FALLBACKS)
                && let Some(syntax) = syntax_set().find_syntax_by_extension(fallback)
            {
                return Some(syntax);
            }
        }

        None
    }
}

fn syntax_set() -> &'static SyntaxSet {
    static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
    SYNTAX_SET.get_or_init(two_face::syntax::extra_newlines)
}

fn theme_set() -> &'static EmbeddedLazyThemeSet {
    static THEME_SET: OnceLock<EmbeddedLazyThemeSet> = OnceLock::new();
    THEME_SET.get_or_init(two_face::theme::extra)
}

fn default_segments(line: &str) -> StyledSegments {
    vec![(Style::default(), line.to_string())]
}

fn find_syntax_by_extension(ext: &str) -> Option<&'static SyntaxReference> {
    if let Some(syntax) = syntax_set().find_syntax_by_extension(ext) {
        return Some(syntax);
    }

    let normalized = ext.to_ascii_lowercase();
    if normalized != ext
        && let Some(syntax) = syntax_set().find_syntax_by_extension(&normalized)
    {
        return Some(syntax);
    }

    lookup_fallback(&normalized, EXTENSION_FALLBACKS)
        .and_then(|fallback| syntax_set().find_syntax_by_extension(fallback))
}

fn lookup_fallback<'a>(value: &str, table: &'a [(&'a str, &'static str)]) -> Option<&'static str> {
    table
        .iter()
        .find_map(|(candidate, fallback)| (*candidate == value).then_some(*fallback))
}

fn to_ratatui_style(style: syntect::highlighting::Style) -> Style {
    let mut ratatui_style = Style::default().fg(Color::Rgb(
        style.foreground.r,
        style.foreground.g,
        style.foreground.b,
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
    use std::path::Path;

    use super::{SyntaxHighlighter, highlight_file};
    use two_face::theme::EmbeddedThemeName;

    #[test]
    fn supported_sources_receive_syntax_colors() {
        for (path, line) in [
            ("src/main.rs", "fn main() { println!(\"hi\"); }"),
            ("Cargo.toml", "version = \"0.1.0\""),
            ("tool.py", "def main():"),
            ("widget.ts", "export const x: number = 1;"),
        ] {
            let highlighted = highlight_file(path, &[line.to_string()]);
            assert_eq!(highlighted.len(), 1);
            assert!(
                highlighted[0].iter().any(|(style, _)| style.fg.is_some()),
                "expected syntax color for {path}"
            );
        }
    }

    #[test]
    fn syntax_detection_handles_case_tokens_and_fallbacks() {
        let highlighter = SyntaxHighlighter::default();
        for path in [
            "SRC/MAIN.RS",
            "BUILD",
            "file.ts",
            "file.tsx",
            "file.mts",
            "file.cts",
            "file.mjs",
            "file.mustache",
            "file.mdx",
            "file.json5",
            "file.heex",
            "Containerfile",
            "justfile",
        ] {
            assert!(
                highlighter.get_syntax(Path::new(path)).is_some(),
                "should resolve syntax for {path}"
            );
        }
    }

    #[test]
    fn should_detect_syntax_from_shebang_when_extensionless() {
        let highlighter = SyntaxHighlighter::default();
        let lines = vec![
            "#!/usr/bin/env python".to_string(),
            "print('hello')".to_string(),
        ];
        let highlighted = highlighter.highlight_file_lines(Path::new("script"), &lines);
        assert!(highlighted.is_some());
        assert_eq!(highlighted.unwrap().len(), lines.len());
    }

    #[test]
    fn toml_highlights_under_gruvbox_dark_theme() {
        let highlighter = SyntaxHighlighter::new(EmbeddedThemeName::GruvboxDark);
        let lines = vec!["version = \"0.1.0\"".to_string()];
        let highlighted = highlighter
            .highlight_file_lines(Path::new("Cargo.toml"), &lines)
            .expect("toml syntax should resolve");
        let spans = highlighted[0]
            .as_ref()
            .expect("line should highlight under gruvbox dark");
        assert!(spans.iter().any(|(style, _)| style.fg.is_some()));
    }
}
