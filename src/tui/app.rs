use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::Result;
use ignore::WalkBuilder;
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

use crate::clipboard;
use crate::export;
use crate::model::{Anchor, Note, Packet, Question, QuestionStatus};
use crate::{storage, syntax};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum FocusPane {
    Files,
    Source,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum InputMode {
    Normal,
    QuestionComposer,
    FilePicker,
    Search,
    Help,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ComposerField {
    Prompt,
    Why,
}

#[derive(Debug, Clone, Default)]
pub struct TextBuffer {
    pub text: String,
    pub cursor: usize,
}

#[derive(Debug, Clone)]
pub struct QuestionComposer {
    pub path: String,
    pub anchor: Anchor,
    pub related_note_ids: Vec<String>,
    pub prompt: TextBuffer,
    pub why: TextBuffer,
    pub field: ComposerField,
}

#[derive(Debug, Clone)]
pub struct FilePickerState {
    pub query: TextBuffer,
    pub candidates: Vec<String>,
    pub matches: Vec<String>,
    pub selected: usize,
}

#[derive(Debug, Clone)]
pub struct SearchMatch {
    pub path: String,
    pub line: usize,
    pub label: String,
    pub preview: String,
}

#[derive(Debug, Clone)]
pub struct SearchState {
    pub query: TextBuffer,
    pub candidates: Vec<SearchMatch>,
    pub matches: Vec<SearchMatch>,
    pub selected: usize,
}

#[derive(Debug, Clone, Default)]
pub struct ViewMetrics {
    pub line_to_row: Vec<usize>,
    pub annotation_lines: Vec<usize>,
    pub total_rows: usize,
    pub viewport_height: usize,
}

#[derive(Debug, Clone)]
pub struct LoadedFile {
    pub path: String,
    pub lines: Vec<String>,
    pub highlighted_lines: Vec<syntax::StyledSegments>,
    pub load_error: Option<String>,
}

pub struct App {
    pub root: PathBuf,
    pub packet_path: PathBuf,
    pub packet: Packet,
    pub output_to_stdout: bool,
    pub files: Vec<LoadedFile>,
    pub current_file: usize,
    pub cursor_line: usize,
    pub scroll: usize,
    pub focus: FocusPane,
    pub input_mode: InputMode,
    pub composer: Option<QuestionComposer>,
    pub file_picker: Option<FilePickerState>,
    pub search: Option<SearchState>,
    pub view_metrics: ViewMetrics,
    pub message: Option<String>,
    pub should_quit: bool,
    pub dirty: bool,
    pub discard_guard: bool,
    pub quit_notice: Option<String>,
    pub quit_export: Option<String>,
    pub composer_cursor_screen_pos: Option<(u16, u16)>,
}

impl App {
    pub fn load(packet_path: PathBuf, packet: Packet, output_to_stdout: bool) -> Result<Self> {
        let root = PathBuf::from(&packet.workspace_root);
        let files = load_files(&root, &packet);
        let mut app = Self {
            root,
            packet_path,
            packet,
            output_to_stdout,
            files,
            current_file: 0,
            cursor_line: 1,
            scroll: 0,
            focus: FocusPane::Files,
            input_mode: InputMode::Normal,
            composer: None,
            file_picker: None,
            search: None,
            view_metrics: ViewMetrics::default(),
            message: None,
            should_quit: false,
            dirty: false,
            discard_guard: false,
            quit_notice: None,
            quit_export: None,
            composer_cursor_screen_pos: None,
        };

        if app.files.is_empty() {
            app.begin_file_picker()?;
        } else {
            app.focus = FocusPane::Source;
        }

        Ok(app)
    }

    pub fn clear_message(&mut self) {
        self.message = None;
    }

    pub fn active_buffer_mut(&mut self) -> &mut TextBuffer {
        let composer = self
            .composer
            .as_mut()
            .expect("composer buffer requested outside of composer mode");
        match composer.field {
            ComposerField::Prompt => &mut composer.prompt,
            ComposerField::Why => &mut composer.why,
        }
    }

    pub fn active_file_picker_buffer_mut(&mut self) -> &mut TextBuffer {
        &mut self
            .file_picker
            .as_mut()
            .expect("file picker buffer requested outside of file picker mode")
            .query
    }

    pub fn active_search_buffer_mut(&mut self) -> &mut TextBuffer {
        &mut self
            .search
            .as_mut()
            .expect("search buffer requested outside of search mode")
            .query
    }

    pub fn toggle_composer_field(&mut self) {
        if let Some(composer) = &mut self.composer {
            composer.field = match composer.field {
                ComposerField::Prompt => ComposerField::Why,
                ComposerField::Why => ComposerField::Prompt,
            };
        }
    }

    pub fn toggle_focus(&mut self) {
        self.discard_guard = false;
        if self.files.is_empty() {
            return;
        }
        self.focus = match self.focus {
            FocusPane::Files => FocusPane::Source,
            FocusPane::Source => FocusPane::Files,
        };
    }

    pub fn move_cursor(&mut self, delta: isize) {
        self.discard_guard = false;
        let max_line = self.max_source_line();
        let next = (self.cursor_line as isize + delta).clamp(1, max_line as isize) as usize;
        self.cursor_line = next;
        self.ensure_cursor_visible();
    }

    pub fn go_to_first_line(&mut self) {
        self.discard_guard = false;
        self.cursor_line = 1;
        self.ensure_cursor_visible();
    }

    pub fn go_to_last_line(&mut self) {
        self.discard_guard = false;
        self.cursor_line = self.max_source_line();
        self.ensure_cursor_visible();
    }

    pub fn page_down(&mut self) {
        let step = self.view_metrics.viewport_height.max(1) as isize;
        self.move_cursor(step);
    }

    pub fn page_up(&mut self) {
        let step = self.view_metrics.viewport_height.max(1) as isize;
        self.move_cursor(-step);
    }

    pub fn move_file(&mut self, delta: isize) {
        self.discard_guard = false;
        if self.files.is_empty() {
            return;
        }
        let next = (self.current_file as isize + delta)
            .clamp(0, self.files.len().saturating_sub(1) as isize) as usize;
        self.select_file(next);
    }

    pub fn select_file(&mut self, index: usize) {
        self.current_file = index.min(self.files.len().saturating_sub(1));
        self.cursor_line = 1;
        self.scroll = 0;
        self.ensure_cursor_visible();
    }

    pub fn jump_to_next_annotation(&mut self) {
        self.discard_guard = false;
        if let Some(line) = self
            .view_metrics
            .annotation_lines
            .iter()
            .copied()
            .find(|line| *line > self.cursor_line)
        {
            self.cursor_line = line;
            self.ensure_cursor_visible();
        }
    }

    pub fn jump_to_previous_annotation(&mut self) {
        self.discard_guard = false;
        if let Some(line) = self
            .view_metrics
            .annotation_lines
            .iter()
            .copied()
            .rev()
            .find(|line| *line < self.cursor_line)
        {
            self.cursor_line = line;
            self.ensure_cursor_visible();
        }
    }

    pub fn begin_question(&mut self) {
        if self.files.is_empty() {
            self.message = Some("add a tracked file before asking a question".to_string());
            return;
        }
        self.discard_guard = false;
        let anchor = Anchor::new(self.cursor_line, None);
        let related_note_ids = self
            .notes_for_current_line()
            .into_iter()
            .map(|note| note.id.clone())
            .collect();
        self.composer = Some(QuestionComposer {
            path: self.current_path().to_string(),
            anchor,
            related_note_ids,
            prompt: TextBuffer::default(),
            why: TextBuffer::default(),
            field: ComposerField::Prompt,
        });
        self.input_mode = InputMode::QuestionComposer;
        self.message = Some("compose the follow-up prompt; Ctrl-S saves the question".to_string());
    }

    pub fn cancel_question(&mut self) {
        self.composer = None;
        self.input_mode = InputMode::Normal;
        self.message = Some("question discarded".to_string());
    }

    pub fn begin_file_picker(&mut self) -> Result<()> {
        let candidates = discover_workspace_files(&self.root, &self.packet)?;
        let mut picker = FilePickerState {
            query: TextBuffer::default(),
            candidates,
            matches: Vec::new(),
            selected: 0,
        };
        picker.refresh_matches();
        self.file_picker = Some(picker);
        self.input_mode = InputMode::FilePicker;
        self.focus = FocusPane::Files;
        self.message = Some("fuzzy-search for a file and press Enter to add it".to_string());
        Ok(())
    }

    pub fn cancel_file_picker(&mut self) {
        self.file_picker = None;
        self.input_mode = InputMode::Normal;
        self.message = Some("file picker closed".to_string());
    }

    pub fn refresh_file_picker_matches(&mut self) {
        if let Some(picker) = &mut self.file_picker {
            picker.refresh_matches();
        }
    }

    pub fn move_file_picker_selection(&mut self, delta: isize) {
        let Some(picker) = &mut self.file_picker else {
            return;
        };
        if picker.matches.is_empty() {
            picker.selected = 0;
            return;
        }
        picker.selected = (picker.selected as isize + delta)
            .clamp(0, picker.matches.len().saturating_sub(1) as isize)
            as usize;
    }

    pub fn commit_file_picker_selection(&mut self) -> bool {
        let Some(mut picker) = self.file_picker.take() else {
            return false;
        };
        let Some(path) = picker.matches.get(picker.selected).cloned() else {
            picker.refresh_matches();
            self.file_picker = Some(picker);
            self.input_mode = InputMode::FilePicker;
            self.message = Some("no file matches the current search".to_string());
            return false;
        };

        self.packet.ensure_file(path.clone());
        self.packet.touch();
        self.dirty = true;
        self.discard_guard = false;
        self.files = load_files(&self.root, &self.packet);
        if let Some(index) = self.files.iter().position(|file| file.path == path) {
            self.current_file = index;
        }
        self.cursor_line = 1;
        self.scroll = 0;
        self.focus = FocusPane::Source;
        self.input_mode = InputMode::Normal;
        self.ensure_cursor_visible();
        self.message = Some(format!("added tracked file {path}"));
        true
    }

    pub fn begin_search(&mut self) {
        let candidates = self
            .packet
            .notes
            .iter()
            .map(|note| SearchMatch {
                path: note.path.clone(),
                line: note.anchor.start_line,
                label: note.title.clone(),
                preview: note.body.clone(),
            })
            .chain(self.packet.questions.iter().filter_map(|question| {
                question.anchor.map(|anchor| SearchMatch {
                    path: question.path.clone(),
                    line: anchor.start_line,
                    label: "Question".to_string(),
                    preview: question.prompt.clone(),
                })
            }))
            .collect::<Vec<_>>();

        if candidates.is_empty() {
            self.message = Some("there are no notes or open questions to search yet".to_string());
            return;
        }

        let mut search = SearchState {
            query: TextBuffer::default(),
            candidates,
            matches: Vec::new(),
            selected: 0,
        };
        search.refresh_matches();
        self.search = Some(search);
        self.input_mode = InputMode::Search;
        self.message =
            Some("fuzzy-search notes and questions, then press Enter to jump".to_string());
    }

    pub fn cancel_search(&mut self) {
        self.search = None;
        self.input_mode = InputMode::Normal;
        self.message = Some("search closed".to_string());
    }

    pub fn refresh_search_matches(&mut self) {
        if let Some(search) = &mut self.search {
            search.refresh_matches();
        }
    }

    pub fn move_search_selection(&mut self, delta: isize) {
        let Some(search) = &mut self.search else {
            return;
        };
        if search.matches.is_empty() {
            search.selected = 0;
            return;
        }
        search.selected = (search.selected as isize + delta)
            .clamp(0, search.matches.len().saturating_sub(1) as isize)
            as usize;
    }

    pub fn commit_search_selection(&mut self) -> bool {
        let Some(mut search) = self.search.take() else {
            return false;
        };
        let Some(selection) = search.matches.get(search.selected).cloned() else {
            search.refresh_matches();
            self.search = Some(search);
            self.input_mode = InputMode::Search;
            self.message = Some("no note or question matches the current search".to_string());
            return false;
        };

        if let Some(index) = self
            .files
            .iter()
            .position(|file| file.path == selection.path)
        {
            self.current_file = index;
        } else {
            self.input_mode = InputMode::Normal;
            self.message = Some(
                "the selected search result points at a file that is no longer tracked".to_string(),
            );
            return false;
        }

        self.focus = FocusPane::Source;
        self.input_mode = InputMode::Normal;
        self.cursor_line = selection.line.max(1);
        self.ensure_cursor_visible();
        self.message = Some(format!("jumped to {}:{}", selection.path, selection.line));
        true
    }

    pub fn commit_question(&mut self) -> Result<()> {
        let Some(composer) = self.composer.take() else {
            return Ok(());
        };

        let prompt = composer.prompt.text.trim().to_string();
        if prompt.is_empty() {
            self.composer = Some(composer);
            self.message = Some("question prompt cannot be empty".to_string());
            return Ok(());
        }

        let why = match composer.why.text.trim() {
            "" => None,
            why => Some(why.to_string()),
        };

        self.packet.ensure_file(composer.path.clone());
        self.packet.questions.push(Question::new(
            composer.path,
            Some(composer.anchor),
            prompt,
            why,
            composer.related_note_ids,
        ));
        self.packet.touch();
        self.dirty = true;
        self.input_mode = InputMode::Normal;
        self.message = Some("question staged; press s to save or x to save and export".to_string());
        Ok(())
    }

    pub fn save(&mut self) -> Result<()> {
        storage::write_packet(&self.packet_path, &self.packet)?;
        self.dirty = false;
        self.discard_guard = false;
        self.message = Some(format!("saved {}", self.packet_path.display()));
        Ok(())
    }

    pub fn export_questions(&mut self) -> Result<()> {
        let export = export::generate_question_export(&self.packet)?;
        self.discard_guard = false;
        if self.output_to_stdout {
            self.message = Some("open questions rendered to stdout on exit".to_string());
            self.quit_export = Some(export);
        } else {
            let message = clipboard::copy_text(&export)?;
            self.message = Some(format!("open questions {message}"));
        }
        Ok(())
    }

    pub fn save_and_quit(&mut self) -> Result<()> {
        self.save()?;
        let notice = if self.packet.open_questions().count() > 0 {
            let export = export::generate_question_export(&self.packet)?;
            if self.output_to_stdout {
                self.quit_export = Some(export);
                format!(
                    "saved {} and wrote the open questions to stdout",
                    self.packet_path.display()
                )
            } else {
                let copy_result = clipboard::copy_text(&export)?;
                format!(
                    "saved {} and exported the open questions ({copy_result})",
                    self.packet_path.display()
                )
            }
        } else {
            format!("saved {}", self.packet_path.display())
        };
        self.quit_notice = Some(notice);
        self.should_quit = true;
        Ok(())
    }

    pub fn delete_current_file(&mut self) -> bool {
        let path = self.current_path().to_string();
        let remaining_paths = self
            .packet
            .files
            .iter()
            .filter(|file| file.path != path)
            .map(|file| file.path.clone())
            .chain(
                self.packet
                    .notes
                    .iter()
                    .filter(|note| note.path != path)
                    .map(|note| note.path.clone()),
            )
            .chain(
                self.packet
                    .questions
                    .iter()
                    .filter(|question| question.path != path)
                    .map(|question| question.path.clone()),
            )
            .collect::<BTreeSet<_>>();
        if remaining_paths.is_empty() {
            self.message =
                Some("cannot remove the last tracked file from inside the TUI".to_string());
            return false;
        }

        let removed_notes = self
            .packet
            .notes
            .iter()
            .filter(|note| note.path == path)
            .count();
        let removed_questions = self
            .packet
            .questions
            .iter()
            .filter(|question| question.path == path)
            .count();

        self.packet.files.retain(|file| file.path != path);
        self.packet.notes.retain(|note| note.path != path);
        self.packet
            .questions
            .retain(|question| question.path != path);
        self.packet.touch();
        self.dirty = true;
        self.discard_guard = false;

        self.files = load_files(&self.root, &self.packet);
        if self.current_file >= self.files.len() {
            self.current_file = self.files.len().saturating_sub(1);
        }
        self.cursor_line = 1;
        self.scroll = 0;
        self.ensure_cursor_visible();
        self.message = Some(format!(
            "removed {path} and purged {removed_notes} notes, {removed_questions} questions"
        ));
        true
    }

    pub fn delete_annotation_at_cursor(&mut self) -> bool {
        let path = self.current_path().to_string();
        if let Some(index) = self
            .packet
            .notes
            .iter()
            .rposition(|note| note.path == path && note_covers_line(note, self.cursor_line))
        {
            let deleted = self.packet.notes.remove(index);
            self.packet.touch();
            self.dirty = true;
            self.discard_guard = false;
            self.message = Some(format!("deleted note {}", deleted.title));
            return true;
        }

        if let Some(index) = self.packet.questions.iter().rposition(|question| {
            question.path == path && question_covers_line(question, self.cursor_line)
        }) {
            let deleted = self.packet.questions.remove(index);
            self.packet.touch();
            self.dirty = true;
            self.discard_guard = false;
            self.message = Some(format!("deleted question {}", deleted.id));
            return true;
        }

        self.message = Some("no note or question is attached to the current line".to_string());
        false
    }

    pub fn request_quit(&mut self) {
        if self.dirty && !self.discard_guard {
            self.discard_guard = true;
            self.message =
                Some("unsaved changes: press q again to discard, or s/x to keep them".to_string());
            return;
        }
        self.should_quit = true;
        self.quit_notice = Some("quit without saving".to_string());
    }

    pub fn reload_sources(&mut self) -> Result<()> {
        self.files = load_files(&self.root, &self.packet);
        if self.current_file >= self.files.len() {
            self.current_file = self.files.len().saturating_sub(1);
        }
        self.cursor_line = self.cursor_line.min(self.max_source_line()).max(1);
        self.ensure_cursor_visible();
        self.message = Some("reloaded tracked source files".to_string());
        Ok(())
    }

    pub fn update_view_metrics(&mut self, mut metrics: ViewMetrics) {
        metrics.viewport_height = metrics.viewport_height.max(1);
        self.view_metrics = metrics;
        self.ensure_cursor_visible();
    }

    pub fn current_file(&self) -> &LoadedFile {
        if let Some(file) = self.files.get(self.current_file) {
            file
        } else {
            empty_loaded_file()
        }
    }

    pub fn current_path(&self) -> &str {
        self.files
            .get(self.current_file)
            .map(|file| file.path.as_str())
            .unwrap_or("")
    }

    pub fn max_source_line(&self) -> usize {
        self.files
            .get(self.current_file)
            .map(|file| file.lines.len().max(1))
            .unwrap_or(1)
    }

    pub fn file_note_count(&self, path: &str) -> usize {
        self.packet
            .notes
            .iter()
            .filter(|note| note.path == path)
            .count()
    }

    pub fn file_open_question_count(&self, path: &str) -> usize {
        self.packet
            .questions
            .iter()
            .filter(|question| question.path == path && question.status == QuestionStatus::Open)
            .count()
    }

    pub fn notes_for_current_line(&self) -> Vec<&Note> {
        self.packet
            .notes
            .iter()
            .filter(|note| {
                note.path == self.current_path() && note_covers_line(note, self.cursor_line)
            })
            .collect()
    }

    pub fn notes_for_path(&self, path: &str) -> Vec<&Note> {
        self.packet
            .notes
            .iter()
            .filter(|note| note.path == path)
            .collect()
    }

    pub fn questions_for_path(&self, path: &str) -> Vec<&Question> {
        self.packet
            .questions
            .iter()
            .filter(|question| question.path == path && question.status == QuestionStatus::Open)
            .collect()
    }

    fn ensure_cursor_visible(&mut self) {
        if self.view_metrics.line_to_row.is_empty() {
            self.scroll = 0;
            return;
        }

        let line_index = self.cursor_line.saturating_sub(1);
        let row = *self
            .view_metrics
            .line_to_row
            .get(line_index)
            .unwrap_or_else(|| self.view_metrics.line_to_row.last().unwrap_or(&0));

        let viewport_height = self.view_metrics.viewport_height.max(1);
        if row < self.scroll {
            self.scroll = row;
        } else if row >= self.scroll + viewport_height {
            self.scroll = row.saturating_add(1).saturating_sub(viewport_height);
        }

        let max_scroll = self.view_metrics.total_rows.saturating_sub(viewport_height);
        self.scroll = self.scroll.min(max_scroll);
    }
}

impl TextBuffer {
    pub fn insert(&mut self, ch: char) {
        self.text.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
    }

    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let previous = self
            .text
            .char_indices()
            .take_while(|(idx, _)| *idx < self.cursor)
            .last()
            .map(|(idx, _)| idx)
            .unwrap_or(0);
        self.text.drain(previous..self.cursor);
        self.cursor = previous;
    }

    pub fn move_left(&mut self) {
        if let Some((idx, _)) = self
            .text
            .char_indices()
            .take_while(|(idx, _)| *idx < self.cursor)
            .last()
        {
            self.cursor = idx;
        }
    }

    pub fn move_right(&mut self) {
        if self.cursor >= self.text.len() {
            return;
        }
        if let Some((idx, ch)) = self.text[self.cursor..].char_indices().next() {
            self.cursor += idx + ch.len_utf8();
        }
    }

    pub fn move_home(&mut self) {
        self.cursor = 0;
    }

    pub fn move_end(&mut self) {
        self.cursor = self.text.len();
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }
}

impl FilePickerState {
    fn refresh_matches(&mut self) {
        self.matches = rank_strings(&self.query.text, &self.candidates);
        if self.matches.is_empty() {
            self.selected = 0;
        } else {
            self.selected = self.selected.min(self.matches.len().saturating_sub(1));
        }
    }
}

impl SearchState {
    fn refresh_matches(&mut self) {
        self.matches = rank_search_matches(&self.query.text, &self.candidates);
        if self.matches.is_empty() {
            self.selected = 0;
        } else {
            self.selected = self.selected.min(self.matches.len().saturating_sub(1));
        }
    }
}

fn load_files(root: &Path, packet: &Packet) -> Vec<LoadedFile> {
    let mut seen = BTreeSet::new();
    let mut ordered_paths = Vec::new();

    for file in &packet.files {
        if seen.insert(file.path.clone()) {
            ordered_paths.push(file.path.clone());
        }
    }
    for note in &packet.notes {
        if seen.insert(note.path.clone()) {
            ordered_paths.push(note.path.clone());
        }
    }
    for question in &packet.questions {
        if seen.insert(question.path.clone()) {
            ordered_paths.push(question.path.clone());
        }
    }

    ordered_paths
        .into_iter()
        .map(|path| load_file(root, path))
        .collect()
}

fn load_file(root: &Path, path: String) -> LoadedFile {
    let absolute = root.join(&path);
    match fs::read_to_string(&absolute) {
        Ok(contents) => {
            let lines = contents
                .lines()
                .map(ToString::to_string)
                .collect::<Vec<_>>();
            let highlighted_lines = syntax::highlight_file(&path, &lines);
            LoadedFile {
                path,
                lines,
                highlighted_lines,
                load_error: None,
            }
        }
        Err(err) => LoadedFile {
            path,
            lines: Vec::new(),
            highlighted_lines: Vec::new(),
            load_error: Some(err.to_string()),
        },
    }
}

fn empty_loaded_file() -> &'static LoadedFile {
    static EMPTY_FILE: OnceLock<LoadedFile> = OnceLock::new();
    EMPTY_FILE.get_or_init(|| LoadedFile {
        path: "No tracked files".to_string(),
        lines: Vec::new(),
        highlighted_lines: Vec::new(),
        load_error: Some("Use f to add a file from the workspace.".to_string()),
    })
}

