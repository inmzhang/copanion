use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::diff::{DiffFile, DiffLine, LineOrigin, calculate_gap};
use crate::model::{
    Note, NoteKind, NoteSource, Question, QuestionStatus, QuestionTurnKind, QuestionTurnRef,
};
use crate::theme::{self, Theme};

use super::app::{
    App, DiffRow, DiffViewMetrics, DraftKind, DraftTarget, FilePickerState, FocusPane, InputMode,
    PromptDraft, ViewMetrics,
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
        InputMode::ThreadView => render_thread_view(frame, app, sections[1]),
        InputMode::FilePicker => render_file_picker(frame, app, sections[1]),
        InputMode::Search => render_search(frame, app, sections[1]),
        InputMode::Normal | InputMode::Visual | InputMode::CommitSelect => {}
    }

    if let Some((x, y)) = app.composer_cursor_screen_pos {
        frame.set_cursor_position(Position { x, y });
    }
}

fn render_header(frame: &mut Frame, app: &App, area: Rect) {
    if app.is_diff_mode() {
        let current_path = app.current_diff_path().unwrap_or("Select a diff range");
        let selection = match app.current_diff_selection() {
            Some(crate::diff::DiffSelection::WorkingTree) => "working tree".to_string(),
            Some(crate::diff::DiffSelection::CommitRange(commit_ids)) => {
                format!(
                    "{} commit{}",
                    commit_ids.len(),
                    if commit_ids.len() == 1 { "" } else { "s" }
                )
            }
            Some(crate::diff::DiffSelection::WorkingTreeAndCommits(commit_ids)) => format!(
                "working tree + {} commit{}",
                commit_ids.len(),
                if commit_ids.len() == 1 { "" } else { "s" }
            ),
            None => "choose commits".to_string(),
        };
        let header = Line::from(vec![
            Span::styled(
                " COPANION DIFF ",
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
            Span::styled(selection, Style::default().fg(theme().muted)),
            Span::raw("  "),
            Span::styled(current_path, Style::default().fg(theme().accent)),
        ]);
        frame.render_widget(
            Paragraph::new(header).style(Style::default().bg(theme().bg)),
            area,
        );
        return;
    }

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
    if app.is_diff_mode() {
        if app.input_mode == InputMode::CommitSelect {
            render_commit_selector(frame, app, area);
        } else {
            render_diff_browser(frame, app, area);
        }
        return;
    }

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

fn render_commit_selector(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(" Diff Selection ")
        .borders(Borders::ALL)
        .style(Style::default().bg(theme().panel).fg(theme().text))
        .border_style(Style::default().fg(theme().border_focus));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let Some(browser) = app.diff_browser.as_ref() else {
        return;
    };

    let items = browser
        .commit_options
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let selected = index == browser.commit_cursor;
            let in_range = browser
                .commit_selection_range
                .is_some_and(|(start, end)| index >= start && index <= end);
            let bg = if selected {
                theme().cursor_line
            } else {
                theme().panel
            };
            let marker = if in_range { "●" } else { "○" };
            let prefix = if selected { "▸ " } else { "  " };
            let details = if entry.is_working_tree() {
                entry.summary.clone()
            } else if let Some(branch) = entry.branch_name.as_deref() {
                format!("{}  {}  ({branch})", entry.short_id, entry.summary)
            } else {
                format!("{}  {}", entry.short_id, entry.summary)
            };

            Line::from(vec![
                Span::styled(prefix, Style::default().fg(theme().accent).bg(bg)),
                Span::styled(
                    marker,
                    Style::default()
                        .fg(if in_range {
                            theme().success
                        } else {
                            theme().muted
                        })
                        .bg(bg),
                ),
                Span::styled(" ", Style::default().bg(bg)),
                Span::styled(details, Style::default().fg(theme().text).bg(bg)),
                Span::styled(
                    if entry.author.is_empty() {
                        String::new()
                    } else {
                        format!("  {}", entry.author)
                    },
                    Style::default().fg(theme().muted).bg(bg),
                ),
            ])
        })
        .collect::<Vec<_>>();

    frame.render_widget(
        Paragraph::new(items).style(Style::default().bg(theme().panel)),
        inner,
    );
}

fn render_diff_browser(frame: &mut Frame, app: &mut App, area: Rect) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(32), Constraint::Min(0)])
        .split(area);
    render_diff_file_list(frame, app, columns[0]);
    render_diff_view(frame, app, columns[1]);
}

