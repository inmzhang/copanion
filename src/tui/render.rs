use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::model::{Note, NoteKind, NoteSource, Question};

use super::app::{App, ComposerField, FocusPane, InputMode, ViewMetrics};

#[derive(Clone, Copy)]
struct Theme {
    bg: Color,
    panel: Color,
    border: Color,
    border_focus: Color,
    text: Color,
    muted: Color,
    accent: Color,
    note_bg: Color,
    note_border: Color,
    question_bg: Color,
    question_border: Color,
    cursor_line: Color,
    danger: Color,
    success: Color,
}

const THEME: Theme = Theme {
    bg: Color::Rgb(14, 20, 28),
    panel: Color::Rgb(18, 28, 38),
    border: Color::Rgb(62, 86, 108),
    border_focus: Color::Rgb(129, 194, 255),
    text: Color::Rgb(228, 237, 245),
    muted: Color::Rgb(138, 159, 178),
    accent: Color::Rgb(102, 214, 201),
    note_bg: Color::Rgb(22, 45, 58),
    note_border: Color::Rgb(102, 214, 201),
    question_bg: Color::Rgb(59, 42, 21),
    question_border: Color::Rgb(236, 194, 83),
    cursor_line: Color::Rgb(39, 55, 71),
    danger: Color::Rgb(234, 106, 108),
    success: Color::Rgb(144, 203, 104),
};

pub fn render(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    app.composer_cursor_screen_pos = None;
    frame.render_widget(Block::default().style(Style::default().bg(THEME.bg)), area);

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);

    render_header(frame, app, sections[0]);
    render_main(frame, app, sections[1]);
    render_status(frame, app, sections[2]);

    match app.input_mode {
        InputMode::Help => render_help(frame, sections[1]),
        InputMode::QuestionComposer => render_composer(frame, app, sections[1]),
        InputMode::Normal => {}
    }

    if let Some((x, y)) = app.composer_cursor_screen_pos {
        frame.set_cursor_position(Position { x, y });
    }
}

fn render_header(frame: &mut Frame, app: &App, area: Rect) {
    let file = app.current_file();
    let header = Line::from(vec![
        Span::styled(" COPANION ", Style::default().fg(THEME.bg).bg(THEME.accent)),
        Span::raw("  "),
        Span::styled(
            &app.packet.title,
            Style::default().fg(THEME.text).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            format!(
                "{} files  {} notes  {} open questions",
                app.files.len(),
                app.packet.notes.len(),
                app.packet.open_questions().count()
            ),
            Style::default().fg(THEME.muted),
        ),
        Span::raw("  "),
        Span::styled(file.path.as_str(), Style::default().fg(THEME.accent)),
    ]);

    frame.render_widget(
        Paragraph::new(header).style(Style::default().bg(THEME.bg)),
        area,
    );
}

fn render_main(frame: &mut Frame, app: &mut App, area: Rect) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(30), Constraint::Min(0)])
        .split(area);

    render_file_list(frame, app, columns[0]);
    render_source_view(frame, app, columns[1]);
}

fn render_file_list(frame: &mut Frame, app: &App, area: Rect) {
    let focused = app.focus == FocusPane::Files && app.input_mode == InputMode::Normal;
    let block = Block::default()
        .title(" Files ")
        .borders(Borders::ALL)
        .style(Style::default().bg(THEME.panel).fg(THEME.text))
        .border_style(border_style(focused));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let lines: Vec<Line> = app
        .files
        .iter()
        .enumerate()
        .map(|(index, file)| {
            let selected = index == app.current_file;
            let note_count = app.file_note_count(&file.path);
            let question_count = app.file_open_question_count(&file.path);
            let style = if selected {
                Style::default()
                    .fg(THEME.text)
                    .bg(THEME.cursor_line)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(THEME.text)
            };
            let mut spans = vec![
                Span::styled(
                    if selected { "▸ " } else { "  " },
                    Style::default().fg(THEME.accent),
                ),
                Span::styled(
                    truncate_from_start(&file.path, inner.width.saturating_sub(12) as usize),
                    style,
                ),
            ];
            spans.push(Span::raw(" "));
            spans.push(Span::styled(
                format!("{note_count}n"),
                Style::default().fg(THEME.note_border),
            ));
            spans.push(Span::raw(" "));
            spans.push(Span::styled(
                format!("{question_count}q"),
                Style::default().fg(THEME.question_border),
            ));
            Line::from(spans)
        })
        .collect();

    frame.render_widget(
        Paragraph::new(lines)
            .style(Style::default().bg(THEME.panel))
            .wrap(Wrap { trim: false }),
        inner,
    );
}