fn rank_strings(query: &str, candidates: &[String]) -> Vec<String> {
    let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
    let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
    pattern
        .match_list(candidates.iter().cloned(), &mut matcher)
        .into_iter()
        .map(|(path, _)| path)
        .collect()
}

fn rank_search_matches(query: &str, candidates: &[SearchMatch]) -> Vec<SearchMatch> {
    let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
    let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
    let mut buf = Vec::new();
    let mut scored = candidates
        .iter()
        .filter_map(|candidate| {
            let haystack = format!(
                "{} {} {}:{}",
                candidate.label, candidate.preview, candidate.path, candidate.line
            );
            pattern
                .score(Utf32Str::new(&haystack, &mut buf), &mut matcher)
                .map(|score| (score, candidate.clone()))
        })
        .collect::<Vec<_>>();
    scored.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| left.1.path.cmp(&right.1.path))
            .then_with(|| left.1.line.cmp(&right.1.line))
    });
    scored.into_iter().map(|(_, candidate)| candidate).collect()
}

fn discover_workspace_files(root: &Path, packet: &Packet) -> Result<Vec<String>> {
    let tracked = packet
        .files
        .iter()
        .map(|file| file.path.clone())
        .collect::<BTreeSet<_>>();
    let mut candidates = WalkBuilder::new(root)
        .standard_filters(true)
        .build()
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .file_type()
                .map(|kind| kind.is_file())
                .unwrap_or(false)
        })
        .map(|entry| storage::normalize_repo_path(entry.path(), root))
        .filter(|path| !tracked.contains(path))
        .collect::<Vec<_>>();
    candidates.sort();
    Ok(candidates)
}

