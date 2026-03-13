mod app;
mod render;

use std::fs;
use std::io::{self, Stdout};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use anyhow::{Result, anyhow};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use uuid::Uuid;

use crate::storage;

use self::app::{App, BrowserMode, FocusPane, InputMode};

pub fn run(packet_path: &Path, output_to_stdout: bool, diff_mode: bool) -> Result<()> {
    let packet = storage::read_packet(packet_path)?;
    let mut app = if diff_mode {
        App::load_diff(packet_path.to_path_buf(), packet, output_to_stdout)?
    } else {
        App::load(packet_path.to_path_buf(), packet, output_to_stdout)?
    };
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        original_hook(panic_info);
    }));

    let mut terminal = setup_terminal()?;
    let result = run_loop(&mut terminal, &mut app);
    restore_terminal(&mut terminal)?;

    if let Some(message) = app.quit_notice.take() {
        println!("{message}");
    }
    if let Some(export) = app.quit_export.take() {
        print!("{export}");
    }

    result
}

fn run_loop(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut App) -> Result<()> {
    let mut pending_d = false;
    loop {
        terminal.draw(|frame| render::render(frame, app))?;

        if app.should_quit {
            break;
        }

        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    if app.input_mode == InputMode::Draft
                        && matches!(key.code, KeyCode::Char('o'))
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        edit_draft_in_editor(terminal, app)?;
                        continue;
                    }

                    if app.input_mode == InputMode::Normal {
                        if pending_d {
                            pending_d = false;
                            if is_plain_char(key, 'd') {
                                if app.browser_mode == BrowserMode::Source {
                                    match app.focus {
                                        FocusPane::Files => {
                                            app.delete_current_file();
                                        }
                                        FocusPane::Source => {
                                            app.delete_annotation_at_cursor();
                                        }
                                    }
                                } else if app.focus == FocusPane::Source {
                                    app.delete_annotation_at_cursor();
                                } else {
                                    app.message = Some(
                                        "dd only deletes annotations from the diff view"
                                            .to_string(),
                                    );
                                }
                                continue;
                            }
                        }

                        if is_plain_char(key, 'd') {
                            pending_d = true;
                            app.message = Some(if app.browser_mode == BrowserMode::Source {
                                match app.focus {
                                    FocusPane::Files => {
                                        "press d again to remove the selected file from this packet"
                                    }
                                    FocusPane::Source => {
                                        "press d again to delete the note or question at the current line"
                                    }
                                }
                                .to_string()
                            } else if app.focus == FocusPane::Source {
                                "press d again to delete the note or comment thread at the current line"
                                    .to_string()
                            } else {
                                "dd is only available in the diff view, not the changed-files sidebar"
                                    .to_string()
                            });
                            continue;
                        }
                    } else {
                        pending_d = false;
                    }

                    handle_key(app, key)?
                }
                Event::Resize(_, _) => {
                    pending_d = false;
                    app.clear_message();
                }
                _ => {}
            }
        }
    }

    Ok(())
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    Ok(Terminal::new(CrosstermBackend::new(stdout))?)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn suspend_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn resume_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    enable_raw_mode()?;
    execute!(terminal.backend_mut(), EnterAlternateScreen)?;
    terminal.hide_cursor()?;
    terminal.clear()?;
    terminal.autoresize()?;
    Ok(())
}

fn is_plain_char(key: KeyEvent, ch: char) -> bool {
    key.modifiers.is_empty() && matches!(key.code, KeyCode::Char(code) if code == ch)
}

fn handle_key(app: &mut App, key: KeyEvent) -> Result<()> {
    match app.input_mode {
        InputMode::Normal => handle_normal_mode(app, key),
        InputMode::Visual => handle_visual_mode(app, key),
        InputMode::Draft => handle_draft_mode(app, key),
        InputMode::DraftConfirm => handle_draft_confirm_mode(app, key),
        InputMode::ThreadView => handle_thread_view_mode(app, key),
        InputMode::FilePicker => handle_file_picker_mode(app, key),
        InputMode::Search => handle_search_mode(app, key),
        InputMode::Help => handle_help_mode(app, key),
        InputMode::CommitSelect => handle_commit_select_mode(app, key),
    }
}