fn render_source_view(frame: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == FocusPane::Source && app.input_mode == InputMode::Normal;
    let block = Block::default()
        .title(format!(" {} ", app.current_path()))
        .borders(Borders::ALL)
        .style(Style::default().bg(THEME.panel).fg(THEME.text))
        .border_style(border_style(focused));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let width = inner.width.max(12) as usize;
    let (lines, metrics) = build_source_lines(app, width);
    let total_rows = metrics.total_rows;
    app.update_view_metrics(ViewMetrics {
        viewport_height: inner.height as usize,
        ..metrics
    });
    if app.scroll > total_rows.saturating_sub(inner.height as usize) {
        app.scroll = total_rows.saturating_sub(inner.height as usize);
    }
    let visible = lines
        .into_iter()
        .skip(app.scroll)
        .take(inner.height as usize)
        .collect::<Vec<_>>();

    frame.render_widget(
        Paragraph::new(visible)
            .style(Style::default().bg(THEME.panel))
            .wrap(Wrap { trim: false }),
        inner,
    );
}

fn render_status(frame: &mut Frame, app: &App, area: Rect) {
    let focus = match app.focus {
        FocusPane::Files => "files",
        FocusPane::Source => "source",
    };
    let dirty = if app.dirty { "unsaved" } else { "saved" };
    let current_notes = app.notes_for_current_line().len();
    let hints = match app.input_mode {
        InputMode::Normal => {
            "Tab focus  j/k move  [/] annotations  a ask  s save  y export  x save+quit  ? help"
        }
        InputMode::QuestionComposer => {
            "Type your question  Tab switch field  Ctrl-S save question  Esc cancel"
        }
        InputMode::Help => "q or Esc closes help",
    };
    let message = app.message.as_deref().unwrap_or(hints);
    let line = Line::from(vec![
        Span::styled(
            format!(" {focus} "),
            Style::default().fg(THEME.bg).bg(THEME.accent),
        ),
        Span::raw(" "),
        Span::styled(
            format!(
                "line {}  {}  {} attached notes",
                app.cursor_line, dirty, current_notes
            ),
            Style::default().fg(THEME.muted),
        ),
        Span::raw("  "),
        Span::styled(message, Style::default().fg(THEME.text)),
    ]);
    frame.render_widget(
        Paragraph::new(line).style(Style::default().bg(THEME.bg)),
        area,
    );
}

fn render_help(frame: &mut Frame, area: Rect) {
    let popup = centered_rect(area, 70, 60);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title(" Copanion Help ")
        .borders(Borders::ALL)
        .style(Style::default().bg(THEME.panel).fg(THEME.text))
        .border_style(Style::default().fg(THEME.border_focus));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let help = vec![
        Line::from(vec![Span::styled(
            "The viewer keeps code primary and injects note cards directly below their anchors.",
            Style::default().fg(THEME.text),
        )]),
        Line::default(),
        help_line("Tab", "toggle focus between file list and source"),
        help_line("j / k", "move through the selected pane"),
        help_line("h / l", "switch files from the source pane"),
        help_line("[ / ]", "jump to the previous or next annotated line"),
        help_line("a", "open the question composer at the current source line"),
        help_line("r", "reload tracked source files from disk"),
        help_line("s", "save the packet to disk"),
        help_line("y", "copy the open-question export without quitting"),
        help_line("x", "save, export open questions, and quit"),
        help_line("q", "quit; press twice if there are unsaved changes"),
        help_line("?", "toggle this help"),
        Line::default(),
        help_line(
            "Question composer",
            "Ctrl-S saves the question, Tab switches between prompt and why",
        ),
    ];

    frame.render_widget(
        Paragraph::new(help)
            .style(Style::default().bg(THEME.panel))
            .wrap(Wrap { trim: false }),
        inner,
    );
}

