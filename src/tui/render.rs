use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::model::{Note, NoteKind, NoteSource, Question};
use crate::theme::{self, Theme};

use super::app::{
    App, DraftKind, DraftTarget, FilePickerState, FocusPane, InputMode, PromptDraft, ViewMetrics,
};

#[derive(Clone, Copy)]
struct SourceLineState {
    digits: usize,
    selected: bool,
    has_note: bool,
    has_question: bool,
    in_visual_selection: bool,
}

fn theme() -> Theme {
    theme::active()
}

pub fn render(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    app.composer_cursor_screen_pos = None;
    frame.render_widget(
        Block::default().style(Style::default().bg(theme().bg)),
        area,
    );

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
        InputMode::Draft => render_draft(frame, app, sections[1]),
        InputMode::DraftConfirm => {
            render_draft(frame, app, sections[1]);
            render_draft_confirm(frame, sections[1]);
        }
        InputMode::FilePicker => render_file_picker(frame, app, sections[1]),
        InputMode::Search => render_search(frame, app, sections[1]),
        InputMode::Normal | InputMode::Visual => {}
    }

    if let Some((x, y)) = app.composer_cursor_screen_pos {
        frame.set_cursor_position(Position { x, y });
    }
}

fn render_header(frame: &mut Frame, app: &App, area: Rect) {
    let file = app.current_file();
    let header = Line::from(vec![
        Span::styled(
            " COPANION ",
            Style::default().fg(theme().bg).bg(theme().accent),
        ),
        Span::raw("  "),
        Span::styled(
            &app.packet.title,
            Style::default()
                .fg(theme().text)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            format!(
                "{} files  {} notes  {} open questions",
                app.files.len(),
                app.packet.notes.len(),
                app.packet.open_questions().count()
            ),
            Style::default().fg(theme().muted),
        ),
        Span::raw("  "),
        Span::styled(file.path.as_str(), Style::default().fg(theme().accent)),
    ]);

    frame.render_widget(
        Paragraph::new(header).style(Style::default().bg(theme().bg)),
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
        .style(Style::default().bg(theme().panel).fg(theme().text))
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
            let bg = if selected {
                theme().cursor_line
            } else {
                theme().panel
            };
            let style = if selected {
                Style::default()
                    .fg(theme().text)
                    .bg(bg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme().text).bg(bg)
            };
            Line::from(vec![
                Span::styled(
                    if selected { "▸ " } else { "  " },
                    Style::default().fg(theme().accent).bg(bg),
                ),
                Span::styled(
                    truncate_from_start(&file.path, inner.width.saturating_sub(12) as usize),
                    style,
                ),
                Span::styled(" ", Style::default().bg(bg)),
                Span::styled(
                    format!("{note_count}n"),
                    Style::default().fg(theme().note_border).bg(bg),
                ),
                Span::styled(" ", Style::default().bg(bg)),
                Span::styled(
                    format!("{question_count}q"),
                    Style::default().fg(theme().question_border).bg(bg),
                ),
            ])
        })
        .collect();

    frame.render_widget(
        Paragraph::new(lines)
            .style(Style::default().bg(theme().panel))
            .wrap(Wrap { trim: false }),
        inner,
    );
}

fn render_source_view(frame: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == FocusPane::Source
        && matches!(app.input_mode, InputMode::Normal | InputMode::Visual);
    let title = if let Some(anchor) = app.visual_selection() {
        format!(" {} [visual {}] ", app.current_path(), anchor)
    } else {
        format!(" {} ", app.current_path())
    };
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .style(Style::default().bg(theme().panel).fg(theme().text))
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
            .style(Style::default().bg(theme().panel))
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
            "Tab focus  j/k move  [] jump  v select  dd delete  a question  n note  i edit  f add file  / search"
        }
        InputMode::Visual => "j/k move  a question  n note  Esc cancel  v finish selection",
        InputMode::Draft => "Type the draft  Ctrl-S save  Ctrl-O edit in $EDITOR  Esc close",
        InputMode::DraftConfirm => "Save this draft before closing? y yes  n no  Esc back",
        InputMode::FilePicker => "Type to fuzzy-search files  Enter add  j/k move  Esc cancel",
        InputMode::Search => "Type to fuzzy-search notes  Enter jump  j/k move  Esc cancel",
        InputMode::Help => "q or Esc closes help",
    };
    let message = app.message.as_deref().unwrap_or(hints);
    let selection = app
        .visual_selection()
        .map(|anchor| format!("  visual {}", anchor))
        .unwrap_or_default();
    let line = Line::from(vec![
        Span::styled(
            format!(" {focus} "),
            Style::default().fg(theme().bg).bg(theme().accent),
        ),
        Span::raw(" "),
        Span::styled(
            format!(
                "line {}{}  {}  {} attached notes",
                app.cursor_line, selection, dirty, current_notes
            ),
            Style::default().fg(theme().muted),
        ),
        Span::raw("  "),
        Span::styled(message, Style::default().fg(theme().text)),
    ]);
    frame.render_widget(
        Paragraph::new(line).style(Style::default().bg(theme().bg)),
        area,
    );
}