fn render_diff_file_list(frame: &mut Frame, app: &App, area: Rect) {
    let focused = app.focus == FocusPane::Files && app.input_mode == InputMode::Normal;
    let block = Block::default()
        .title(" Changed Files ")
        .borders(Borders::ALL)
        .style(Style::default().bg(theme().panel).fg(theme().text))
        .border_style(border_style(focused));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let Some(browser) = app.diff_browser.as_ref() else {
        return;
    };

    let lines = browser
        .files
        .iter()
        .enumerate()
        .map(|(index, file)| {
            let selected = index == browser.current_file;
            let bg = if selected {
                theme().cursor_line
            } else {
                theme().panel
            };
            let review_mark = if app.is_current_diff_file_reviewed(file.display_path()) {
                "✓ "
            } else {
                ""
            };
            let note_count = app.file_note_count(file.display_path());
            let question_count = app.file_open_question_count(file.display_path());
            Line::from(vec![
                Span::styled(
                    if selected { "▸ " } else { "  " },
                    Style::default().fg(theme().accent).bg(bg),
                ),
                Span::styled(
                    file.status.as_char().to_string(),
                    Style::default()
                        .fg(match file.status.as_char() {
                            'A' | 'C' => theme().success,
                            'D' => theme().danger,
                            _ => theme().accent,
                        })
                        .bg(bg),
                ),
                Span::styled(" ", Style::default().bg(bg)),
                Span::styled(review_mark, Style::default().fg(theme().success).bg(bg)),
                Span::styled(
                    truncate_from_start(
                        file.display_path(),
                        inner.width.saturating_sub(16) as usize,
                    ),
                    Style::default()
                        .fg(theme().text)
                        .bg(bg)
                        .add_modifier(if selected {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                ),
                Span::styled(
                    format!("  {note_count}n {question_count}c"),
                    Style::default().fg(theme().muted).bg(bg),
                ),
            ])
        })
        .collect::<Vec<_>>();

    frame.render_widget(
        Paragraph::new(lines).style(Style::default().bg(theme().panel)),
        inner,
    );
}

fn render_diff_view(frame: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == FocusPane::Source
        && matches!(app.input_mode, InputMode::Normal | InputMode::Visual);
    let title = if let Some(anchor) = app.visual_selection() {
        app.current_diff_path()
            .map(|path| format!(" {path} [visual {anchor}] "))
            .unwrap_or_else(|| format!(" Diff [visual {anchor}] "))
    } else {
        app.current_diff_path()
            .map(|path| format!(" {path} "))
            .unwrap_or_else(|| " Diff ".to_string())
    };
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .style(Style::default().bg(theme().panel).fg(theme().text))
        .border_style(border_style(focused));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let (lines, metrics) = build_diff_lines(app, inner.width.max(16) as usize);
    app.update_diff_view_metrics(DiffViewMetrics {
        viewport_height: inner.height as usize,
        ..metrics
    });
    let scroll = app
        .diff_browser
        .as_ref()
        .map(|browser| browser.scroll)
        .unwrap_or(0);
    let visible = lines
        .into_iter()
        .skip(scroll)
        .take(inner.height as usize)
        .collect::<Vec<_>>();

    frame.render_widget(
        Paragraph::new(visible).style(Style::default().bg(theme().panel)),
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

fn render_diff_status(frame: &mut Frame, app: &App, area: Rect) {
    let focus = status_mode_label(app);
    let file_count = app.diff_files().len();
    let dirty = if app.dirty { "unsaved" } else { "saved" };
    let reviewed_count = app
        .diff_browser
        .as_ref()
        .map(|browser| browser.reviewed_paths.len())
        .unwrap_or(0);
    let current_path = app.current_diff_path().unwrap_or("no diff");
    let cursor_row = app
        .diff_browser
        .as_ref()
        .map(|browser| browser.cursor_row.saturating_add(1))
        .unwrap_or(0);
    let hints = match app.input_mode {
        InputMode::CommitSelect => "j/k move  Space toggle range  Enter open diff  q quit",
        InputMode::Help => "q or Esc closes help",
        InputMode::Normal if app.current_open_question().is_some() => {
            "Tab focus  j/k move  [] jump  v select  Enter/Space view thread  dd delete  a comment or reply  c close  n note  i edit  / search  r review+next  Esc/m commits"
        }
        InputMode::Normal
            if app
                .current_question()
                .is_some_and(|question| question.status != QuestionStatus::Open) =>
        {
            "Tab focus  j/k move  [] jump  v select  Enter/Space view thread  dd delete  a comment or reply  o reopen  n note  i edit  / search  r review+next  Esc/m commits"
        }
        InputMode::Normal => {
            "Tab focus  j/k move  [] jump  v select  Enter/Space context  dd delete  a comment or reply  n note  i edit  / search  r review+next  Esc/m commits"
        }
        InputMode::Visual => "j/k move  a comment  n note  Esc cancel  v finish selection",
        InputMode::Draft => "Type the draft  Ctrl-S save  Ctrl-O edit in $EDITOR  Esc close",
        InputMode::DraftConfirm => "Save this draft before closing? y yes  n no  Esc back",
        InputMode::ThreadView => {
            "j/k scroll  Ctrl-U/Ctrl-D half-page  PageUp/PageDown page  Enter/Esc close"
        }
        InputMode::FilePicker => "Type to fuzzy-search files  Enter add  j/k move  Esc cancel",
        InputMode::Search => {
            "Type to fuzzy-search review notes and comment threads  Enter jump  j/k move  Esc cancel"
        }
    };
    let message = app.message.as_deref().unwrap_or(hints);
    let badge_text = format!(" {focus} ");
    let selection = app
        .visual_selection()
        .map(|anchor| format!("  visual {}", anchor))
        .unwrap_or_default();
    let meta_text = format!(
        "row {}{}  {}  {}/{} reviewed  {}",
        cursor_row, selection, dirty, reviewed_count, file_count, current_path
    );
    let status = fit_status_segments(&badge_text, &meta_text, message, area.width as usize);
    let line = Line::from(vec![
        Span::styled(
            status.badge,
            Style::default().fg(theme().bg).bg(theme().accent),
        ),
        Span::raw(status.between_badge_and_meta),
        Span::styled(status.meta, Style::default().fg(theme().muted)),
        Span::raw(status.between_meta_and_message),
        Span::styled(status.message, Style::default().fg(theme().text)),
    ]);
    frame.render_widget(
        Paragraph::new(line).style(Style::default().bg(theme().bg)),
        area,
    );
}

fn render_status(frame: &mut Frame, app: &App, area: Rect) {
    if app.is_diff_mode() {
        render_diff_status(frame, app, area);
        return;
    }

    let focus = status_mode_label(app);
    let dirty = if app.dirty { "unsaved" } else { "saved" };
    let current_notes = app.notes_for_current_line().len();
    let current_thread = app
        .current_question()
        .map(|question| format!("  thread {}", question_status_label(question.status)))
        .unwrap_or_default();
    let hints = match app.input_mode {
        InputMode::Normal if app.current_open_question().is_some() => {
            "Tab focus  j/k move  [] jump  Enter/Space view  v select  dd delete  a ask or continue  c close  R reload  n note  i edit  f add file  / search"
        }
        InputMode::Normal
            if app
                .current_question()
                .is_some_and(|question| question.status != QuestionStatus::Open) =>
        {
            "Tab focus  j/k move  [] jump  Enter/Space view  v select  dd delete  a ask or continue  o reopen  R reload  n note  i edit  f add file  / search"
        }
        InputMode::Normal => {
            "Tab focus  j/k move  [] jump  Enter/Space view  v select  dd delete  a ask or continue  R reload  n note  i edit  f add file  / search"
        }
        InputMode::Visual => "j/k move  a question  n note  Esc cancel  v finish selection",
        InputMode::Draft => "Type the draft  Ctrl-S save  Ctrl-O edit in $EDITOR  Esc close",
        InputMode::DraftConfirm => "Save this draft before closing? y yes  n no  Esc back",
        InputMode::ThreadView => {
            "j/k scroll  Ctrl-U/Ctrl-D half-page  PageUp/PageDown page  Enter/Esc close"
        }
        InputMode::FilePicker => "Type to fuzzy-search files  Enter add  j/k move  Esc cancel",
        InputMode::Search => {
            "Type to fuzzy-search notes and questions  Enter jump  j/k move  Esc cancel"
        }
        InputMode::Help => "q or Esc closes help",
        InputMode::CommitSelect => "j/k move  Space toggle range  Enter open diff  q quit",
    };
    let message = app.message.as_deref().unwrap_or(hints);
    let selection = app
        .visual_selection()
        .map(|anchor| format!("  visual {}", anchor))
        .unwrap_or_default();
    let badge_text = format!(" {focus} ");
    let meta_text = format!(
        "line {}{}  {}  {} attached notes{}",
        app.cursor_line, selection, dirty, current_notes, current_thread
    );
    let status = fit_status_segments(&badge_text, &meta_text, message, area.width as usize);
    let line = Line::from(vec![
        Span::styled(
            status.badge,
            Style::default().fg(theme().bg).bg(theme().accent),
        ),
        Span::raw(status.between_badge_and_meta),
        Span::styled(status.meta, Style::default().fg(theme().muted)),
        Span::raw(status.between_meta_and_message),
        Span::styled(status.message, Style::default().fg(theme().text)),
    ]);
    frame.render_widget(
        Paragraph::new(line).style(Style::default().bg(theme().bg)),
        area,
    );
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct StatusSegments {
    badge: String,
    between_badge_and_meta: String,
    meta: String,
    between_meta_and_message: String,
    message: String,
}

fn fit_status_segments(
    badge: &str,
    meta: &str,
    message: &str,
    total_width: usize,
) -> StatusSegments {
    let badge = truncate_to_width_end(badge, total_width);
    let badge_width = UnicodeWidthStr::width(badge.as_str());
    let remaining_after_badge = total_width.saturating_sub(badge_width);

    if remaining_after_badge == 0 {
        return StatusSegments {
            badge,
            between_badge_and_meta: String::new(),
            meta: String::new(),
            between_meta_and_message: String::new(),
            message: String::new(),
        };
    }

    let gap_after_badge = " ";
    let gap_after_badge_width = UnicodeWidthStr::width(gap_after_badge);
    let remaining = remaining_after_badge.saturating_sub(gap_after_badge_width);

    if remaining == 0 {
        return StatusSegments {
            badge,
            between_badge_and_meta: String::new(),
            meta: String::new(),
            between_meta_and_message: String::new(),
            message: String::new(),
        };
    }

    let wants_message = !message.is_empty();
    let separator_width = if wants_message { 2 } else { 0 };
    let min_message_width = if wants_message { remaining.min(12) } else { 0 };
    let meta_budget = if wants_message {
        remaining.saturating_sub(min_message_width + separator_width)
    } else {
        remaining
    };
    let meta = truncate_to_width_end(meta, meta_budget);
    let meta_width = UnicodeWidthStr::width(meta.as_str());

    let message_width = if wants_message {
        remaining
            .saturating_sub(meta_width)
            .saturating_sub(separator_width)
    } else {
        0
    };
    let (between_meta_and_message, message) = if message_width > 0 {
        (
            "  ".to_string(),
            truncate_to_width_end(message, message_width),
        )
    } else {
        (String::new(), String::new())
    };

    StatusSegments {
        badge,
        between_badge_and_meta: gap_after_badge.to_string(),
        meta,
        between_meta_and_message,
        message,
    }
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
            "Source mode shows tracked files with inline notes. Diff mode adds a commit selector and unified patch browser.",
            Style::default().fg(theme().text),
        )]),
        Line::default(),
        Line::from(Span::styled(
            "Shared keys",
            Style::default()
                .fg(theme().accent)
                .add_modifier(Modifier::BOLD),
        )),
        help_line("Tab", "toggle focus between the sidebar and the main view"),
        help_line("j / k", "move through the selected pane"),
        help_line("h / l", "switch files from the source pane"),
        help_line("[ / ]", "jump to the previous or next note or thread"),
        help_line("PageUp / PageDown", "move by one viewport"),
        help_line("Ctrl-U / Ctrl-D", "move by half a viewport"),
        help_line("v / V", "start a visual selection on anchorable lines"),
        help_line("a", "open or continue the thread under the cursor"),
        help_line(
            "Enter / Space",
            "open the thread under the cursor in a readonly viewer",
        ),
        help_line("c", "close the open thread under the cursor"),
        help_line("o", "reopen the closed thread under the cursor"),
        help_line("n", "open a NOTE draft at the cursor or selected range"),
        help_line(
            "i",
            "edit the thread under the cursor, or fall back to the note",
        ),
        help_line("Esc", "leave visual mode or close the current popup"),
        help_line(
            "I",
            "edit the note under the cursor, or fall back to the thread",
        ),
        help_line("/", "fuzzy-search notes and threads, then jump"),
        help_line(
            "dd",
            "delete the current annotation, or the tracked file in source mode",
        ),
        help_line("Ctrl-O", "open the current draft in $VISUAL or $EDITOR"),
        help_line("s", "save the packet to disk"),
        Line::default(),
        Line::from(Span::styled(
            "Source mode",
            Style::default()
                .fg(theme().accent)
                .add_modifier(Modifier::BOLD),
        )),
        help_line("f", "open the fuzzy file picker and add a tracked file"),
        help_line("R", "reload tracked file contents from disk"),
        help_line("y", "copy the open-question export without quitting"),
        help_line("x", "save, export open questions, and quit"),
        Line::default(),
        Line::from(Span::styled(
            "Diff mode",
            Style::default()
                .fg(theme().accent)
                .add_modifier(Modifier::BOLD),
        )),
        help_line(
            "--diff",
            "start in diff mode instead of the tracked-source browser",
        ),
        help_line(
            "Space",
            "toggle a contiguous commit range, or expand/collapse hidden diff context",
        ),
        help_line(
            "Enter",
            "open the selected diff range, or expand/collapse hidden context",
        ),
        help_line("Esc", "in diff mode, return to the commit selector"),
        help_line("m", "in diff mode, reopen the commit selector"),
        help_line("r", "mark the current diff file reviewed and jump next"),
        help_line(
            "y",
            "copy a diff-review export with selected commits and your comments",
        ),
        help_line("x", "save, export the diff review, and quit"),
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
        DraftKind::Question if app.is_diff_mode() => (" Comment Draft ", theme().question_border),
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
            Span::styled(
                draft_mode_label(draft, app.is_diff_mode()),
                Style::default().fg(theme().accent),
            ),
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
            DraftKind::Question if app.is_diff_mode() => " Comment ",
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

fn render_thread_view(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(question) = app.active_thread_view_question() else {
        return;
    };
    let popup = centered_rect(area, 84, 82);
    frame.render_widget(Clear, popup);
    let (border_color, _) = question_status_style(question.status);
    let block = Block::default()
        .title(if app.is_diff_mode() {
            " Comment Thread "
        } else {
            " Question Thread "
        })
        .borders(Borders::ALL)
        .style(Style::default().bg(theme().panel).fg(theme().text))
        .border_style(Style::default().fg(border_color));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(inner);

    let anchor = question
        .anchor
        .map(|anchor| anchor.to_string())
        .unwrap_or_else(|| "none".to_string());
    let summary = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("File: ", Style::default().fg(theme().muted)),
            Span::styled(question.path.as_str(), Style::default().fg(theme().text)),
            Span::raw("  "),
            Span::styled("Anchor: ", Style::default().fg(theme().muted)),
            Span::styled(anchor, Style::default().fg(theme().text)),
        ]),
        Line::from(vec![
            Span::styled("Status: ", Style::default().fg(theme().muted)),
            Span::styled(
                question_status_label(question.status),
                Style::default().fg(theme().accent),
            ),
            Span::raw("  "),
            Span::styled("Turns: ", Style::default().fg(theme().muted)),
            Span::styled(
                question.turn_count().to_string(),
                Style::default().fg(theme().text),
            ),
        ]),
    ])
    .style(Style::default().bg(theme().panel));
    frame.render_widget(summary, sections[0]);

    let lines = render_question_thread(
        question,
        sections[1].width.max(20) as usize,
        app.is_diff_mode(),
        false,
    );
    app.update_thread_view_metrics(lines.len(), sections[1].height as usize);
    let visible = lines
        .into_iter()
        .skip(app.thread_view_scroll())
        .take(sections[1].height as usize)
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(visible).style(Style::default().bg(theme().panel)),
        sections[1],
    );

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("j/k", Style::default().fg(theme().accent)),
            Span::raw(" scroll  "),
            Span::styled("Ctrl-U/D", Style::default().fg(theme().accent)),
            Span::raw(" half-page  "),
            Span::styled("PgUp/PgDn", Style::default().fg(theme().accent)),
            Span::raw(" page  "),
            Span::styled("Enter/Esc", Style::default().fg(theme().danger)),
            Span::raw(" close"),
        ]))
        .style(Style::default().bg(theme().panel)),
        sections[2],
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
        .title(" Search Annotations ")
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
        " Search annotations ",
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
            rendered.extend(render_question_card(question, width, false));
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