fn render_composer(frame: &mut Frame, app: &mut App, area: Rect) {
    let popup = centered_rect(area, 76, 72);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title(" Ask Follow-up Question ")
        .borders(Borders::ALL)
        .style(Style::default().bg(THEME.panel).fg(THEME.text))
        .border_style(Style::default().fg(THEME.question_border));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let composer = app
        .composer
        .as_ref()
        .expect("composer must exist in composer mode");
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(5),
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(2),
        ])
        .split(inner);

    let summary = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("File: ", Style::default().fg(THEME.muted)),
            Span::styled(composer.path.as_str(), Style::default().fg(THEME.accent)),
            Span::raw("  "),
            Span::styled("Anchor: ", Style::default().fg(THEME.muted)),
            Span::styled(composer.anchor.to_string(), Style::default().fg(THEME.text)),
        ]),
        Line::from(vec![
            Span::styled("Related notes: ", Style::default().fg(THEME.muted)),
            Span::styled(
                if composer.related_note_ids.is_empty() {
                    "none".to_string()
                } else {
                    composer.related_note_ids.join(", ")
                },
                Style::default().fg(THEME.text),
            ),
        ]),
    ])
    .style(Style::default().bg(THEME.panel));
    frame.render_widget(summary, sections[0]);

    let prompt_focused = composer.field == ComposerField::Prompt;
    render_text_area(
        frame,
        sections[1],
        " Prompt ",
        &composer.prompt.text,
        prompt_focused,
        THEME.question_border,
    );

    frame.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            "Why this is unclear",
            Style::default().fg(THEME.muted),
        )]))
        .style(Style::default().bg(THEME.panel)),
        sections[2],
    );

    let why_focused = composer.field == ComposerField::Why;
    render_text_area(
        frame,
        sections[3],
        " Why ",
        &composer.why.text,
        why_focused,
        THEME.border_focus,
    );

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Ctrl-S", Style::default().fg(THEME.success)),
            Span::raw(" save question  "),
            Span::styled("Esc", Style::default().fg(THEME.danger)),
            Span::raw(" cancel  "),
            Span::styled("Tab", Style::default().fg(THEME.accent)),
            Span::raw(" switch field"),
        ]))
        .style(Style::default().bg(THEME.panel)),
        sections[4],
    );

    let (cursor_x, cursor_y) = cursor_position_for_composer(composer, sections[1], sections[3]);
    app.composer_cursor_screen_pos = Some((cursor_x, cursor_y));
}

fn render_text_area(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    text: &str,
    focused: bool,
    accent: Color,
) {
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .style(Style::default().bg(THEME.panel).fg(THEME.text))
        .border_style(if focused {
            Style::default().fg(accent)
        } else {
            Style::default().fg(THEME.border)
        });
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let lines = if text.is_empty() {
        vec![Line::from(Span::styled(
            "Type here...",
            Style::default().fg(THEME.muted),
        ))]
    } else {
        text.lines()
            .map(|line| Line::from(Span::raw(line.to_string())))
            .collect()
    };
    frame.render_widget(
        Paragraph::new(lines)
            .style(Style::default().bg(THEME.panel).fg(THEME.text))
            .wrap(Wrap { trim: false }),
        inner,
    );
}