fn render_help(frame: &mut Frame, area: Rect) {
    let popup = centered_rect(area, 74, 68);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title(" Copanion Help ")
        .borders(Borders::ALL)
        .style(Style::default().bg(theme().panel).fg(theme().text))
        .border_style(Style::default().fg(theme().border_focus));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let help = vec![
        Line::from(vec![Span::styled(
            "The viewer keeps code primary and injects note cards directly below their anchors.",
            Style::default().fg(theme().text),
        )]),
        Line::default(),
        help_line("Tab", "toggle focus between file list and source"),
        help_line("j / k", "move through the selected pane"),
        help_line("h / l", "switch files from the source pane"),
        help_line("[ / ]", "jump to the previous or next annotated line"),
        help_line(
            "v / V",
            "start a visual selection for a ranged note or question",
        ),
        help_line("a", "open a QUESTION draft at the cursor or selected range"),
        help_line("n", "open a NOTE draft at the cursor or selected range"),
        help_line(
            "i",
            "edit the question under the cursor, or fall back to the note",
        ),
        help_line("Esc", "leave visual mode or close the current popup"),
        help_line(
            "I",
            "edit the note under the cursor, or fall back to the question",
        ),
        help_line("f", "open the fuzzy file picker and add a tracked file"),
        help_line("/", "fuzzy-search notes and questions, then jump"),
        help_line(
            "dd",
            "delete the selected file or the annotation under the cursor",
        ),
        help_line("Ctrl-O", "open the current draft in $VISUAL or $EDITOR"),
        help_line("s", "save the session to disk"),
        help_line("y", "copy the open-question export without quitting"),
        help_line("x", "save, export open questions, and quit"),
        help_line("q", "quit; press twice if there are unsaved changes"),
        help_line("?", "toggle this help"),
    ];

    frame.render_widget(
        Paragraph::new(help)
            .style(Style::default().bg(theme().panel))
            .wrap(Wrap { trim: false }),
        inner,
    );
}

fn render_draft(frame: &mut Frame, app: &mut App, area: Rect) {
    let popup = centered_rect(area, 76, 72);
    frame.render_widget(Clear, popup);
    let draft = app.draft.as_ref().expect("draft must exist in draft mode");
    let (title, border_color) = match draft.kind {
        DraftKind::Question => (" Question Draft ", theme().question_border),
        DraftKind::Note => (" Note Draft ", theme().note_border),
    };
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .style(Style::default().bg(theme().panel).fg(theme().text))
        .border_style(Style::default().fg(border_color));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(5),
            Constraint::Length(2),
        ])
        .split(inner);

    let summary = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("Mode: ", Style::default().fg(theme().muted)),
            Span::styled(draft_mode_label(draft), Style::default().fg(theme().accent)),
            Span::raw("  "),
            Span::styled("File: ", Style::default().fg(theme().muted)),
            Span::styled(draft.path.as_str(), Style::default().fg(theme().text)),
        ]),
        Line::from(vec![
            Span::styled("Anchor: ", Style::default().fg(theme().muted)),
            Span::styled(draft.anchor.to_string(), Style::default().fg(theme().text)),
            Span::raw("  "),
            Span::styled("Linked notes: ", Style::default().fg(theme().muted)),
            Span::styled(
                if draft.related_note_ids.is_empty() {
                    "none".to_string()
                } else {
                    draft.related_note_ids.join(", ")
                },
                Style::default().fg(theme().text),
            ),
        ]),
    ])
    .style(Style::default().bg(theme().panel));
    frame.render_widget(summary, sections[0]);

    render_text_area(
        frame,
        sections[1],
        match draft.kind {
            DraftKind::Question => " Question ",
            DraftKind::Note => " Note ",
        },
        &draft.buffer.text,
        true,
        border_color,
    );

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Ctrl-S", Style::default().fg(theme().success)),
            Span::raw(" save  "),
            Span::styled("Ctrl-O", Style::default().fg(theme().accent)),
            Span::raw(" external editor  "),
            Span::styled("Esc", Style::default().fg(theme().danger)),
            Span::raw(" close"),
        ]))
        .style(Style::default().bg(theme().panel)),
        sections[2],
    );

    let (cursor_x, cursor_y) = text_cursor_position(&draft.buffer, sections[1]);
    app.composer_cursor_screen_pos = Some((cursor_x, cursor_y));
}