fn build_diff_lines(app: &App, width: usize) -> (Vec<Line<'static>>, DiffViewMetrics) {
    let Some(browser) = app.diff_browser.as_ref() else {
        return (
            vec![Line::from(vec![Span::styled(
                "Diff mode is unavailable",
                Style::default().fg(theme().muted),
            )])],
            DiffViewMetrics {
                file_rows: vec![0],
                annotation_rows: Vec::new(),
                rows: vec![DiffRow::FileHeader],
                total_rows: 1,
                viewport_height: 1,
            },
        );
    };

    if browser.files.is_empty() {
        return (
            vec![Line::from(vec![Span::styled(
                "Select a commit range and press Enter to load a diff.",
                Style::default().fg(theme().muted),
            )])],
            DiffViewMetrics {
                file_rows: vec![0],
                annotation_rows: Vec::new(),
                rows: vec![DiffRow::FileHeader],
                total_rows: 1,
                viewport_height: 1,
            },
        );
    }

    let mut rendered = Vec::new();
    let mut file_rows = Vec::new();
    let mut annotation_rows = Vec::new();
    let mut rows = Vec::new();

    for (file_index, file) in browser.files.iter().enumerate() {
        let file_selected = file_index == browser.current_file;
        file_rows.push(rendered.len());
        rows.push(DiffRow::FileHeader);
        rendered.push(render_diff_file_header(
            app,
            file,
            file_selected,
            browser.cursor_row == rendered.len(),
            app.is_diff_row_in_visual_selection(rendered.len()),
        ));

        if file.is_binary {
            rows.push(DiffRow::FileEnd);
            rendered.push(Line::from(vec![Span::styled(
                "  binary file",
                Style::default().fg(theme().muted),
            )]));
            continue;
        }

        if file.is_too_large {
            rows.push(DiffRow::FileEnd);
            rendered.push(Line::from(vec![Span::styled(
                "  untracked file too large to render inline",
                Style::default().fg(theme().muted),
            )]));
            continue;
        }

        let digits = diff_line_digits(file);
        if file.hunks.is_empty() {
            rows.push(DiffRow::FileEnd);
            rendered.push(Line::from(vec![Span::styled(
                "  no textual hunks",
                Style::default().fg(theme().muted),
            )]));
        }

        for (hunk_index, hunk) in file.hunks.iter().enumerate() {
            let previous_hunk = hunk_index
                .checked_sub(1)
                .and_then(|index| file.hunks.get(index));
            let gap = calculate_gap(previous_hunk, hunk);
            if gap > 0 {
                let gap_id = super::app::GapId {
                    file_idx: file_index,
                    hunk_idx: hunk_index,
                };
                if browser.expanded_gaps.contains(&gap_id) {
                    if let Some(expanded_lines) = browser.expanded_content.get(&gap_id) {
                        for (context_idx, context_line) in expanded_lines.iter().enumerate() {
                            let row = rendered.len();
                            rows.push(DiffRow::ExpandedContext {
                                file_idx: file_index,
                                gap_id: gap_id.clone(),
                                context_idx,
                            });
                            rendered.push(render_diff_line(
                                context_line,
                                digits,
                                browser.cursor_row == row,
                                app.is_diff_row_in_visual_selection(row),
                            ));
                            append_diff_annotations(
                                app,
                                file,
                                context_line.new_lineno,
                                width,
                                &mut rendered,
                                &mut annotation_rows,
                                &mut rows,
                            );
                        }
                    }
                } else {
                    let row = rendered.len();
                    rows.push(DiffRow::GapExpander {
                        gap_id: gap_id.clone(),
                    });
                    rendered.push(render_diff_gap_line(
                        gap,
                        browser.cursor_row == row,
                        app.is_diff_row_in_visual_selection(row),
                    ));
                }
            }

            let hunk_row = rendered.len();
            rows.push(DiffRow::HunkHeader);
            rendered.push(render_diff_hunk_header(
                hunk,
                browser.cursor_row == hunk_row,
                app.is_diff_row_in_visual_selection(hunk_row),
            ));

            for (line_idx, diff_line) in hunk.lines.iter().enumerate() {
                let row = rendered.len();
                rows.push(DiffRow::DiffLine {
                    file_idx: file_index,
                    hunk_idx: hunk_index,
                    line_idx,
                });
                rendered.push(render_diff_line(
                    diff_line,
                    digits,
                    browser.cursor_row == row,
                    app.is_diff_row_in_visual_selection(row),
                ));
                append_diff_annotations(
                    app,
                    file,
                    diff_line.new_lineno,
                    width,
                    &mut rendered,
                    &mut annotation_rows,
                    &mut rows,
                );
            }
        }

        rows.push(DiffRow::FileEnd);
        rendered.push(Line::from(vec![Span::styled(
            format!("─ end of {} ─", file.display_path()),
            Style::default().fg(theme().muted),
        )]));
    }

    let total_rows = rendered.len().max(1);
    (
        rendered,
        DiffViewMetrics {
            file_rows,
            annotation_rows,
            rows,
            total_rows,
            viewport_height: 1,
        },
    )
}