fn handle_normal_mode(app: &mut App, key: KeyEvent) -> Result<()> {
    if app.is_diff_mode() {
        return handle_diff_normal_mode(app, key);
    }
    app.clear_message();
    if handle_scroll_shortcut(app, key) || handle_shared_normal_key(app, key)? {
        return Ok(());
    }

    if let KeyCode::Char('f') = key.code {
        app.begin_file_picker()?;
    }
    Ok(())
}

fn handle_diff_normal_mode(app: &mut App, key: KeyEvent) -> Result<()> {
    app.clear_message();
    if handle_scroll_shortcut(app, key) || handle_shared_normal_key(app, key)? {
        return Ok(());
    }

    match key.code {
        KeyCode::Esc => app.reopen_diff_commit_selector()?,
        KeyCode::Enter | KeyCode::Char(' ') if app.focus == FocusPane::Source => {
            if !app.open_current_thread_viewer() {
                app.toggle_diff_gap_at_cursor()?;
            }
        }
        KeyCode::Char('m') => app.reopen_diff_commit_selector()?,
        KeyCode::Char('r') => app.mark_current_diff_file_reviewed_and_next(),
        _ => {}
    }
    Ok(())
}

fn handle_visual_mode(app: &mut App, key: KeyEvent) -> Result<()> {
    app.clear_message();
    if handle_scroll_shortcut(app, key) {
        return Ok(());
    }

    match key.code {
        KeyCode::Esc | KeyCode::Char('v') | KeyCode::Char('V') => app.exit_visual_mode(),
        KeyCode::Char('j') | KeyCode::Down => app.move_cursor(1),
        KeyCode::Char('k') | KeyCode::Up => app.move_cursor(-1),
        KeyCode::Char('g') => app.go_to_first_line(),
        KeyCode::Char('G') => app.go_to_last_line(),
        KeyCode::PageDown => app.page_down(),
        KeyCode::PageUp => app.page_up(),
        KeyCode::Char('[') => app.jump_to_previous_annotation(),
        KeyCode::Char(']') => app.jump_to_next_annotation(),
        KeyCode::Char('a') => app.begin_question_or_follow_up(),
        KeyCode::Char('n') => app.begin_note(),
        KeyCode::Char('q') => app.request_quit(),
        _ => {}
    }
    Ok(())
}

fn handle_scroll_shortcut(app: &mut App, key: KeyEvent) -> bool {
    if !key.modifiers.contains(KeyModifiers::CONTROL) {
        return false;
    }
    match key.code {
        KeyCode::Char('d') => {
            app.half_page_down();
            true
        }
        KeyCode::Char('u') => {
            app.half_page_up();
            true
        }
        _ => false,
    }
}

fn handle_shared_normal_key(app: &mut App, key: KeyEvent) -> Result<bool> {
    match key.code {
        KeyCode::Tab => app.toggle_focus(),
        KeyCode::Char('?') => app.input_mode = InputMode::Help,
        KeyCode::Char('j') | KeyCode::Down => match app.focus {
            FocusPane::Files => app.move_file(1),
            FocusPane::Source => app.move_cursor(1),
        },
        KeyCode::Char('k') | KeyCode::Up => match app.focus {
            FocusPane::Files => app.move_file(-1),
            FocusPane::Source => app.move_cursor(-1),
        },
        KeyCode::Char('h') | KeyCode::Left if app.focus == FocusPane::Source => app.move_file(-1),
        KeyCode::Char('l') | KeyCode::Right if app.focus == FocusPane::Source => app.move_file(1),
        KeyCode::Enter if app.focus == FocusPane::Files => app.focus = FocusPane::Source,
        KeyCode::Enter | KeyCode::Char(' ') if app.focus == FocusPane::Source => {
            app.open_current_thread_viewer();
        }
        KeyCode::Char('g') => app.go_to_first_line(),
        KeyCode::Char('G') => app.go_to_last_line(),
        KeyCode::Char('[') => app.jump_to_previous_annotation(),
        KeyCode::Char(']') => app.jump_to_next_annotation(),
        KeyCode::PageDown => app.page_down(),
        KeyCode::PageUp => app.page_up(),
        KeyCode::Char('v') | KeyCode::Char('V') => app.enter_visual_mode(),
        KeyCode::Char('a') => app.begin_question_or_follow_up(),
        KeyCode::Char('c') => {
            app.resolve_current_question();
        }
        KeyCode::Char('o') => {
            app.reopen_current_question();
        }
        KeyCode::Char('n') => app.begin_note(),
        KeyCode::Char('i') => app.begin_edit_current_annotation(false),
        KeyCode::Char('I') => app.begin_edit_current_annotation(true),
        KeyCode::Char('/') => app.begin_search(),
        KeyCode::Char('R') => app.reload_sources()?,
        KeyCode::Char('s') => app.save()?,
        KeyCode::Char('y') => app.export_questions()?,
        KeyCode::Char('x') => app.save_and_quit()?,
        KeyCode::Char('q') => app.request_quit(),
        _ => return Ok(false),
    }
    Ok(true)
}