fn render_draft_confirm(frame: &mut Frame, area: Rect) {
    let popup = centered_rect(area, 44, 18);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title(" Close Draft ")
        .borders(Borders::ALL)
        .style(Style::default().bg(theme().panel).fg(theme().text))
        .border_style(Style::default().fg(theme().danger));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let lines = vec![
        Line::from(Span::styled(
            "Save the current draft before closing?",
            Style::default()
                .fg(theme().text)
                .add_modifier(Modifier::BOLD),
        )),
        Line::default(),
        Line::from(vec![
            Span::styled("y", Style::default().fg(theme().success)),
            Span::raw(" save it  "),
            Span::styled("n", Style::default().fg(theme().danger)),
            Span::raw(" discard it  "),
            Span::styled("Esc", Style::default().fg(theme().accent)),
            Span::raw(" keep editing"),
        ]),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .style(Style::default().bg(theme().panel))
            .wrap(Wrap { trim: false }),
        inner,
    );
}

fn render_file_picker(frame: &mut Frame, app: &mut App, area: Rect) {
    let popup = centered_rect(area, 74, 72);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title(" Add Tracked File ")
        .borders(Borders::ALL)
        .style(Style::default().bg(theme().panel).fg(theme().text))
        .border_style(Style::default().fg(theme().accent));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let picker = app
        .file_picker
        .as_ref()
        .expect("file picker must exist in file picker mode");
    render_picker_contents(frame, inner, picker, "Search workspace files");
    let (cursor_x, cursor_y) = text_cursor_position(&picker.query, inner);
    app.composer_cursor_screen_pos = Some((cursor_x, cursor_y));
}