fn append_diff_annotations(
    app: &App,
    file: &DiffFile,
    line_no: Option<usize>,
    width: usize,
    rendered: &mut Vec<Line<'static>>,
    annotation_rows: &mut Vec<usize>,
    rows: &mut Vec<DiffRow>,
) {
    let Some(line_no) = line_no else {
        return;
    };
    let Some(path) = file.new_path.as_deref() else {
        return;
    };
    let source_notes = app
        .notes_for_path(path)
        .into_iter()
        .filter(|note| super::app::anchor_display_line(note.anchor) == line_no)
        .collect::<Vec<_>>();
    let source_questions = app
        .questions_for_path(path)
        .into_iter()
        .filter(|question| question.anchor.map(super::app::anchor_display_line) == Some(line_no))
        .collect::<Vec<_>>();

    for note in source_notes {
        annotation_rows.push(rendered.len());
        for line in render_note_card(note, width) {
            rendered.push(line);
            rows.push(DiffRow::Annotation {
                path: path.to_string(),
                line_no,
            });
        }
    }
    for question in source_questions {
        annotation_rows.push(rendered.len());
        for line in render_question_card(question, width, true) {
            rendered.push(line);
            rows.push(DiffRow::Annotation {
                path: path.to_string(),
                line_no,
            });
        }
    }
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

fn render_diff_file_header(
    app: &App,
    file: &DiffFile,
    file_selected: bool,
    cursor_selected: bool,
    in_visual_selection: bool,
) -> Line<'static> {
    let bg = match (cursor_selected, in_visual_selection) {
        (true, true) => blend_color(theme().panel, theme().border_focus, 28),
        (true, false) => theme().cursor_line,
        (false, true) => blend_color(theme().panel, theme().border_focus, 18),
        (false, false) => theme().panel,
    };
    let label_style = Style::default()
        .fg(theme().text)
        .bg(bg)
        .add_modifier(if file_selected {
            Modifier::BOLD
        } else {
            Modifier::empty()
        });
    let note_count = app.file_note_count(file.display_path());
    let question_count = app.file_open_question_count(file.display_path());
    let review_mark = if app.is_current_diff_file_reviewed(file.display_path()) {
        "✓ "
    } else {
        ""
    };
    Line::from(vec![
        Span::styled(
            "▸ ",
            Style::default()
                .fg(if file_selected {
                    theme().accent
                } else {
                    theme().border
                })
                .bg(bg),
        ),
        Span::styled(
            file.status.as_char().to_string(),
            Style::default()
                .fg(match file.status.as_char() {
                    'A' | 'C' => theme().success,
                    'D' => theme().danger,
                    _ => theme().accent,
                })
                .bg(bg),
        ),
        Span::styled(" ", Style::default().bg(bg)),
        Span::styled(review_mark, Style::default().fg(theme().success).bg(bg)),
        Span::styled(file.display_path().to_string(), label_style),
        Span::styled(
            format!("  {note_count}n {question_count}c"),
            Style::default().fg(theme().muted).bg(bg),
        ),
    ])
}