fn handle_thread_view_mode(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter | KeyCode::Char(' ') => {
            app.close_thread_viewer()
        }
        KeyCode::Char('j') | KeyCode::Down => app.scroll_thread_view(1),
        KeyCode::Char('k') | KeyCode::Up => app.scroll_thread_view(-1),
        KeyCode::Char('g') => app.thread_view_to_top(),
        KeyCode::Char('G') => app.thread_view_to_bottom(),
        KeyCode::PageDown => app.page_thread_view_down(),
        KeyCode::PageUp => app.page_thread_view_up(),
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.half_page_thread_view_down()
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.half_page_thread_view_up()
        }
        _ => {}
    }
    Ok(())
}

fn handle_draft_mode(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc => app.request_close_draft(),
        KeyCode::Enter if key.modifiers.contains(KeyModifiers::CONTROL) => app.commit_draft()?,
        KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.commit_draft()?
        }
        KeyCode::Enter => app.active_draft_buffer_mut().insert('\n'),
        KeyCode::Backspace => app.active_draft_buffer_mut().backspace(),
        KeyCode::Left => app.active_draft_buffer_mut().move_left(),
        KeyCode::Right => app.active_draft_buffer_mut().move_right(),
        KeyCode::Home => app.active_draft_buffer_mut().move_home(),
        KeyCode::End => app.active_draft_buffer_mut().move_end(),
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.active_draft_buffer_mut().clear()
        }
        KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.active_draft_buffer_mut().insert(ch)
        }
        _ => {}
    }

    Ok(())
}

fn handle_draft_confirm_mode(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => app.commit_draft()?,
        KeyCode::Char('n') | KeyCode::Char('N') => app.discard_draft(),
        KeyCode::Esc => app.resume_draft(),
        _ => {}
    }
    Ok(())
}

fn handle_file_picker_mode(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc => app.cancel_file_picker(),
        KeyCode::Enter => {
            app.commit_file_picker_selection();
        }
        KeyCode::Char('j') | KeyCode::Down => app.move_file_picker_selection(1),
        KeyCode::Char('k') | KeyCode::Up => app.move_file_picker_selection(-1),
        KeyCode::Backspace => {
            app.active_file_picker_buffer_mut().backspace();
            app.refresh_file_picker_matches();
        }
        KeyCode::Home => app.active_file_picker_buffer_mut().move_home(),
        KeyCode::End => app.active_file_picker_buffer_mut().move_end(),
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.active_file_picker_buffer_mut().clear();
            app.refresh_file_picker_matches();
        }
        KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.active_file_picker_buffer_mut().insert(ch);
            app.refresh_file_picker_matches();
        }
        _ => {}
    }
    Ok(())
}

fn handle_search_mode(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc => app.cancel_search(),
        KeyCode::Enter => {
            app.commit_search_selection();
        }
        KeyCode::Char('j') | KeyCode::Down => app.move_search_selection(1),
        KeyCode::Char('k') | KeyCode::Up => app.move_search_selection(-1),
        KeyCode::Backspace => {
            app.active_search_buffer_mut().backspace();
            app.refresh_search_matches();
        }
        KeyCode::Home => app.active_search_buffer_mut().move_home(),
        KeyCode::End => app.active_search_buffer_mut().move_end(),
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.active_search_buffer_mut().clear();
            app.refresh_search_matches();
        }
        KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.active_search_buffer_mut().insert(ch);
            app.refresh_search_matches();
        }
        _ => {}
    }
    Ok(())
}