fn build_source_lines(app: &App, width: usize) -> (Vec<Line<'static>>, ViewMetrics) {
    let file = app.current_file();
    if let Some(error) = &file.load_error {
        let lines = vec![
            Line::from(Span::styled(
                format!("unable to read {}", file.path),
                Style::default()
                    .fg(THEME.danger)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                error.clone(),
                Style::default().fg(THEME.muted),
            )),
        ];
        return (
            lines,
            ViewMetrics {
                line_to_row: vec![0],
                annotation_lines: Vec::new(),
                total_rows: 2,
                viewport_height: 1,
            },
        );
    }

    let notes = app.notes_for_path(&file.path);
    let questions = app.questions_for_path(&file.path);
    let digits = file.lines.len().max(1).to_string().len();
    let mut rendered = Vec::new();
    let mut line_to_row = Vec::new();
    let mut annotation_lines = Vec::new();
    let source_line_count = file.lines.len().max(1);

    for line_no in 1..=source_line_count {
        let content = file
            .lines
            .get(line_no.saturating_sub(1))
            .cloned()
            .unwrap_or_default();
        let source_notes = notes
            .iter()
            .copied()
            .filter(|note| note.anchor.start_line == line_no)
            .collect::<Vec<_>>();
        let source_questions = questions
            .iter()
            .copied()
            .filter(|question| question.anchor.map(|anchor| anchor.start_line) == Some(line_no))
            .collect::<Vec<_>>();

        line_to_row.push(rendered.len());
        if !source_notes.is_empty() || !source_questions.is_empty() {
            annotation_lines.push(line_no);
        }
        rendered.push(render_source_line(
            line_no,
            &content,
            digits,
            line_no == app.cursor_line,
            !source_notes.is_empty(),
            !source_questions.is_empty(),
        ));

        for note in source_notes {
            rendered.extend(render_note_card(note, width));
        }
        for question in source_questions {
            rendered.extend(render_question_card(question, width));
        }
    }

    rendered.push(Line::from(vec![Span::styled(
        format!(
            "─ end of {} ─ {} annotations attached",
            file.path,
            annotation_lines.len()
        ),
        Style::default().fg(THEME.muted),
    )]));

    let total_rows = rendered.len();
    (
        rendered,
        ViewMetrics {
            line_to_row,
            annotation_lines,
            total_rows,
            viewport_height: 1,
        },
    )
}

fn render_source_line(
    line_no: usize,
    content: &str,
    digits: usize,
    selected: bool,
    has_note: bool,
    has_question: bool,
) -> Line<'static> {
    let mut prefix = " ".to_string();
    if has_note {
        prefix = "●".to_string();
    }
    if has_question {
        prefix = "◌".to_string();
    }
    if has_note && has_question {
        prefix = "◆".to_string();
    }

    let style = if selected {
        Style::default().bg(THEME.cursor_line).fg(THEME.text)
    } else {
        Style::default().fg(THEME.text)
    };

    Line::from(vec![
        Span::styled(prefix, Style::default().fg(THEME.accent)),
        Span::raw(" "),
        Span::styled(
            format!("{line_no:>digits$}", digits = digits),
            Style::default().fg(THEME.muted),
        ),
        Span::styled(" │ ", Style::default().fg(THEME.border)),
        Span::styled(content.to_string(), style),
    ])
}

fn render_note_card(note: &Note, width: usize) -> Vec<Line<'static>> {
    let title = format!(
        "{} note · {} · lines {}",
        note_kind_label(note.kind),
        note_source_label(note.source),
        note.anchor
    );
    let mut lines = render_card(
        note.title.as_str(),
        &title,
        &note.body,
        note.tags.as_slice(),
        width,
        THEME.note_border,
        THEME.note_bg,
    );
    lines.insert(
        0,
        Line::from(vec![
            Span::styled("  ", Style::default().bg(THEME.panel)),
            Span::styled(
                "╭─ ",
                Style::default().fg(THEME.note_border).bg(THEME.note_bg),
            ),
            Span::styled(
                note.title.clone(),
                Style::default()
                    .fg(THEME.text)
                    .bg(THEME.note_bg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  {title}"),
                Style::default().fg(THEME.muted).bg(THEME.note_bg),
            ),
        ]),
    );
    lines
}