fn render_diff_gap_line(gap: usize, selected: bool, in_visual_selection: bool) -> Line<'static> {
    let base_bg = blend_color(theme().panel, theme().border, 8);
    let bg = match (selected, in_visual_selection) {
        (true, true) => blend_color(base_bg, theme().border_focus, 28),
        (true, false) => blend_color(theme().panel, theme().border_focus, 16),
        (false, true) => blend_color(base_bg, theme().border_focus, 18),
        (false, false) => base_bg,
    };
    Line::from(vec![Span::styled(
        format!("       ... expand ({gap} lines) ..."),
        Style::default().fg(theme().muted).bg(bg),
    )])
}

fn render_diff_hunk_header(
    hunk: &crate::diff::DiffHunk,
    selected: bool,
    in_visual_selection: bool,
) -> Line<'static> {
    let base_bg = blend_color(theme().panel, theme().border, 10);
    let bg = match (selected, in_visual_selection) {
        (true, true) => blend_color(base_bg, theme().border_focus, 28),
        (true, false) => blend_color(theme().panel, theme().border_focus, 20),
        (false, true) => blend_color(base_bg, theme().border_focus, 18),
        (false, false) => base_bg,
    };
    Line::from(vec![Span::styled(
        format!("  {}", hunk.header),
        Style::default().fg(theme().muted).bg(bg),
    )])
}