fn handle_help_mode(app: &mut App, key: KeyEvent) -> Result<()> {
    if app.is_diff_mode() {
        match key.code {
            KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') => {
                app.input_mode = InputMode::Normal
            }
            _ => {}
        }
        return Ok(());
    }

    match key.code {
        KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') => {
            app.input_mode = InputMode::Normal
        }
        KeyCode::Char('a') => {
            app.input_mode = InputMode::Normal;
            app.begin_question_or_follow_up();
        }
        KeyCode::Char('c') => {
            app.input_mode = InputMode::Normal;
            app.resolve_current_question();
        }
        KeyCode::Char('o') => {
            app.input_mode = InputMode::Normal;
            app.reopen_current_question();
        }
        KeyCode::Char('n') => {
            app.input_mode = InputMode::Normal;
            app.begin_note();
        }
        KeyCode::Char('v') | KeyCode::Char('V') => {
            app.input_mode = InputMode::Normal;
            app.enter_visual_mode();
        }
        KeyCode::Char('i') => {
            app.input_mode = InputMode::Normal;
            app.begin_edit_current_annotation(false);
        }
        KeyCode::Char('f') => {
            app.input_mode = InputMode::Normal;
            app.begin_file_picker()?;
        }
        KeyCode::Char('R') => {
            app.input_mode = InputMode::Normal;
            app.reload_sources()?;
        }
        KeyCode::Char('/') => {
            app.input_mode = InputMode::Normal;
            app.begin_search();
        }
        KeyCode::Char('x') => {
            app.input_mode = InputMode::Normal;
            app.save_and_quit()?;
        }
        KeyCode::Char('s') => {
            app.input_mode = InputMode::Normal;
            app.save()?;
        }
        KeyCode::Char('y') => {
            app.input_mode = InputMode::Normal;
            app.export_questions()?;
        }
        _ => {}
    }

    Ok(())
}

fn handle_commit_select_mode(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => app.request_quit(),
        KeyCode::Char('j') | KeyCode::Down => app.move_diff_commit_cursor(1),
        KeyCode::Char('k') | KeyCode::Up => app.move_diff_commit_cursor(-1),
        KeyCode::Char(' ') => app.toggle_diff_commit_selection(),
        KeyCode::Enter => app.confirm_diff_commit_selection()?,
        _ => {}
    }
    Ok(())
}

fn edit_draft_in_editor(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
) -> Result<()> {
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .map_err(|_| anyhow!("set VISUAL or EDITOR to use external editing"))?;
    let Some(initial_text) = app.draft.as_ref().map(|draft| draft.buffer.text.clone()) else {
        return Ok(());
    };

    let edit_path = std::env::temp_dir().join(format!("copanion-draft-{}.md", Uuid::new_v4()));
    fs::write(&edit_path, initial_text)?;

    suspend_terminal(terminal)?;
    let status = Command::new("sh")
        .arg("-c")
        .arg("\"$COPANION_EDITOR\" \"$COPANION_EDIT_PATH\"")
        .env("COPANION_EDITOR", editor)
        .env("COPANION_EDIT_PATH", &edit_path)
        .status();
    resume_terminal(terminal)?;
    terminal.draw(|frame| render::render(frame, app))?;

    let status = status?;
    if !status.success() {
        let _ = fs::remove_file(&edit_path);
        return Err(anyhow!("the external editor exited with {status}"));
    }

    let edited = fs::read_to_string(&edit_path)?;
    let _ = fs::remove_file(&edit_path);
    let Some(draft) = app.draft.as_mut() else {
        return Ok(());
    };
    draft.buffer = self::app::TextBuffer::from_text(edited);
    app.message = Some("draft updated from the external editor".to_string());
    Ok(())
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::is_plain_char;

    #[test]
    fn plain_char_helper_rejects_control_modified_keys() {
        assert!(is_plain_char(
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE),
            'd'
        ));
        assert!(!is_plain_char(
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL),
            'd'
        ));
    }
}
