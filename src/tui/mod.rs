mod app;
mod render;

use std::io::{self, Stdout};
use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

use crate::storage;

use self::app::{App, FocusPane, InputMode};

pub fn run(packet_path: &Path, output_to_stdout: bool) -> Result<()> {
    let packet = storage::read_packet(packet_path)?;
    let mut app = App::load(packet_path.to_path_buf(), packet, output_to_stdout)?;
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
                    if app.input_mode == InputMode::Normal {
                        if pending_d {
                            pending_d = false;
                            if key.code == KeyCode::Char('d') {
                                match app.focus {
                                    FocusPane::Files => {
                                        app.delete_current_file();
                                    }
                                    FocusPane::Source => {
                                        app.delete_annotation_at_cursor();
                                    }
                                }
                                continue;
                            }
                        }

                        if key.code == KeyCode::Char('d') {
                            pending_d = true;
                            app.message = Some(match app.focus {
                                FocusPane::Files => {
                                    "press d again to remove the selected file from this session"
                                }
                                FocusPane::Source => {
                                    "press d again to delete the note or question at the current line"
                                }
                            }
                            .to_string());
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

fn handle_key(app: &mut App, key: KeyEvent) -> Result<()> {
    match app.input_mode {
        InputMode::Normal => handle_normal_mode(app, key),
        InputMode::QuestionComposer => handle_composer_mode(app, key),
        InputMode::FilePicker => handle_file_picker_mode(app, key),
        InputMode::Search => handle_search_mode(app, key),
        InputMode::Help => handle_help_mode(app, key),
    }
}

fn handle_normal_mode(app: &mut App, key: KeyEvent) -> Result<()> {
    app.clear_message();
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
        KeyCode::Char('g') => app.go_to_first_line(),
        KeyCode::Char('G') => app.go_to_last_line(),
        KeyCode::Char('[') => app.jump_to_previous_annotation(),
        KeyCode::Char(']') => app.jump_to_next_annotation(),
        KeyCode::PageDown => app.page_down(),
        KeyCode::PageUp => app.page_up(),
        KeyCode::Char('a') => app.begin_question(),
        KeyCode::Char('f') => app.begin_file_picker()?,
        KeyCode::Char('/') => app.begin_search(),
        KeyCode::Char('r') => app.reload_sources()?,
        KeyCode::Char('s') => app.save()?,
        KeyCode::Char('y') => app.export_questions()?,
        KeyCode::Char('x') => app.save_and_quit()?,
        KeyCode::Char('q') => app.request_quit(),
        _ => {}
    }
    Ok(())
}

fn handle_composer_mode(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc => app.cancel_question(),
        KeyCode::Tab => app.toggle_composer_field(),
        KeyCode::Enter if key.modifiers.contains(KeyModifiers::CONTROL) => app.commit_question()?,
        KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.commit_question()?
        }
        KeyCode::Enter => app.active_buffer_mut().insert('\n'),
        KeyCode::Backspace => app.active_buffer_mut().backspace(),
        KeyCode::Left => app.active_buffer_mut().move_left(),
        KeyCode::Right => app.active_buffer_mut().move_right(),
        KeyCode::Home => app.active_buffer_mut().move_home(),
        KeyCode::End => app.active_buffer_mut().move_end(),
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.active_buffer_mut().clear()
        }
        KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.active_buffer_mut().insert(ch)
        }
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
    match key.code {
        KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') => {
            app.input_mode = InputMode::Normal
        }
        KeyCode::Char('a') => {
            app.input_mode = InputMode::Normal;
            app.begin_question();
        }
        KeyCode::Char('f') => {
            app.input_mode = InputMode::Normal;
            app.begin_file_picker()?;
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