fn render_diff_line(
    diff_line: &DiffLine,
    digits: usize,
    selected: bool,
    in_visual_selection: bool,
) -> Line<'static> {
    let base_bg = match diff_line.origin {
        LineOrigin::Context => theme().panel,
        LineOrigin::Addition => blend_color(theme().panel, theme().success, 14),
        LineOrigin::Deletion => blend_color(theme().panel, theme().danger, 14),
    };
    let bg = match (selected, in_visual_selection) {
        (true, true) => blend_color(base_bg, theme().border_focus, 30),
        (true, false) => blend_color(base_bg, theme().border_focus, 24),
        (false, true) => blend_color(base_bg, theme().border_focus, 16),
        (false, false) => base_bg,
    };
    let prefix_fg = match diff_line.origin {
        LineOrigin::Context => theme().muted,
        LineOrigin::Addition => theme().success,
        LineOrigin::Deletion => theme().danger,
    };
    let prefix = match diff_line.origin {
        LineOrigin::Context => ' ',
        LineOrigin::Addition => '+',
        LineOrigin::Deletion => '-',
    };
    let old_lineno = diff_line
        .old_lineno
        .map(|line| format!("{line:>digits$}", digits = digits))
        .unwrap_or_else(|| " ".repeat(digits));
    let new_lineno = diff_line
        .new_lineno
        .map(|line| format!("{line:>digits$}", digits = digits))
        .unwrap_or_else(|| " ".repeat(digits));

    let mut spans = vec![
        Span::styled(old_lineno, Style::default().fg(theme().muted).bg(bg)),
        Span::styled(" ", Style::default().bg(bg)),
        Span::styled(new_lineno, Style::default().fg(theme().muted).bg(bg)),
        Span::styled(" │ ", Style::default().fg(theme().border).bg(bg)),
        Span::styled(prefix.to_string(), Style::default().fg(prefix_fg).bg(bg)),
        Span::styled(" ", Style::default().bg(bg)),
    ];

    if diff_line.segments.is_empty() {
        spans.push(Span::styled(
            diff_line.content.clone(),
            Style::default().fg(theme().text).bg(bg),
        ));
    } else {
        spans.extend(diff_line.segments.iter().map(|(style, text)| {
            let mut patched = *style;
            patched = patched.bg(bg);
            Span::styled(text.clone(), patched)
        }));
    }

    Line::from(spans)
}

