use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::Result;
use chrono::Utc;
use ignore::WalkBuilder;
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

use crate::clipboard;
use crate::export;
use crate::model::{
    Anchor, Note, NoteKind, NoteSource, Packet, Question, QuestionMessageRole, QuestionStatus,
};
use crate::{storage, syntax};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum FocusPane {
    Files,
    Source,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum InputMode {
    Normal,
    Visual,
    Draft,
    DraftConfirm,
    FilePicker,
    Search,
    Help,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum DraftKind {
    Question,
    Note,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum DraftTarget {
    New,
    EditNote { index: usize },
    EditQuestionPrompt { index: usize },
    EditQuestionMessage { question_index: usize, message_index: usize },
    ContinueQuestion { index: usize },
}

#[derive(Debug, Clone, Default)]
pub struct TextBuffer {
    pub text: String,
    pub cursor: usize,
}

#[derive(Debug, Clone)]
pub struct PromptDraft {
    pub kind: DraftKind,
    pub target: DraftTarget,
    pub path: String,
    pub anchor: Anchor,
    pub related_note_ids: Vec<String>,
    pub buffer: TextBuffer,
    pub original_text: String,
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
    pub draft: Option<PromptDraft>,
    pub file_picker: Option<FilePickerState>,
    pub search: Option<SearchState>,
    pub visual_anchor: Option<usize>,
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
            draft: None,
            file_picker: None,
            search: None,
            visual_anchor: None,
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

    pub fn active_draft_buffer_mut(&mut self) -> &mut TextBuffer {
        &mut self
            .draft
            .as_mut()
            .expect("draft buffer requested outside of draft mode")
            .buffer
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
            .or_else(|| self.view_metrics.annotation_lines.first().copied())
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
            .or_else(|| self.view_metrics.annotation_lines.last().copied())
        {
            self.cursor_line = line;
            self.ensure_cursor_visible();
        }
    }

    pub fn begin_question(&mut self) {
        self.begin_new_draft(DraftKind::Question);
    }

    pub fn begin_question_follow_up(&mut self) {
        if self.files.is_empty() {
            self.message = Some("add a tracked file before continuing a question".to_string());
            return;
        }
        let Some(index) = self.current_open_question_index() else {
            self.message =
                Some("there is no open question thread on the current line to continue".to_string());
            return;
        };
        let question = &self.packet.questions[index];
        self.discard_guard = false;
        self.visual_anchor = None;
        self.draft = Some(PromptDraft {
            kind: DraftKind::Question,
            target: DraftTarget::ContinueQuestion { index },
            path: question.path.clone(),
            anchor: question
                .anchor
                .unwrap_or_else(|| Anchor::new(self.cursor_line, None)),
            related_note_ids: question.related_note_ids.clone(),
            buffer: TextBuffer::default(),
            original_text: String::new(),
        });
        self.input_mode = InputMode::Draft;
        self.message = Some(
            "continue the conversation; Ctrl-S saves, Ctrl-O opens the external editor"
                .to_string(),
        );
    }

    pub fn begin_note(&mut self) {
        self.begin_new_draft(DraftKind::Note);
    }

    pub fn enter_visual_mode(&mut self) {
        if self.files.is_empty() {
            self.message = Some("add a tracked file before selecting a range".to_string());
            return;
        }
        self.discard_guard = false;
        self.focus = FocusPane::Source;
        self.input_mode = InputMode::Visual;
        self.visual_anchor = Some(self.cursor_line);
        self.message =
            Some("visual selection started; move the cursor and press a or n".to_string());
    }

    pub fn exit_visual_mode(&mut self) {
        self.input_mode = InputMode::Normal;
        self.visual_anchor = None;
        self.message = Some("visual selection cleared".to_string());
    }

    pub fn visual_selection(&self) -> Option<Anchor> {
        if self.input_mode != InputMode::Visual {
            return None;
        }
        let anchor = self.visual_anchor?;
        Some(anchor_from_lines(anchor, self.cursor_line))
    }

    pub fn is_line_in_visual_selection(&self, line: usize) -> bool {
        let Some(anchor) = self.visual_selection() else {
            return false;
        };
        anchor_contains_line(anchor, line)
    }

    pub fn begin_edit_current_annotation(&mut self, prefer_note: bool) {
        if self.files.is_empty() {
            self.message = Some("add a tracked file before editing annotations".to_string());
            return;
        }
        let note_index = self.current_note_index();
        let question_target = self
            .current_question_index()
            .map(|index| self.latest_question_draft_target(index));
        let choice = if prefer_note {
            note_index
                .map(|index| DraftTarget::EditNote { index })
                .or(question_target)
        } else {
            question_target.or_else(|| note_index.map(|index| DraftTarget::EditNote { index }))
        };

        let Some(target) = choice else {
            self.message =
                Some("there is no editable note or question on the current line".to_string());
            return;
        };

        let draft = match target {
            DraftTarget::EditNote { index } => {
                let note = &self.packet.notes[index];
                PromptDraft {
                    kind: DraftKind::Note,
                    target,
                    path: note.path.clone(),
                    anchor: note.anchor,
                    related_note_ids: Vec::new(),
                    buffer: TextBuffer::from_text(note.body.clone()),
                    original_text: note.body.clone(),
                }
            }
            DraftTarget::EditQuestionPrompt { index } => {
                let question = &self.packet.questions[index];
                PromptDraft {
                    kind: DraftKind::Question,
                    target,
                    path: question.path.clone(),
                    anchor: question
                        .anchor
                        .unwrap_or_else(|| Anchor::new(self.cursor_line, None)),
                    related_note_ids: question.related_note_ids.clone(),
                    buffer: TextBuffer::from_text(question.prompt.clone()),
                    original_text: question.prompt.clone(),
                }
            }
            DraftTarget::EditQuestionMessage {
                question_index,
                message_index,
            } => {
                let question = &self.packet.questions[question_index];
                let message = &question.conversation[message_index];
                PromptDraft {
                    kind: DraftKind::Question,
                    target,
                    path: question.path.clone(),
                    anchor: question
                        .anchor
                        .unwrap_or_else(|| Anchor::new(self.cursor_line, None)),
                    related_note_ids: question.related_note_ids.clone(),
                    buffer: TextBuffer::from_text(message.body.clone()),
                    original_text: message.body.clone(),
                }
            }
            DraftTarget::ContinueQuestion { .. } => {
                unreachable!("edit path cannot pick a continuation target")
            }
            DraftTarget::New => unreachable!("edit path cannot pick a new target"),
        };

        self.draft = Some(draft);
        self.visual_anchor = None;
        self.input_mode = InputMode::Draft;
        self.message =
            Some("edit the annotation; Ctrl-S saves, Ctrl-O opens the external editor".to_string());
    }

    fn begin_new_draft(&mut self, kind: DraftKind) {
        if self.files.is_empty() {
            self.message = Some("add a tracked file before creating annotations".to_string());
            return;
        }
        self.discard_guard = false;
        let anchor = self.selected_anchor();
        let related_note_ids = if kind == DraftKind::Question {
            self.related_note_ids_for_anchor(anchor)
        } else {
            Vec::new()
        };
        self.visual_anchor = None;
        self.draft = Some(PromptDraft {
            kind,
            target: DraftTarget::New,
            path: self.current_path().to_string(),
            anchor,
            related_note_ids,
            buffer: TextBuffer::default(),
            original_text: String::new(),
        });
        self.input_mode = InputMode::Draft;
        self.message = Some(
            match kind {
                DraftKind::Question => {
                    "compose the follow-up question; Ctrl-S saves, Ctrl-O opens the external editor"
                }
                DraftKind::Note => {
                    "write the note body; Ctrl-S saves, Ctrl-O opens the external editor"
                }
            }
            .to_string(),
        );
    }

    pub fn request_close_draft(&mut self) {
        let Some(draft) = &self.draft else {
            return;
        };
        if draft.is_dirty() {
            self.input_mode = InputMode::DraftConfirm;
            self.message = Some("save the draft before closing? y=yes, n=no".to_string());
        } else {
            self.discard_draft();
        }
    }

    pub fn discard_draft(&mut self) {
        self.draft = None;
        self.input_mode = InputMode::Normal;
        self.message = Some("draft closed".to_string());
    }

    pub fn resume_draft(&mut self) {
        if self.draft.is_some() {
            self.input_mode = InputMode::Draft;
            self.message = Some("continue editing the draft".to_string());
        }
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
                line: anchor_display_line(note.anchor),
                label: format!("Note: {}", note.title),
                preview: note.body.clone(),
            })
            .chain(self.packet.questions.iter().filter_map(|question| {
                question.anchor.map(|anchor| SearchMatch {
                    path: question.path.clone(),
                    line: anchor_display_line(anchor),
                    label: format!("Question ({})", question_status_label(question.status)),
                    preview: question_search_preview(question),
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

    pub fn commit_draft(&mut self) -> Result<()> {
        let Some(draft) = self.draft.take() else {
            return Ok(());
        };

        let text = draft.buffer.text.trim().to_string();
        if text.is_empty() {
            self.draft = Some(draft);
            self.message = Some("the draft cannot be empty".to_string());
            return Ok(());
        }

        self.packet.ensure_file(draft.path.clone());
        match draft.target {
            DraftTarget::New => match draft.kind {
                DraftKind::Question => {
                    self.packet.questions.push(Question::new(
                        draft.path.clone(),
                        Some(draft.anchor),
                        text,
                        None,
                        draft.related_note_ids,
                    ));
                }
                DraftKind::Note => {
                    self.packet.notes.push(Note::new(
                        draft.path.clone(),
                        draft.anchor,
                        NoteKind::Overview,
                        note_title_from_text(&text),
                        text,
                        Vec::new(),
                        None,
                        NoteSource::Human,
                    ));
                }
            },
            DraftTarget::ContinueQuestion { index } => {
                if let Some(question) = self.packet.questions.get_mut(index) {
                    question.add_message(QuestionMessageRole::User, text);
                    question.status = QuestionStatus::Open;
                }
            }
            DraftTarget::EditNote { index } => {
                if let Some(note) = self.packet.notes.get_mut(index) {
                    note.path = draft.path.clone();
                    note.anchor = draft.anchor;
                    note.title = note_title_from_text(&text);
                    note.body = text;
                    note.updated_at = Utc::now();
                }
            }
            DraftTarget::EditQuestionPrompt { index } => {
                if let Some(question) = self.packet.questions.get_mut(index) {
                    question.path = draft.path.clone();
                    question.anchor = Some(draft.anchor);
                    question.prompt = text;
                    question.why = None;
                    question.related_note_ids = draft.related_note_ids;
                    question.conversation.clear();
                    question.status = QuestionStatus::Open;
                    question.updated_at = Utc::now();
                }
            }
            DraftTarget::EditQuestionMessage {
                question_index,
                message_index,
            } => {
                if let Some(question) = self.packet.questions.get_mut(question_index) {
                    question.path = draft.path.clone();
                    question.anchor = Some(draft.anchor);
                    question.related_note_ids = draft.related_note_ids;
                    if let Some(message) = question.conversation.get_mut(message_index) {
                        message.body = text;
                        message.updated_at = Utc::now();
                    }
                    question.conversation.truncate(message_index + 1);
                    question.status = QuestionStatus::Open;
                    question.updated_at = Utc::now();
                }
            }
        }

        self.packet.touch();
        self.dirty = true;
        self.input_mode = InputMode::Normal;
        self.message = Some(
            match (draft.kind, draft.target) {
                (DraftKind::Question, DraftTarget::New) => {
                    "question staged; press s to save or x to save and export"
                }
                (DraftKind::Question, DraftTarget::ContinueQuestion { .. }) => {
                    "follow-up staged; press s to save or x to save and export"
                }
                (DraftKind::Note, DraftTarget::New) => {
                    "note staged; press s to save or keep reading"
                }
                (DraftKind::Question, DraftTarget::EditQuestionPrompt { .. })
                | (DraftKind::Question, DraftTarget::EditQuestionMessage { .. }) => {
                    "question updated; press s to save or x to save and export"
                }
                (DraftKind::Note, DraftTarget::EditNote { .. }) => {
                    "note updated; press s to save or keep reading"
                }
                (DraftKind::Question, DraftTarget::EditNote { .. })
                | (DraftKind::Note, DraftTarget::EditQuestionPrompt { .. })
                | (DraftKind::Note, DraftTarget::EditQuestionMessage { .. })
                | (DraftKind::Note, DraftTarget::ContinueQuestion { .. }) => "annotation updated",
            }
            .to_string(),
        );
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
        let export = export::generate_question_export(&self.packet, &self.packet_path)?;
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
        let notice = if self.packet.questions_requiring_reply().count() > 0 {
            let export = export::generate_question_export(&self.packet, &self.packet_path)?;
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
        if self.files.is_empty() {
            let _ = self.begin_file_picker();
        }
        self.message = Some(format!(
            "removed {path} and purged {removed_notes} notes, {removed_questions} questions"
        ));
        true
    }

    pub fn delete_annotation_at_cursor(&mut self) -> bool {
        if let Some(index) = self.current_question_index() {
            let deleted = self.delete_latest_question_turn(index);
            self.packet.touch();
            self.dirty = true;
            self.discard_guard = false;
            self.message = Some(deleted);
            return true;
        }

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

        self.message = Some("no note or question is attached to the current line".to_string());
        false
    }

    fn delete_latest_question_turn(&mut self, index: usize) -> String {
        let Some(message_index) = self
            .packet
            .questions
            .get(index)
            .and_then(latest_user_message_index)
        else {
            let deleted = self.packet.questions.remove(index);
            return format!("deleted question {}", deleted.id);
        };

        let Some(question) = self.packet.questions.get_mut(index) else {
            return "no question is attached to the current line".to_string();
        };

        let removed_count = question.conversation.len().saturating_sub(message_index);
        question.conversation.truncate(message_index);
        question.status = if question.needs_agent_reply() {
            QuestionStatus::Open
        } else {
            QuestionStatus::Answered
        };
        question.updated_at = Utc::now();
        if removed_count == 1 {
            format!("deleted latest follow-up from question {}", question.id)
        } else {
            format!(
                "deleted latest follow-up from question {} and {} dependent repl{}",
                question.id,
                removed_count - 1,
                if removed_count == 2 { "y" } else { "ies" }
            )
        }
    }

    pub fn resolve_current_question(&mut self) -> bool {
        let Some(index) = self.current_open_question_index() else {
            self.message = Some("there is no open question thread on the current line".to_string());
            return false;
        };
        let question_id = {
            let question = &mut self.packet.questions[index];
            question.status = QuestionStatus::Answered;
            question.updated_at = Utc::now();
            question.id.clone()
        };
        self.packet.touch();
        self.dirty = true;
        self.discard_guard = false;
        self.message = Some(format!("resolved question {question_id}"));
        true
    }

    pub fn reopen_current_question(&mut self) -> bool {
        let Some(index) = self.current_question_index() else {
            self.message = Some("there is no question thread on the current line".to_string());
            return false;
        };
        if self.packet.questions[index].status == QuestionStatus::Open {
            self.message = Some("the current question thread is already open".to_string());
            return false;
        }
        let question_id = {
            let question = &mut self.packet.questions[index];
            question.status = QuestionStatus::Open;
            question.updated_at = Utc::now();
            question.id.clone()
        };
        self.packet.touch();
        self.dirty = true;
        self.discard_guard = false;
        self.message = Some(format!("reopened question {question_id}"));
        true
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

    pub fn current_question(&self) -> Option<&Question> {
        self.current_question_index()
            .and_then(|index| self.packet.questions.get(index))
    }

    pub fn current_open_question(&self) -> Option<&Question> {
        self.current_open_question_index()
            .and_then(|index| self.packet.questions.get(index))
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
            .filter(|question| question.path == path)
            .collect()
    }

    fn current_note_index(&self) -> Option<usize> {
        let path = self.current_path();
        self.packet
            .notes
            .iter()
            .rposition(|note| note.path == path && note_covers_line(note, self.cursor_line))
    }

    fn current_question_index(&self) -> Option<usize> {
        let path = self.current_path();
        self.packet.questions.iter().rposition(|question| {
            question.path == path && question_covers_line(question, self.cursor_line)
        })
    }

    fn latest_question_draft_target(&self, question_index: usize) -> DraftTarget {
        match self
            .packet
            .questions
            .get(question_index)
            .and_then(latest_user_message_index)
        {
            Some(message_index) => DraftTarget::EditQuestionMessage {
                question_index,
                message_index,
            },
            None => DraftTarget::EditQuestionPrompt {
                index: question_index,
            },
        }
    }

    fn current_open_question_index(&self) -> Option<usize> {
        let path = self.current_path();
        self.packet.questions.iter().rposition(|question| {
            question.path == path
                && question.status == QuestionStatus::Open
                && question_covers_line(question, self.cursor_line)
        })
    }

    fn selected_anchor(&self) -> Anchor {
        self.visual_selection()
            .unwrap_or_else(|| Anchor::new(self.cursor_line, None))
    }

    fn related_note_ids_for_anchor(&self, anchor: Anchor) -> Vec<String> {
        self.notes_for_path(self.current_path())
            .into_iter()
            .filter(|note| anchors_overlap(note.anchor, anchor))
            .map(|note| note.id.clone())
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

    pub fn from_text(text: String) -> Self {
        let cursor = text.len();
        Self { text, cursor }
    }
}

impl PromptDraft {
    pub fn is_dirty(&self) -> bool {
        self.buffer.text.trim() != self.original_text.trim()
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
    anchor_contains_line(note.anchor, line)
}

fn question_covers_line(question: &Question, line: usize) -> bool {
    let Some(anchor) = question.anchor else {
        return false;
    };
    anchor_contains_line(anchor, line)
}

pub(crate) fn anchor_contains_line(anchor: Anchor, line: usize) -> bool {
    let end = anchor.end_line.unwrap_or(anchor.start_line);
    line >= anchor.start_line && line <= end
}

fn anchors_overlap(left: Anchor, right: Anchor) -> bool {
    let left_end = left.end_line.unwrap_or(left.start_line);
    let right_end = right.end_line.unwrap_or(right.start_line);
    left.start_line <= right_end && right.start_line <= left_end
}

fn anchor_from_lines(start: usize, end: usize) -> Anchor {
    let start_line = start.min(end).max(1);
    let end_line = start.max(end).max(1);
    let maybe_end = (end_line != start_line).then_some(end_line);
    Anchor::new(start_line, maybe_end)
}

pub(crate) fn anchor_display_line(anchor: Anchor) -> usize {
    anchor.end_line.unwrap_or(anchor.start_line)
}

fn note_title_from_text(text: &str) -> String {
    let first_line = text
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("Note");
    let compact = first_line.trim();
    if compact.chars().count() <= 48 {
        compact.to_string()
    } else {
        format!("{}...", compact.chars().take(45).collect::<String>())
    }
}

fn question_search_preview(question: &Question) -> String {
    question
        .conversation
        .last()
        .map(|message| format!("{} {}", message.role.label(), message.body))
        .unwrap_or_else(|| question.prompt.clone())
}

fn question_status_label(status: QuestionStatus) -> &'static str {
    match status {
        QuestionStatus::Open => "open",
        QuestionStatus::Answered => "answered",
        QuestionStatus::Archived => "archived",
    }
}

fn latest_user_message_index(question: &Question) -> Option<usize> {
    question
        .conversation
        .iter()
        .rposition(|message| message.role == QuestionMessageRole::User)
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use crate::model::{Note, NoteKind, NoteSource, Packet, Question, QuestionStatus, TrackedFile};

    use super::{App, DraftKind, FocusPane, InputMode, TextBuffer};
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
        let prompt = app.active_draft_buffer_mut();
        prompt.text = "Why is main empty?".to_string();
        prompt.cursor = prompt.text.len();
        app.commit_draft().unwrap();
        assert_eq!(app.packet.questions.len(), 1);
        assert!(app.dirty);
        assert_eq!(app.packet.questions[0].related_note_ids.len(), 1);
    }

    #[test]
    fn can_start_note_draft() {
        let temp = tempdir().unwrap();
        std::fs::write(temp.path().join("main.rs"), "fn main() {}\n").unwrap();
        let packet = Packet::new(
            "tour",
            "Tour",
            temp.path().display().to_string(),
            vec![TrackedFile::new("main.rs")],
        );
        let mut app = App::load(temp.path().join("tour.toml"), packet, false).unwrap();
        app.begin_note();
        assert_eq!(app.input_mode, InputMode::Draft);
        assert_eq!(
            app.draft.as_ref().map(|draft| draft.kind),
            Some(DraftKind::Note)
        );
        assert_eq!(
            app.draft.as_ref().map(|draft| draft.anchor),
            Some(Anchor::new(1, None))
        );
    }

    #[test]
    fn esc_requests_save_confirmation_for_dirty_draft() {
        let temp = tempdir().unwrap();
        std::fs::write(temp.path().join("main.rs"), "fn main() {}\n").unwrap();
        let packet = Packet::new(
            "tour",
            "Tour",
            temp.path().display().to_string(),
            vec![TrackedFile::new("main.rs")],
        );
        let mut app = App::load(temp.path().join("tour.toml"), packet, false).unwrap();
        app.begin_question();
        let buffer = app.active_draft_buffer_mut();
        buffer.text = "What is this?".to_string();
        buffer.cursor = buffer.text.len();
        app.request_close_draft();
        assert_eq!(app.input_mode, InputMode::DraftConfirm);
    }

    #[test]
    fn can_reopen_and_edit_existing_question() {
        let temp = tempdir().unwrap();
        std::fs::write(temp.path().join("main.rs"), "fn main() {}\n").unwrap();
        let mut packet = Packet::new(
            "tour",
            "Tour",
            temp.path().display().to_string(),
            vec![TrackedFile::new("main.rs")],
        );
        packet.questions.push(Question::new(
            "main.rs",
            Some(Anchor::new(1, None)),
            "Original question?",
            None,
            Vec::new(),
        ));
        let mut app = App::load(temp.path().join("tour.toml"), packet, false).unwrap();
        app.begin_edit_current_annotation(false);
        let buffer = app.active_draft_buffer_mut();
        buffer.text = "Updated question?".to_string();
        buffer.cursor = buffer.text.len();
        app.commit_draft().unwrap();
        assert_eq!(app.packet.questions[0].prompt, "Updated question?");
    }

    #[test]
    fn delete_annotation_prefers_question_threads_before_notes() {
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
        assert!(app.packet.questions.is_empty());
        assert_eq!(app.packet.notes.len(), 1);
        assert!(app.delete_annotation_at_cursor());
        assert!(app.packet.notes.is_empty());
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
        assert_eq!(app.input_mode, InputMode::FilePicker);
        assert!(app.commit_file_picker_selection());
        assert_eq!(app.files.len(), 1);
        assert_eq!(app.current_path(), "src/main.rs");
    }

    #[test]
    fn visual_selection_creates_range_note_anchor() {
        let temp = tempdir().unwrap();
        std::fs::write(temp.path().join("main.rs"), "one\ntwo\nthree\n").unwrap();
        let packet = Packet::new(
            "tour",
            "Tour",
            temp.path().display().to_string(),
            vec![TrackedFile::new("main.rs")],
        );
        let mut app = App::load(temp.path().join("tour.toml"), packet, false).unwrap();
        app.enter_visual_mode();
        app.move_cursor(2);
        app.begin_note();
        let buffer = app.active_draft_buffer_mut();
        buffer.text = "Range note".to_string();
        buffer.cursor = buffer.text.len();
        app.commit_draft().unwrap();
        assert_eq!(app.packet.notes.len(), 1);
        assert_eq!(app.packet.notes[0].anchor, Anchor::new(1, Some(3)));
    }

    #[test]
    fn visual_selection_collects_related_notes_across_the_range() {
        let temp = tempdir().unwrap();
        std::fs::write(temp.path().join("main.rs"), "one\ntwo\nthree\n").unwrap();
        let mut packet = Packet::new(
            "tour",
            "Tour",
            temp.path().display().to_string(),
            vec![TrackedFile::new("main.rs")],
        );
        packet.notes.push(Note::new(
            "main.rs",
            Anchor::new(2, Some(3)),
            NoteKind::Overview,
            "shared range",
            "This overlaps the selected range.",
            vec![],
            None,
            NoteSource::Agent,
        ));
        let note_id = packet.notes[0].id.clone();
        let mut app = App::load(temp.path().join("tour.toml"), packet, false).unwrap();
        app.enter_visual_mode();
        app.move_cursor(2);
        app.begin_question();
        let draft = app.draft.as_ref().expect("question draft should exist");
        assert_eq!(draft.anchor, Anchor::new(1, Some(3)));
        assert_eq!(draft.related_note_ids, vec![note_id]);
    }

    #[test]
    fn continuing_question_appends_user_follow_up() {
        let temp = tempdir().unwrap();
        std::fs::write(temp.path().join("main.rs"), "one\ntwo\nthree\n").unwrap();
        let mut packet = Packet::new(
            "tour",
            "Tour",
            temp.path().display().to_string(),
            vec![TrackedFile::new("main.rs")],
        );
        let mut question = Question::new(
            "main.rs",
            Some(Anchor::new(2, None)),
            "Why is this separate?",
            None,
            vec![],
        );
        question.add_message(
            crate::model::QuestionMessageRole::Agent,
            "It separates setup from the hot path.",
        );
        packet.questions.push(question);

        let mut app = App::load(temp.path().join("tour.toml"), packet, false).unwrap();
        app.cursor_line = 2;
        app.begin_question_follow_up();
        let buffer = app.active_draft_buffer_mut();
        buffer.text = "What invariant depends on that split?".to_string();
        buffer.cursor = buffer.text.len();
        app.commit_draft().unwrap();

        assert_eq!(app.packet.questions[0].conversation.len(), 2);
        assert_eq!(
            app.packet.questions[0].conversation[1].role,
            crate::model::QuestionMessageRole::User
        );
    }

    #[test]
    fn editing_question_targets_latest_user_follow_up() {
        let temp = tempdir().unwrap();
        std::fs::write(temp.path().join("main.rs"), "one\ntwo\nthree\n").unwrap();
        let mut packet = Packet::new(
            "tour",
            "Tour",
            temp.path().display().to_string(),
            vec![TrackedFile::new("main.rs")],
        );
        let mut question = Question::new(
            "main.rs",
            Some(Anchor::new(2, None)),
            "Why is this separate?",
            None,
            vec![],
        );
        question.add_message(
            crate::model::QuestionMessageRole::Agent,
            "It separates setup from the hot path.",
        );
        question.add_message(
            crate::model::QuestionMessageRole::User,
            "What invariant depends on that split?",
        );
        packet.questions.push(question);

        let mut app = App::load(temp.path().join("tour.toml"), packet, false).unwrap();
        app.cursor_line = 2;
        app.begin_edit_current_annotation(false);
        let buffer = app.active_draft_buffer_mut();
        buffer.text = "Which invariant forces that split?".to_string();
        buffer.cursor = buffer.text.len();
        app.commit_draft().unwrap();

        assert_eq!(app.packet.questions[0].prompt, "Why is this separate?");
        assert_eq!(app.packet.questions[0].conversation.len(), 2);
        assert_eq!(
            app.packet.questions[0].conversation[1].body,
            "Which invariant forces that split?"
        );
    }

    #[test]
    fn deleting_question_targets_latest_user_follow_up() {
        let temp = tempdir().unwrap();
        std::fs::write(temp.path().join("main.rs"), "one\ntwo\nthree\n").unwrap();
        let mut packet = Packet::new(
            "tour",
            "Tour",
            temp.path().display().to_string(),
            vec![TrackedFile::new("main.rs")],
        );
        let mut question = Question::new(
            "main.rs",
            Some(Anchor::new(2, None)),
            "Why is this separate?",
            None,
            vec![],
        );
        question.add_message(
            crate::model::QuestionMessageRole::Agent,
            "It separates setup from the hot path.",
        );
        question.add_message(
            crate::model::QuestionMessageRole::User,
            "What invariant depends on that split?",
        );
        question.add_message(
            crate::model::QuestionMessageRole::Agent,
            "The hot path assumes setup has already frozen the scheduler state.",
        );
        packet.questions.push(question);

        let mut app = App::load(temp.path().join("tour.toml"), packet, false).unwrap();
        app.cursor_line = 2;
        assert!(app.delete_annotation_at_cursor());
        assert_eq!(app.packet.questions.len(), 1);
        assert_eq!(app.packet.questions[0].conversation.len(), 1);
        assert_eq!(
            app.packet.questions[0].conversation[0].role,
            crate::model::QuestionMessageRole::Agent
        );
        assert_eq!(app.packet.questions[0].status, QuestionStatus::Answered);
    }

    #[test]
    fn resolving_question_marks_it_answered() {
        let temp = tempdir().unwrap();
        std::fs::write(temp.path().join("main.rs"), "fn main() {}\n").unwrap();
        let mut packet = Packet::new(
            "tour",
            "Tour",
            temp.path().display().to_string(),
            vec![TrackedFile::new("main.rs")],
        );
        packet.questions.push(Question::new(
            "main.rs",
            Some(Anchor::new(1, None)),
            "Why is main empty?",
            None,
            vec![],
        ));
        let mut app = App::load(temp.path().join("tour.toml"), packet, false).unwrap();
        assert!(app.resolve_current_question());
        assert_eq!(app.packet.questions[0].status, QuestionStatus::Answered);
    }

    #[test]
    fn reopening_question_marks_it_open_again() {
        let temp = tempdir().unwrap();
        std::fs::write(temp.path().join("main.rs"), "fn main() {}\n").unwrap();
        let mut packet = Packet::new(
            "tour",
            "Tour",
            temp.path().display().to_string(),
            vec![TrackedFile::new("main.rs")],
        );
        let mut question = Question::new(
            "main.rs",
            Some(Anchor::new(1, None)),
            "Why is main empty?",
            None,
            vec![],
        );
        question.status = QuestionStatus::Answered;
        packet.questions.push(question);
        let mut app = App::load(temp.path().join("tour.toml"), packet, false).unwrap();
        assert!(app.reopen_current_question());
        assert_eq!(app.packet.questions[0].status, QuestionStatus::Open);
    }

    #[test]
    fn next_annotation_wraps_back_to_the_head() {
        let temp = tempdir().unwrap();
        std::fs::write(temp.path().join("main.rs"), "one\ntwo\nthree\nfour\n").unwrap();
        let packet = Packet::new(
            "tour",
            "Tour",
            temp.path().display().to_string(),
            vec![TrackedFile::new("main.rs")],
        );
        let mut app = App::load(temp.path().join("tour.toml"), packet, false).unwrap();
        app.view_metrics.annotation_lines = vec![2, 4];
        app.cursor_line = 4;
        app.jump_to_next_annotation();
        assert_eq!(app.cursor_line, 2);
    }

    #[test]
    fn previous_annotation_wraps_back_to_the_tail() {
        let temp = tempdir().unwrap();
        std::fs::write(temp.path().join("main.rs"), "one\ntwo\nthree\nfour\n").unwrap();
        let packet = Packet::new(
            "tour",
            "Tour",
            temp.path().display().to_string(),
            vec![TrackedFile::new("main.rs")],
        );
        let mut app = App::load(temp.path().join("tour.toml"), packet, false).unwrap();
        app.view_metrics.annotation_lines = vec![2, 4];
        app.cursor_line = 2;
        app.jump_to_previous_annotation();
        assert_eq!(app.cursor_line, 4);
    }
}