fn render_question_card(question: &Question, width: usize) -> Vec<Line<'static>> {
    let title = question
        .anchor
        .map(|anchor| format!("open question · line {anchor}"))
        .unwrap_or_else(|| "open question".to_string());
    let body = match &question.why {
        Some(why) => format!("{}\n\nWhy unclear: {why}", question.prompt),
        None => question.prompt.clone(),
    };
    let mut lines = render_card(
        "Question",
        &title,
        &body,
        question.related_note_ids.as_slice(),
        width,
        THEME.question_border,
        THEME.question_bg,
    );
    lines.insert(
        0,
        Line::from(vec![
            Span::styled("  ", Style::default().bg(THEME.panel)),
            Span::styled(
                "╭─ ",
                Style::default()
                    .fg(THEME.question_border)
                    .bg(THEME.question_bg),
            ),
            Span::styled(
                "Open Question",
                Style::default()
                    .fg(THEME.text)
                    .bg(THEME.question_bg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  {title}"),
                Style::default().fg(THEME.muted).bg(THEME.question_bg),
            ),
        ]),
    );
    lines
}

fn render_card(
    label: &str,
    subtitle: &str,
    body: &str,
    tags: &[String],
    width: usize,
    border_color: Color,
    background: Color,
) -> Vec<Line<'static>> {
    let inner_width = width.saturating_sub(8).max(18);
    let mut lines = Vec::new();
    let wrapped = wrap_text(body, inner_width);
    for line in wrapped {
        lines.push(Line::from(vec![
            Span::styled("  ", Style::default().bg(THEME.panel)),
            Span::styled("│ ", Style::default().fg(border_color).bg(background)),
            Span::styled(line, Style::default().fg(THEME.text).bg(background)),
        ]));
    }
    if !tags.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("  ", Style::default().bg(THEME.panel)),
            Span::styled("│ ", Style::default().fg(border_color).bg(background)),
            Span::styled(
                format!("Linked: {}", tags.join(", ")),
                Style::default().fg(THEME.muted).bg(background),
            ),
        ]));
    }
    lines.push(Line::from(vec![
        Span::styled("  ", Style::default().bg(THEME.panel)),
        Span::styled("╰─ ", Style::default().fg(border_color).bg(background)),
        Span::styled(
            format!("{label} · {subtitle}"),
            Style::default().fg(THEME.muted).bg(background),
        ),
    ]));
    lines
}

fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return vec![String::new()];
    }

    let mut wrapped = Vec::new();
    for paragraph in text.lines() {
        if paragraph.trim().is_empty() {
            wrapped.push(String::new());
            continue;
        }
        let mut current = String::new();
        for word in paragraph.split_whitespace() {
            let pending = if current.is_empty() {
                word.to_string()
            } else {
                format!("{current} {word}")
            };
            if UnicodeWidthStr::width(pending.as_str()) > max_width && !current.is_empty() {
                wrapped.push(current);
                current = word.to_string();
            } else if UnicodeWidthStr::width(word) > max_width {
                if !current.is_empty() {
                    wrapped.push(current);
                    current = String::new();
                }
                for chunk in hard_wrap_word(word, max_width) {
                    wrapped.push(chunk);
                }
            } else {
                current = pending;
            }
        }
        if !current.is_empty() {
            wrapped.push(current);
        }
    }
    if wrapped.is_empty() {
        wrapped.push(String::new());
    }
    wrapped
}

fn hard_wrap_word(word: &str, max_width: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    for ch in word.chars() {
        let pending = format!("{current}{ch}");
        if UnicodeWidthStr::width(pending.as_str()) > max_width && !current.is_empty() {
            chunks.push(current);
            current = ch.to_string();
        } else {
            current.push(ch);
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

fn note_kind_label(kind: NoteKind) -> &'static str {
    match kind {
        NoteKind::Overview => "overview",
        NoteKind::Flow => "flow",
        NoteKind::Pitfall => "pitfall",
        NoteKind::Reference => "reference",
    }
}

fn note_source_label(source: NoteSource) -> &'static str {
    match source {
        NoteSource::Agent => "agent",
        NoteSource::Human => "human",
        NoteSource::Imported => "imported",
    }
}

fn help_line(key: &str, description: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{key:<16}"),
            Style::default()
                .fg(THEME.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(description.to_string(), Style::default().fg(THEME.text)),
    ])
}