fn diff_line_digits(file: &DiffFile) -> usize {
    file.hunks
        .iter()
        .flat_map(|hunk| hunk.lines.iter())
        .flat_map(|line| [line.old_lineno, line.new_lineno])
        .flatten()
        .max()
        .unwrap_or(1)
        .to_string()
        .len()
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

fn render_question_card(
    question: &Question,
    width: usize,
    comment_mode: bool,
) -> Vec<Line<'static>> {
    render_question_thread(question, width, comment_mode, true)
}

fn render_question_thread(
    question: &Question,
    width: usize,
    comment_mode: bool,
    show_actions: bool,
) -> Vec<Line<'static>> {
    let (border_color, background) = question_status_style(question.status);
    let status = question_status_label(question.status);
    let turn_count = question.turn_count();
    let thread_kind = if comment_mode {
        "comment thread"
    } else {
        "question thread"
    };
    let title = question
        .anchor
        .map(|anchor| {
            format!(
                "{} {} · line {} · {} turn{}",
                status,
                thread_kind,
                anchor,
                turn_count,
                if turn_count == 1 { "" } else { "s" }
            )
        })
        .unwrap_or_else(|| format!("{status} {thread_kind}"));
    let mut lines = vec![Line::from(vec![
        Span::styled("  ", Style::default().bg(theme().panel)),
        Span::styled("╭─ ", Style::default().fg(border_color).bg(background)),
        Span::styled(
            question_status_header(question.status, comment_mode),
            Style::default()
                .fg(theme().text)
                .bg(background)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  {title}"),
            Style::default().fg(theme().muted).bg(background),
        ),
    ])];
    if let Some(why) = &question.why {
        lines.push(render_thread_meta_line(
            border_color,
            background,
            format!("Why unclear: {why}"),
        ));
    }
    if !question.related_note_ids.is_empty() {
        lines.push(render_thread_meta_line(
            border_color,
            background,
            format!("Linked notes: {}", question.related_note_ids.join(", ")),
        ));
    }
    for turn in question.turns() {
        lines.extend(render_question_turn_card(
            turn,
            question.status,
            width,
            comment_mode,
        ));
    }
    if show_actions && question.status == QuestionStatus::Open {
        lines.push(Line::from(vec![
            Span::styled("  ", Style::default().bg(theme().panel)),
            Span::styled(
                "↳ ",
                Style::default()
                    .fg(theme().question_border)
                    .bg(theme().panel),
            ),
            Span::styled(
                if comment_mode {
                    "Actions: a reply  c close thread"
                } else {
                    "Actions: a continue thread  c close thread"
                },
                Style::default().fg(theme().muted).bg(theme().panel),
            ),
        ]));
    } else if show_actions {
        lines.push(Line::from(vec![
            Span::styled("  ", Style::default().bg(theme().panel)),
            Span::styled(
                "↳ ",
                Style::default().fg(theme().note_border).bg(theme().panel),
            ),
            Span::styled(
                "Actions: o reopen thread",
                Style::default().fg(theme().muted).bg(theme().panel),
            ),
        ]));
    }
    lines
}

fn render_thread_meta_line(border_color: Color, background: Color, text: String) -> Line<'static> {
    Line::from(vec![
        Span::styled("  ", Style::default().bg(theme().panel)),
        Span::styled("│ ", Style::default().fg(border_color).bg(background)),
        Span::styled(text, Style::default().fg(theme().muted).bg(background)),
    ])
}

fn render_question_turn_card(
    turn: QuestionTurnRef<'_>,
    question_status: QuestionStatus,
    width: usize,
    comment_mode: bool,
) -> Vec<Line<'static>> {
    let (border_color, background) = question_turn_style(turn.kind, question_status);
    render_card(
        question_turn_label(turn.kind, comment_mode),
        question_turn_subtitle(turn.kind, comment_mode),
        turn.body,
        &[],
        width,
        border_color,
        background,
    )
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

fn question_status_style(status: QuestionStatus) -> (Color, Color) {
    match status {
        QuestionStatus::Open => (theme().question_border, theme().question_bg),
        QuestionStatus::Answered => (theme().note_border, theme().note_bg),
        QuestionStatus::Archived => (
            theme().border,
            blend_color(theme().panel, theme().border, 10),
        ),
    }
}

fn question_status_header(status: QuestionStatus, comment_mode: bool) -> &'static str {
    if comment_mode {
        return match status {
            QuestionStatus::Open => "Open Comment Thread",
            QuestionStatus::Answered => "Closed Comment Thread",
            QuestionStatus::Archived => "Archived Comment Thread",
        };
    }
    match status {
        QuestionStatus::Open => "Open Question",
        QuestionStatus::Answered => "Closed Conversation",
        QuestionStatus::Archived => "Archived Conversation",
    }
}