fn render_search(frame: &mut Frame, app: &mut App, area: Rect) {
    let popup = centered_rect(area, 74, 72);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title(" Search Notes ")
        .borders(Borders::ALL)
        .style(Style::default().bg(theme().panel).fg(theme().text))
        .border_style(Style::default().fg(theme().border_focus));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let search = app
        .search
        .as_ref()
        .expect("search state must exist in search mode");
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(inner);

    render_text_area(
        frame,
        sections[0],
        " Search ",
        &search.query.text,
        true,
        theme().border_focus,
    );

    let items = search
        .matches
        .iter()
        .take(sections[1].height as usize)
        .enumerate()
        .map(|(index, candidate)| {
            let selected = index == search.selected;
            let bg = if selected {
                theme().cursor_line
            } else {
                theme().panel
            };
            let style = if selected {
                Style::default()
                    .bg(bg)
                    .fg(theme().text)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().bg(bg).fg(theme().text)
            };
            Line::from(vec![
                Span::styled(
                    if selected { "▸ " } else { "  " },
                    Style::default().fg(theme().accent).bg(bg),
                ),
                Span::styled(
                    format!("{}:{} ", candidate.path, candidate.line),
                    Style::default().fg(theme().accent).bg(bg),
                ),
                Span::styled(candidate.label.clone(), style),
                Span::styled(" ", Style::default().bg(bg)),
                Span::styled(
                    truncate_from_start(
                        &candidate.preview,
                        sections[1].width.saturating_sub(24) as usize,
                    ),
                    Style::default().fg(theme().muted).bg(bg),
                ),
            ])
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(items)
            .style(Style::default().bg(theme().panel))
            .wrap(Wrap { trim: false }),
        sections[1],
    );
    let (cursor_x, cursor_y) = text_cursor_position(&search.query, sections[0]);
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
        .style(Style::default().bg(theme().panel).fg(theme().text))
        .border_style(if focused {
            Style::default().fg(accent)
        } else {
            Style::default().fg(theme().border)
        });
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let lines = if text.is_empty() {
        vec![Line::from(Span::styled(
            "Type here...",
            Style::default().fg(theme().muted),
        ))]
    } else {
        text.lines()
            .map(|line| Line::from(Span::raw(line.to_string())))
            .collect()
    };
    frame.render_widget(
        Paragraph::new(lines)
            .style(Style::default().bg(theme().panel).fg(theme().text))
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
                    .fg(theme().danger)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                error.clone(),
                Style::default().fg(theme().muted),
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
            .filter(|note| super::app::anchor_display_line(note.anchor) == line_no)
            .collect::<Vec<_>>();
        let source_questions = questions
            .iter()
            .copied()
            .filter(|question| {
                question.anchor.map(super::app::anchor_display_line) == Some(line_no)
            })
            .collect::<Vec<_>>();
        let line_has_note = notes
            .iter()
            .copied()
            .any(|note| super::app::anchor_contains_line(note.anchor, line_no));
        let line_has_question = questions.iter().copied().any(|question| {
            question
                .anchor
                .map(|anchor| super::app::anchor_contains_line(anchor, line_no))
                .unwrap_or(false)
        });
        let in_visual_selection = app.is_line_in_visual_selection(line_no);

        line_to_row.push(rendered.len());
        if !source_notes.is_empty() || !source_questions.is_empty() {
            annotation_lines.push(line_no);
        }
        let highlighted = file.highlighted_lines.get(line_no.saturating_sub(1));
        rendered.push(render_source_line(
            line_no,
            &content,
            highlighted,
            SourceLineState {
                digits,
                selected: line_no == app.cursor_line,
                has_note: line_has_note,
                has_question: line_has_question,
                in_visual_selection,
            },
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
        Style::default().fg(theme().muted),
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
    highlighted: Option<&crate::syntax::StyledSegments>,
    state: SourceLineState,
) -> Line<'static> {
    let mut prefix = " ".to_string();
    if state.has_note {
        prefix = "●".to_string();
    }
    if state.has_question {
        prefix = "◌".to_string();
    }
    if state.has_note && state.has_question {
        prefix = "◆".to_string();
    }

    let line_bg = match (state.selected, state.in_visual_selection) {
        (true, true) => blend_color(theme().panel, theme().border_focus, 28),
        (true, false) => theme().cursor_line,
        (false, true) => blend_color(theme().panel, theme().border_focus, 18),
        (false, false) => theme().panel,
    };
    let prefix_fg = if state.in_visual_selection {
        theme().border_focus
    } else {
        theme().accent
    };

    let mut spans = vec![
        Span::styled(prefix, Style::default().fg(prefix_fg).bg(line_bg)),
        Span::styled(" ", Style::default().bg(line_bg)),
        Span::styled(
            format!("{line_no:>digits$}", digits = state.digits),
            Style::default().fg(theme().muted).bg(line_bg),
        ),
        Span::styled(" │ ", Style::default().fg(theme().border).bg(line_bg)),
    ];

    if let Some(segments) = highlighted {
        if segments.is_empty() {
            spans.push(Span::styled(
                content.to_string(),
                Style::default().fg(theme().text).bg(line_bg),
            ));
        } else {
            spans.extend(segments.iter().map(|(style, text)| {
                let mut patched = *style;
                patched = patched.bg(line_bg);
                Span::styled(text.clone(), patched)
            }));
        }
    } else {
        spans.push(Span::styled(
            content.to_string(),
            Style::default().fg(theme().text).bg(line_bg),
        ));
    }

    Line::from(spans)
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
        theme().note_border,
        theme().note_bg,
    );
    lines.insert(
        0,
        Line::from(vec![
            Span::styled("  ", Style::default().bg(theme().panel)),
            Span::styled(
                "╭─ ",
                Style::default().fg(theme().note_border).bg(theme().note_bg),
            ),
            Span::styled(
                note.title.clone(),
                Style::default()
                    .fg(theme().text)
                    .bg(theme().note_bg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  {title}"),
                Style::default().fg(theme().muted).bg(theme().note_bg),
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
    let mut lines = render_card(
        "Question",
        &title,
        &question.prompt,
        question.related_note_ids.as_slice(),
        width,
        theme().question_border,
        theme().question_bg,
    );
    lines.insert(
        0,
        Line::from(vec![
            Span::styled("  ", Style::default().bg(theme().panel)),
            Span::styled(
                "╭─ ",
                Style::default()
                    .fg(theme().question_border)
                    .bg(theme().question_bg),
            ),
            Span::styled(
                "Open Question",
                Style::default()
                    .fg(theme().text)
                    .bg(theme().question_bg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  {title}"),
                Style::default().fg(theme().muted).bg(theme().question_bg),
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
            Span::styled("  ", Style::default().bg(theme().panel)),
            Span::styled("│ ", Style::default().fg(border_color).bg(background)),
            Span::styled(line, Style::default().fg(theme().text).bg(background)),
        ]));
    }
    if !tags.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("  ", Style::default().bg(theme().panel)),
            Span::styled("│ ", Style::default().fg(border_color).bg(background)),
            Span::styled(
                format!("Linked: {}", tags.join(", ")),
                Style::default().fg(theme().muted).bg(background),
            ),
        ]));
    }
    lines.push(Line::from(vec![
        Span::styled("  ", Style::default().bg(theme().panel)),
        Span::styled("╰─ ", Style::default().fg(border_color).bg(background)),
        Span::styled(
            format!("{label} · {subtitle}"),
            Style::default().fg(theme().muted).bg(background),
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

fn blend_color(base: Color, accent: Color, accent_percent: u8) -> Color {
    debug_assert!(accent_percent <= 100);
    match (base, accent) {
        (Color::Rgb(br, bg, bb), Color::Rgb(ar, ag, ab)) => {
            let p = u16::from(accent_percent);
            let inv = 100_u16.saturating_sub(p);
            let mix = |base_component: u8, accent_component: u8| -> u8 {
                ((u16::from(base_component) * inv + u16::from(accent_component) * p) / 100) as u8
            };
            Color::Rgb(mix(br, ar), mix(bg, ag), mix(bb, ab))
        }
        _ => accent,
    }
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
                .fg(theme().accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(description.to_string(), Style::default().fg(theme().text)),
    ])
}

fn render_picker_contents(frame: &mut Frame, area: Rect, picker: &FilePickerState, title: &str) {
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);
    render_text_area(
        frame,
        sections[0],
        " Search ",
        &picker.query.text,
        true,
        theme().accent,
    );

    let items = picker
        .matches
        .iter()
        .take(sections[1].height as usize)
        .enumerate()
        .map(|(index, path)| {
            let selected = index == picker.selected;
            let bg = if selected {
                theme().cursor_line
            } else {
                theme().panel
            };
            let style = if selected {
                Style::default()
                    .bg(bg)
                    .fg(theme().text)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme().text).bg(bg)
            };
            Line::from(vec![
                Span::styled(
                    if selected { "▸ " } else { "  " },
                    Style::default().fg(theme().accent).bg(bg),
                ),
                Span::styled(path.clone(), style),
            ])
        })
        .collect::<Vec<_>>();

    frame.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            title.to_string(),
            Style::default().fg(theme().muted).bg(theme().panel),
        )]))
        .style(Style::default().bg(theme().panel)),
        Rect {
            x: sections[1].x,
            y: sections[1].y.saturating_sub(1),
            width: sections[1].width,
            height: 1,
        },
    );
    frame.render_widget(
        Paragraph::new(items)
            .style(Style::default().bg(theme().panel))
            .wrap(Wrap { trim: false }),
        sections[1],
    );
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

fn text_cursor_position(buffer: &super::app::TextBuffer, area: Rect) -> (u16, u16) {
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
        Style::default().fg(theme().border_focus)
    } else {
        Style::default().fg(theme().border)
    }
}

fn draft_mode_label(draft: &PromptDraft) -> String {
    let target = match draft.target {
        DraftTarget::New => "new",
        DraftTarget::EditNote { .. } | DraftTarget::EditQuestion { .. } => "edit",
    };
    match draft.kind {
        DraftKind::Question => format!("{target} question"),
        DraftKind::Note => format!("{target} note"),
    }
}

#[cfg(test)]
mod tests {
    use crate::model::{Anchor, Note, NoteKind, NoteSource, Packet, Question, TrackedFile};

    use super::{build_source_lines, wrap_text};
    use crate::tui::app::App;

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

    #[test]
    fn ranged_annotations_render_at_their_end_line() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("src")).unwrap();
        std::fs::write(root.path().join("src/main.rs"), "one\ntwo\nthree\n").unwrap();
        let mut packet = Packet::new(
            "demo",
            "Demo",
            root.path().display().to_string(),
            vec![TrackedFile::new("src/main.rs")],
        );
        packet.notes.push(Note::new(
            "src/main.rs",
            Anchor::new(1, Some(3)),
            NoteKind::Flow,
            "range note",
            "covers the first block",
            vec![],
            None,
            NoteSource::Agent,
        ));
        let app = App::load(root.path().join("packet.toml"), packet, false).unwrap();
        let (_, metrics) = build_source_lines(&app, 80);
        assert_eq!(metrics.annotation_lines, vec![3]);
    }
}