fn centered_rect(area: Rect, width_percent: u16, height_percent: u16) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - height_percent) / 2),
            Constraint::Percentage(height_percent),
            Constraint::Percentage((100 - height_percent) / 2),
        ])
        .split(area);
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - width_percent) / 2),
            Constraint::Percentage(width_percent),
            Constraint::Percentage((100 - width_percent) / 2),
        ])
        .split(vertical[1]);
    horizontal[1]
}

fn cursor_position_for_composer(
    composer: &super::app::QuestionComposer,
    prompt_area: Rect,
    why_area: Rect,
) -> (u16, u16) {
    let (buffer, area) = match composer.field {
        ComposerField::Prompt => (&composer.prompt, prompt_area),
        ComposerField::Why => (&composer.why, why_area),
    };
    let inner = Block::default().borders(Borders::ALL).inner(area);
    let (line_index, column_index) = line_col_at(&buffer.text, buffer.cursor);
    let x = inner.x.saturating_add(column_index as u16);
    let y = inner.y.saturating_add(line_index as u16);
    (
        x.min(inner.x + inner.width.saturating_sub(1)),
        y.min(inner.y + inner.height.saturating_sub(1)),
    )
}

fn line_col_at(text: &str, cursor: usize) -> (usize, usize) {
    let mut line = 0;
    let mut column = 0;
    for (idx, ch) in text.char_indices() {
        if idx >= cursor {
            break;
        }
        if ch == '\n' {
            line += 1;
            column = 0;
        } else {
            column += ch.width().unwrap_or(1);
        }
    }
    (line, column)
}

fn truncate_from_start(path: &str, max_width: usize) -> String {
    if max_width == 0 || path.chars().count() <= max_width {
        return path.to_string();
    }
    let tail_len = max_width.saturating_sub(3);
    format!(
        "...{}",
        path.chars()
            .rev()
            .take(tail_len)
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>()
    )
}

fn border_style(focused: bool) -> Style {
    if focused {
        Style::default().fg(THEME.border_focus)
    } else {
        Style::default().fg(THEME.border)
    }
}

#[cfg(test)]
mod tests {
    use crate::model::{Anchor, Note, NoteKind, NoteSource, Packet, Question, TrackedFile};

    use super::{App, build_source_lines, wrap_text};

    #[test]
    fn wrapped_text_preserves_blank_paragraphs() {
        let wrapped = wrap_text("one two\n\nthree", 5);
        assert!(wrapped.contains(&String::new()));
    }

    #[test]
    fn source_view_injects_note_and_question_cards() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("src")).unwrap();
        std::fs::write(root.path().join("src/main.rs"), "fn main() {}\n").unwrap();
        let mut packet = Packet::new(
            "demo",
            "Demo",
            root.path().display().to_string(),
            vec![TrackedFile::new("src/main.rs")],
        );
        packet.notes.push(Note::new(
            "src/main.rs",
            Anchor::new(1, None),
            NoteKind::Flow,
            "entry",
            "starts here",
            vec![],
            None,
            NoteSource::Agent,
        ));
        packet.questions.push(Question::new(
            "src/main.rs",
            Some(Anchor::new(1, None)),
            "why empty?",
            None,
            vec![],
        ));
        let app = App::load(root.path().join("packet.toml"), packet, false).unwrap();
        let (lines, metrics) = build_source_lines(&app, 80);
        let rendered = lines
            .iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("entry"));
        assert!(rendered.contains("why empty?"));
        assert_eq!(metrics.annotation_lines, vec![1]);
    }
}