fn question_status_label(status: QuestionStatus) -> &'static str {
    match status {
        QuestionStatus::Open => "open",
        QuestionStatus::Answered => "closed",
        QuestionStatus::Archived => "archived",
    }
}

fn question_turn_style(kind: QuestionTurnKind, status: QuestionStatus) -> (Color, Color) {
    match kind {
        QuestionTurnKind::Prompt => question_status_style(status),
        QuestionTurnKind::UserFollowUp => (
            theme().border_focus,
            blend_color(theme().panel, theme().border_focus, 12),
        ),
        QuestionTurnKind::AgentReply => (
            theme().accent,
            blend_color(theme().panel, theme().accent, 12),
        ),
    }
}

fn question_turn_label(kind: QuestionTurnKind, comment_mode: bool) -> &'static str {
    match (kind, comment_mode) {
        (QuestionTurnKind::Prompt, true) => "Comment",
        (QuestionTurnKind::Prompt, false) => "Question",
        (QuestionTurnKind::UserFollowUp, true) => "Reply",
        (QuestionTurnKind::UserFollowUp, false) => "Follow-up",
        (QuestionTurnKind::AgentReply, _) => "Agent Reply",
    }
}

fn question_turn_subtitle(kind: QuestionTurnKind, comment_mode: bool) -> &'static str {
    match (kind, comment_mode) {
        (QuestionTurnKind::Prompt, true) => "original comment",
        (QuestionTurnKind::Prompt, false) => "original question",
        (QuestionTurnKind::UserFollowUp, true) => "comment reply",
        (QuestionTurnKind::UserFollowUp, false) => "user follow-up",
        (QuestionTurnKind::AgentReply, true) => "agent reply",
        (QuestionTurnKind::AgentReply, false) => "agent answer",
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

fn truncate_to_width_end(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if UnicodeWidthStr::width(text) <= max_width {
        return text.to_string();
    }
    if max_width == 1 {
        return "…".to_string();
    }

    let ellipsis = "…";
    let ellipsis_width = UnicodeWidthStr::width(ellipsis);
    let target_width = max_width.saturating_sub(ellipsis_width);
    let mut out = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + ch_width > target_width {
            break;
        }
        out.push(ch);
        used += ch_width;
    }
    out.push_str(ellipsis);
    out
}

fn border_style(focused: bool) -> Style {
    if focused {
        Style::default().fg(theme().border_focus)
    } else {
        Style::default().fg(theme().border)
    }
}

fn draft_mode_label(draft: &PromptDraft, diff_mode: bool) -> String {
    let target = match draft.target {
        DraftTarget::New => "new",
        DraftTarget::ContinueQuestion { .. } => "continue",
        DraftTarget::EditNote { .. }
        | DraftTarget::EditQuestionPrompt { .. }
        | DraftTarget::EditQuestionMessage { .. } => "edit",
    };
    match draft.kind {
        DraftKind::Question => format!(
            "{target} {}",
            if diff_mode { "comment" } else { "question" }
        ),
        DraftKind::Note => format!("{target} note"),
    }
}

fn status_mode_label(app: &App) -> String {
    match app.input_mode {
        InputMode::Normal => match app.focus {
            FocusPane::Files => {
                if app.is_diff_mode() {
                    "changed".to_string()
                } else {
                    "files".to_string()
                }
            }
            FocusPane::Source => {
                if app.is_diff_mode() {
                    "diff".to_string()
                } else {
                    "source".to_string()
                }
            }
        },
        InputMode::Visual => "visual".to_string(),
        InputMode::Draft | InputMode::DraftConfirm => app
            .draft
            .as_ref()
            .map(|draft| match draft.kind {
                DraftKind::Question => {
                    if app.is_diff_mode() {
                        "comment".to_string()
                    } else {
                        "question".to_string()
                    }
                }
                DraftKind::Note => "note".to_string(),
            })
            .unwrap_or_else(|| "draft".to_string()),
        InputMode::ThreadView => {
            if app.is_diff_mode() {
                "thread".to_string()
            } else {
                "question".to_string()
            }
        }
        InputMode::FilePicker => "files".to_string(),
        InputMode::Search => "search".to_string(),
        InputMode::Help => "help".to_string(),
        InputMode::CommitSelect => "commits".to_string(),
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
    fn question_cards_render_the_full_thread_in_order() {
        let mut question = Question::new(
            "src/main.rs",
            Some(Anchor::new(11, None)),
            "Why is this branch separate?",
            None,
            vec![],
        );
        question.add_message(
            crate::model::QuestionMessageRole::Agent,
            "It keeps startup work off the fast path.",
        );
        question.add_message(
            crate::model::QuestionMessageRole::User,
            "What invariant depends on that split?",
        );
        question.add_message(
            crate::model::QuestionMessageRole::Agent,
            "The fast path assumes setup already validated the inputs.",
        );

        let rendered = super::render_question_thread(&question, 80, false, false)
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("4 turns"));
        assert!(rendered.contains("Question · original question"));
        assert!(rendered.contains("Follow-up · user follow-up"));
        assert!(rendered.contains("Agent Reply · agent answer"));
        assert!(rendered.contains("Why is this branch separate?"));
        assert!(rendered.contains("What invariant depends on that split?"));
        assert!(rendered.contains("The fast path assumes setup already validated the inputs."));
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