fn note_covers_line(note: &Note, line: usize) -> bool {
    let end = note.anchor.end_line.unwrap_or(note.anchor.start_line);
    line >= note.anchor.start_line && line <= end
}

fn question_covers_line(question: &Question, line: usize) -> bool {
    let Some(anchor) = question.anchor else {
        return false;
    };
    let end = anchor.end_line.unwrap_or(anchor.start_line);
    line >= anchor.start_line && line <= end
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use crate::model::{Note, NoteKind, NoteSource, Packet, Question, TrackedFile};

    use super::{App, FocusPane, TextBuffer};
    use crate::model::Anchor;

    #[test]
    fn text_buffer_basic_editing() {
        let mut buffer = TextBuffer::default();
        buffer.insert('a');
        buffer.insert('b');
        buffer.backspace();
        assert_eq!(buffer.text, "a");
        buffer.move_left();
        buffer.insert('z');
        assert_eq!(buffer.text, "za");
    }

    #[test]
    fn app_stages_questions_against_current_line() {
        let temp = tempdir().unwrap();
        std::fs::write(temp.path().join("main.rs"), "fn main() {}\n").unwrap();
        let mut packet = Packet::new(
            "tour",
            "Tour",
            temp.path().display().to_string(),
            vec![TrackedFile::new("main.rs")],
        );
        packet.notes.push(Note::new(
            "main.rs",
            Anchor::new(1, None),
            NoteKind::Overview,
            "Entry point",
            "This function starts the binary.",
            vec![],
            None,
            NoteSource::Agent,
        ));
        let mut app = App::load(temp.path().join("tour.toml"), packet, false).unwrap();
        app.focus = FocusPane::Source;
        app.begin_question();
        {
            let prompt = app.active_buffer_mut();
            prompt.text = "Why is main empty?".to_string();
            prompt.cursor = prompt.text.len();
        }
        app.toggle_composer_field();
        {
            let why = app.active_buffer_mut();
            why.text = "The note explains what it is, not why it is currently a stub.".to_string();
            why.cursor = why.text.len();
        }
        app.commit_question().unwrap();
        assert_eq!(app.packet.questions.len(), 1);
        assert!(app.dirty);
        assert_eq!(app.packet.questions[0].related_note_ids.len(), 1);
    }

    #[test]
    fn delete_annotation_prefers_notes_then_questions() {
        let temp = tempdir().unwrap();
        std::fs::write(temp.path().join("main.rs"), "fn main() {}\n").unwrap();
        let mut packet = Packet::new(
            "tour",
            "Tour",
            temp.path().display().to_string(),
            vec![TrackedFile::new("main.rs")],
        );
        packet.notes.push(Note::new(
            "main.rs",
            Anchor::new(1, None),
            NoteKind::Overview,
            "Entry point",
            "This function starts the binary.",
            vec![],
            None,
            NoteSource::Agent,
        ));
        packet.questions.push(Question::new(
            "main.rs",
            Some(Anchor::new(1, None)),
            "Why is main empty?",
            None,
            vec![],
        ));
        let mut app = App::load(temp.path().join("tour.toml"), packet, false).unwrap();
        assert!(app.delete_annotation_at_cursor());
        assert!(app.packet.notes.is_empty());
        assert_eq!(app.packet.questions.len(), 1);
        assert!(app.delete_annotation_at_cursor());
        assert!(app.packet.questions.is_empty());
    }

    #[test]
    fn deleting_current_file_purges_related_state() {
        let temp = tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("src")).unwrap();
        std::fs::write(temp.path().join("src/main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(temp.path().join("src/lib.rs"), "pub fn helper() {}\n").unwrap();
        let mut packet = Packet::new(
            "tour",
            "Tour",
            temp.path().display().to_string(),
            vec![
                TrackedFile::new("src/main.rs"),
                TrackedFile::new("src/lib.rs"),
            ],
        );
        packet.notes.push(Note::new(
            "src/main.rs",
            Anchor::new(1, None),
            NoteKind::Overview,
            "Entry point",
            "This function starts the binary.",
            vec![],
            None,
            NoteSource::Agent,
        ));
        packet.questions.push(Question::new(
            "src/main.rs",
            Some(Anchor::new(1, None)),
            "Why is main empty?",
            None,
            vec![],
        ));
        let mut app = App::load(temp.path().join("tour.toml"), packet, false).unwrap();
        assert!(app.delete_current_file());
        assert_eq!(app.files.len(), 1);
        assert_eq!(app.current_path(), "src/lib.rs");
        assert!(app.packet.notes.is_empty());
        assert!(app.packet.questions.is_empty());
    }

    #[test]
    fn empty_session_opens_file_picker_and_adds_file() {
        let temp = tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("src")).unwrap();
        std::fs::write(temp.path().join("src/main.rs"), "fn main() {}\n").unwrap();
        let packet = Packet::new("tour", "Tour", temp.path().display().to_string(), vec![]);
        let mut app = App::load(temp.path().join("tour.toml"), packet, false).unwrap();
        assert_eq!(app.input_mode, super::InputMode::FilePicker);
        assert!(app.commit_file_picker_selection());
        assert_eq!(app.files.len(), 1);
        assert_eq!(app.current_path(), "src/main.rs");
    }
}
