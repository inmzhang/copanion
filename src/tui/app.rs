use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{Context, Result};
use chrono::Utc;
use ignore::WalkBuilder;
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

use crate::clipboard;
use crate::diff::{CommitInfo, DEFAULT_COMMIT_LIMIT, DiffFile, DiffSelection, GitDiffLoader};
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
    ThreadView,
    FilePicker,
    Search,
    Help,
    CommitSelect,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum DraftKind {
    Question,
    Note,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum DraftTarget {
    New,
    EditNote {
        index: usize,
    },
    EditQuestionPrompt {
        index: usize,
    },
    EditQuestionMessage {
        question_index: usize,
        message_index: usize,
    },
    ContinueQuestion {
        index: usize,
    },
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

#[derive(Debug, Clone)]
pub struct ThreadViewState {
    pub question_index: usize,
    pub scroll: usize,
    pub total_rows: usize,
    pub viewport_height: usize,
}

#[derive(Debug, Clone, Default)]
pub struct ViewMetrics {
    pub line_to_row: Vec<usize>,
    pub annotation_lines: Vec<usize>,
    pub total_rows: usize,
    pub viewport_height: usize,
}

#[derive(Debug, Clone, Default)]
pub struct DiffViewMetrics {
    pub file_rows: Vec<usize>,
    pub annotation_rows: Vec<usize>,
    pub rows: Vec<DiffRow>,
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

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum BrowserMode {
    Source,
    Diff,
}

pub struct DiffBrowserState {
    pub loader: GitDiffLoader,
    pub commit_options: Vec<CommitInfo>,
    pub active_review_entries: Vec<CommitInfo>,
    pub commit_cursor: usize,
    pub commit_selection_range: Option<(usize, usize)>,
    pub selection: Option<DiffSelection>,
    pub files: Vec<DiffFile>,
    pub current_file: usize,
    pub cursor_row: usize,
    pub scroll: usize,
    pub view_metrics: DiffViewMetrics,
    pub expanded_gaps: HashSet<GapId>,
    pub expanded_content: HashMap<GapId, Vec<crate::diff::DiffLine>>,
    pub reviewed_paths: BTreeSet<String>,
    pub visual_anchor_row: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GapId {
    pub file_idx: usize,
    pub hunk_idx: usize,
}

#[derive(Debug, Clone)]
pub enum DiffRow {
    FileHeader,
    GapExpander {
        gap_id: GapId,
    },
    ExpandedContext {
        file_idx: usize,
        gap_id: GapId,
        context_idx: usize,
    },
    HunkHeader,
    DiffLine {
        file_idx: usize,
        hunk_idx: usize,
        line_idx: usize,
    },
    Annotation {
        path: String,
        line_no: usize,
    },
    FileEnd,
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
    pub thread_view: Option<ThreadViewState>,
    pub visual_anchor: Option<usize>,
    pub view_metrics: ViewMetrics,
    pub browser_mode: BrowserMode,
    pub diff_browser: Option<DiffBrowserState>,
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
        let mut app = Self::load_base(packet_path, packet, output_to_stdout);

        if app.files.is_empty() {
            app.begin_file_picker()?;
        } else {
            app.focus = FocusPane::Source;
        }

        Ok(app)
    }

    pub fn load_diff(packet_path: PathBuf, packet: Packet, output_to_stdout: bool) -> Result<Self> {
        let mut app = Self::load_base(packet_path, packet, output_to_stdout);
        let diff_browser = DiffBrowserState::new(&app.root)?;
        app.browser_mode = BrowserMode::Diff;
        app.focus = FocusPane::Source;
        app.input_mode = InputMode::CommitSelect;
        app.message = Some(
            "select uncommitted changes or a contiguous commit range, then press Enter".to_string(),
        );
        app.diff_browser = Some(diff_browser);
        Ok(app)
    }

    fn load_base(packet_path: PathBuf, packet: Packet, output_to_stdout: bool) -> Self {
        let root = PathBuf::from(&packet.workspace_root);
        let files = load_files(&root, &packet);
        Self {
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
            thread_view: None,
            visual_anchor: None,
            view_metrics: ViewMetrics::default(),
            browser_mode: BrowserMode::Source,
            diff_browser: None,
            message: None,
            should_quit: false,
            dirty: false,
            discard_guard: false,
            quit_notice: None,
            quit_export: None,
            composer_cursor_screen_pos: None,
        }
    }

    pub fn clear_message(&mut self) {
        self.message = None;
    }

    pub fn is_diff_mode(&self) -> bool {
        self.browser_mode == BrowserMode::Diff
    }

    pub fn current_diff_file(&self) -> Option<&DiffFile> {
        let browser = self.diff_browser.as_ref()?;
        browser.files.get(browser.current_file)
    }

    pub fn current_diff_path(&self) -> Option<&str> {
        self.current_diff_file().map(DiffFile::display_path)
    }

    pub fn current_diff_selection(&self) -> Option<&DiffSelection> {
        self.diff_browser
            .as_ref()
            .and_then(|browser| browser.selection.as_ref())
    }

    pub fn diff_files(&self) -> &[DiffFile] {
        self.diff_browser
            .as_ref()
            .map(|browser| browser.files.as_slice())
            .unwrap_or(&[])
    }

    pub fn current_diff_row(&self) -> Option<&DiffRow> {
        let browser = self.diff_browser.as_ref()?;
        browser.view_metrics.rows.get(browser.cursor_row)
    }

    pub fn is_current_diff_file_reviewed(&self, path: &str) -> bool {
        self.diff_browser
            .as_ref()
            .map(|browser| browser.reviewed_paths.contains(path))
            .unwrap_or(false)
    }

    fn current_annotation_target(&self) -> Option<(String, usize)> {
        if !self.is_diff_mode() {
            let path = self.current_path();
            if path.is_empty() {
                return None;
            }
            return Some((path.to_string(), self.cursor_line));
        }

        self.current_diff_row()
            .and_then(|row| self.annotation_target_for_diff_row(row))
    }

    fn selected_draft_target(&self) -> Option<(String, Anchor)> {
        if !self.is_diff_mode() {
            return Some((
                self.current_path().to_string(),
                self.visual_selection()
                    .unwrap_or_else(|| Anchor::new(self.cursor_line, None)),
            ));
        }

        if self.input_mode == InputMode::Visual
            && let Some((path, anchor)) = self.diff_visual_selection()
        {
            return Some((path, anchor));
        }

        let (path, line) = self.current_annotation_target()?;
        Some((path, Anchor::new(line, None)))
    }

    fn annotation_target_for_diff_row(&self, row: &DiffRow) -> Option<(String, usize)> {
        let browser = self.diff_browser.as_ref()?;
        match row {
            DiffRow::ExpandedContext {
                file_idx,
                gap_id,
                context_idx,
            } => {
                let file = browser.files.get(*file_idx)?;
                file.new_path.as_ref()?;
                let line = browser
                    .expanded_content
                    .get(gap_id)?
                    .get(*context_idx)?
                    .new_lineno?;
                Some((file.display_path().to_string(), line))
            }
            DiffRow::DiffLine {
                file_idx,
                hunk_idx,
                line_idx,
            } => {
                let file = browser.files.get(*file_idx)?;
                file.new_path.as_ref()?;
                let line = file
                    .hunks
                    .get(*hunk_idx)?
                    .lines
                    .get(*line_idx)?
                    .new_lineno?;
                Some((file.display_path().to_string(), line))
            }
            DiffRow::Annotation { path, line_no } => Some((path.clone(), *line_no)),
            _ => None,
        }
    }

    fn diff_visual_selection(&self) -> Option<(String, Anchor)> {
        let browser = self.diff_browser.as_ref()?;
        let start_row = browser.visual_anchor_row?;
        let end_row = browser.cursor_row;
        let start =
            self.annotation_target_for_diff_row(browser.view_metrics.rows.get(start_row)?)?;
        let end = self.annotation_target_for_diff_row(browser.view_metrics.rows.get(end_row)?)?;
        if start.0 != end.0 {
            return None;
        }
        Some((start.0, anchor_from_lines(start.1, end.1)))
    }

    pub fn is_diff_row_in_visual_selection(&self, row: usize) -> bool {
        if !self.is_diff_mode() || self.input_mode != InputMode::Visual {
            return false;
        }
        let Some(browser) = &self.diff_browser else {
            return false;
        };
        let Some(anchor_row) = browser.visual_anchor_row else {
            return false;
        };
        let start = anchor_row.min(browser.cursor_row);
        let end = anchor_row.max(browser.cursor_row);
        row >= start && row <= end
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

    fn has_annotation_context(&self) -> bool {
        if self.is_diff_mode() {
            !self.diff_files().is_empty()
        } else {
            !self.files.is_empty()
        }
    }

    fn clear_visual_selection_state(&mut self) {
        self.visual_anchor = None;
        if let Some(browser) = &mut self.diff_browser {
            browser.visual_anchor_row = None;
        }
    }

    fn current_annotation_line(&self) -> usize {
        self.current_annotation_target()
            .map(|(_, line)| line)
            .unwrap_or(self.cursor_line)
    }

    pub fn toggle_focus(&mut self) {
        self.discard_guard = false;
        let has_files = if self.is_diff_mode() {
            !self.diff_files().is_empty()
        } else {
            !self.files.is_empty()
        };
        if !has_files {
            return;
        }
        self.focus = match self.focus {
            FocusPane::Files => FocusPane::Source,
            FocusPane::Source => FocusPane::Files,
        };
    }

    pub fn move_cursor(&mut self, delta: isize) {
        if self.is_diff_mode() {
            self.move_diff_cursor(delta);
            return;
        }
        self.discard_guard = false;
        let max_line = self.max_source_line();
        let next = (self.cursor_line as isize + delta).clamp(1, max_line as isize) as usize;
        self.cursor_line = next;
        self.ensure_cursor_visible();
    }

    pub fn go_to_first_line(&mut self) {
        if self.is_diff_mode() {
            self.go_to_first_diff_row();
            return;
        }
        self.discard_guard = false;
        self.cursor_line = 1;
        self.ensure_cursor_visible();
    }

    pub fn go_to_last_line(&mut self) {
        if self.is_diff_mode() {
            self.go_to_last_diff_row();
            return;
        }
        self.discard_guard = false;
        self.cursor_line = self.max_source_line();
        self.ensure_cursor_visible();
    }

    fn viewport_step(&self, divisor: usize) -> isize {
        if self.is_diff_mode() {
            return self
                .diff_browser
                .as_ref()
                .map(|browser| (browser.view_metrics.viewport_height / divisor).max(1) as isize)
                .unwrap_or(1);
        }
        (self.view_metrics.viewport_height / divisor).max(1) as isize
    }

    pub fn page_down(&mut self) {
        self.move_cursor(self.viewport_step(1));
    }

    pub fn page_up(&mut self) {
        self.move_cursor(-self.viewport_step(1));
    }

    pub fn half_page_down(&mut self) {
        self.move_cursor(self.viewport_step(2));
    }

    pub fn half_page_up(&mut self) {
        self.move_cursor(-self.viewport_step(2));
    }

    pub fn move_file(&mut self, delta: isize) {
        self.discard_guard = false;
        if self.is_diff_mode() {
            self.move_diff_file(delta);
            return;
        }
        if self.files.is_empty() {
            return;
        }
        let next = (self.current_file as isize + delta)
            .clamp(0, self.files.len().saturating_sub(1) as isize) as usize;
        self.select_file(next);
    }

    pub fn select_file(&mut self, index: usize) {
        if self.is_diff_mode() {
            self.select_diff_file(index);
            return;
        }
        self.current_file = index.min(self.files.len().saturating_sub(1));
        self.cursor_line = 1;
        self.scroll = 0;
        self.ensure_cursor_visible();
    }

    pub fn jump_to_next_annotation(&mut self) {
        if self.is_diff_mode() {
            self.jump_to_next_diff_annotation();
            return;
        }
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
        if self.is_diff_mode() {
            self.jump_to_previous_diff_annotation();
            return;
        }
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

    pub fn begin_question_or_follow_up(&mut self) {
        if self.current_open_question_index().is_some() {
            self.begin_question_follow_up();
        } else {
            self.begin_question();
        }
    }

    pub fn begin_question_follow_up(&mut self) {
        if !self.has_annotation_context() {
            self.message = Some(if self.is_diff_mode() {
                "add a tracked file before continuing a comment".to_string()
            } else {
                "add a tracked file before continuing a question".to_string()
            });
            return;
        }
        let Some(index) = self.current_open_question_index() else {
            self.message = Some(if self.is_diff_mode() {
                "there is no open comment thread on the current line to continue".to_string()
            } else {
                "there is no open question thread on the current line to continue".to_string()
            });
            return;
        };
        let (path, related_note_ids, anchor) = {
            let question = &self.packet.questions[index];
            (
                question.path.clone(),
                question.related_note_ids.clone(),
                question.anchor,
            )
        };
        let current_line = self.current_annotation_line();
        self.discard_guard = false;
        self.clear_visual_selection_state();
        self.draft = Some(PromptDraft {
            kind: DraftKind::Question,
            target: DraftTarget::ContinueQuestion { index },
            path,
            anchor: anchor.unwrap_or_else(|| Anchor::new(current_line, None)),
            related_note_ids,
            buffer: TextBuffer::default(),
            original_text: String::new(),
        });
        self.input_mode = InputMode::Draft;
        self.message = Some(if self.is_diff_mode() {
            "continue the comment thread; Ctrl-S saves, Ctrl-O opens the external editor"
                .to_string()
        } else {
            "continue the conversation; Ctrl-S saves, Ctrl-O opens the external editor".to_string()
        });
    }

    pub fn begin_note(&mut self) {
        self.begin_new_draft(DraftKind::Note);
    }

    pub fn enter_visual_mode(&mut self) {
        if self.is_diff_mode() {
            if self.current_annotation_target().is_none() {
                self.message = Some(
                    "move to an added, unchanged, or expanded-context line before starting a visual selection"
                        .to_string(),
                );
                return;
            }
            let cursor_row = self
                .diff_browser
                .as_ref()
                .map(|browser| browser.cursor_row)
                .unwrap_or(0);
            self.discard_guard = false;
            self.focus = FocusPane::Source;
            self.input_mode = InputMode::Visual;
            if let Some(browser) = &mut self.diff_browser {
                browser.visual_anchor_row = Some(cursor_row);
            }
            self.message =
                Some("visual selection started; move the cursor and press a or n".to_string());
            return;
        }
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
        if let Some(browser) = &mut self.diff_browser {
            browser.visual_anchor_row = None;
        }
        self.message = Some("visual selection cleared".to_string());
    }

    pub fn visual_selection(&self) -> Option<Anchor> {
        if self.input_mode != InputMode::Visual {
            return None;
        }
        if self.is_diff_mode() {
            return self.diff_visual_selection().map(|(_, anchor)| anchor);
        }
        let anchor = self.visual_anchor?;
        Some(anchor_from_lines(anchor, self.cursor_line))
    }

    pub fn is_line_in_visual_selection(&self, line: usize) -> bool {
        if self.is_diff_mode() {
            return false;
        }
        let Some(anchor) = self.visual_selection() else {
            return false;
        };
        anchor_contains_line(anchor, line)
    }

    pub fn begin_edit_current_annotation(&mut self, prefer_note: bool) {
        if !self.has_annotation_context() {
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
            self.message = Some(if self.is_diff_mode() {
                "there is no editable note or comment thread on the current line".to_string()
            } else {
                "there is no editable note or question on the current line".to_string()
            });
            return;
        };
        let current_line = self.current_annotation_line();

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
                        .unwrap_or_else(|| Anchor::new(current_line, None)),
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
                        .unwrap_or_else(|| Anchor::new(current_line, None)),
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
        self.clear_visual_selection_state();
        self.input_mode = InputMode::Draft;
        self.message = Some(
            if self.is_diff_mode() {
                "edit the review comment; Ctrl-S saves, Ctrl-O opens the external editor"
            } else {
                "edit the annotation; Ctrl-S saves, Ctrl-O opens the external editor"
            }
            .to_string(),
        );
    }

    fn begin_new_draft(&mut self, kind: DraftKind) {
        if !self.has_annotation_context() {
            self.message = Some("add a tracked file before creating annotations".to_string());
            return;
        }
        let Some((path, anchor)) = self.selected_draft_target() else {
            self.message = Some(
                "move to a context, added, or unchanged line in the diff before annotating"
                    .to_string(),
            );
            return;
        };
        self.discard_guard = false;
        let related_note_ids = if kind == DraftKind::Question {
            self.related_note_ids_for_anchor(&path, anchor)
        } else {
            Vec::new()
        };
        self.clear_visual_selection_state();
        self.draft = Some(PromptDraft {
            kind,
            target: DraftTarget::New,
            path,
            anchor,
            related_note_ids,
            buffer: TextBuffer::default(),
            original_text: String::new(),
        });
        self.input_mode = InputMode::Draft;
        self.message = Some(
            match (self.is_diff_mode(), kind) {
                (true, DraftKind::Question) => {
                    "compose the review comment; Ctrl-S saves, Ctrl-O opens the external editor"
                }
                (true, DraftKind::Note) => {
                    "write the review note; Ctrl-S saves, Ctrl-O opens the external editor"
                }
                (false, DraftKind::Question) => {
                    "compose the follow-up question; Ctrl-S saves, Ctrl-O opens the external editor"
                }
                (false, DraftKind::Note) => {
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
        let diff_mode = self.is_diff_mode();
        let candidates = if diff_mode {
            let Some(browser) = self.diff_browser.as_ref() else {
                return;
            };
            let mut seen_notes = HashSet::new();
            let mut seen_questions = HashSet::new();
            let mut candidates = Vec::new();

            for row in &browser.view_metrics.rows {
                let DiffRow::Annotation { path, line_no } = row else {
                    continue;
                };

                for note in self
                    .notes_for_path(path)
                    .into_iter()
                    .filter(|note| anchor_display_line(note.anchor) == *line_no)
                {
                    if seen_notes.insert(note.id.clone()) {
                        candidates.push(SearchMatch {
                            path: note.path.clone(),
                            line: *line_no,
                            label: format!("Note: {}", note.title),
                            preview: note.body.clone(),
                        });
                    }
                }

                for question in self
                    .questions_for_path(path)
                    .into_iter()
                    .filter(|question| question.anchor.map(anchor_display_line) == Some(*line_no))
                {
                    if seen_questions.insert(question.id.clone()) {
                        candidates.push(SearchMatch {
                            path: question.path.clone(),
                            line: *line_no,
                            label: format!("Comment ({})", question_status_label(question.status)),
                            preview: question_search_preview(question),
                        });
                    }
                }
            }

            candidates
        } else {
            self.packet
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
                .collect::<Vec<_>>()
        };

        if candidates.is_empty() {
            self.message = Some(if diff_mode {
                "there are no review notes or comment threads to search yet".to_string()
            } else {
                "there are no notes or questions to search yet".to_string()
            });
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
        self.message = Some(if diff_mode {
            "fuzzy-search review notes and comment threads, then press Enter to jump".to_string()
        } else {
            "fuzzy-search notes and questions, then press Enter to jump".to_string()
        });
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
            self.message = Some(if self.is_diff_mode() {
                "no review note or comment thread matches the current search".to_string()
            } else {
                "no note or question matches the current search".to_string()
            });
            return false;
        };

        if self.is_diff_mode() {
            if self.jump_to_diff_annotation_match(&selection.path, selection.line) {
                self.focus = FocusPane::Source;
                self.input_mode = InputMode::Normal;
                self.message = Some(format!("jumped to {}:{}", selection.path, selection.line));
                return true;
            }

            self.input_mode = InputMode::Normal;
            self.message = Some(
                "that note or comment thread is outside the currently visible diff context"
                    .to_string(),
            );
            return false;
        }

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

    fn jump_to_diff_annotation_match(&mut self, path: &str, line: usize) -> bool {
        let Some(browser) = &mut self.diff_browser else {
            return false;
        };
        let Some(file_index) = browser
            .files
            .iter()
            .position(|file| file.display_path() == path)
        else {
            return false;
        };
        let Some(row_index) =
            browser
                .view_metrics
                .rows
                .iter()
                .enumerate()
                .find_map(|(row, kind)| match kind {
                    DiffRow::Annotation {
                        path: row_path,
                        line_no,
                    } if row_path == path && *line_no == line => Some(row),
                    _ => None,
                })
        else {
            return false;
        };
        browser.current_file = file_index;
        browser.cursor_row = row_index;
        self.ensure_cursor_visible();
        self.update_current_diff_file_from_cursor();
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

        self.apply_draft(&draft, text);

        self.packet.touch();
        self.dirty = true;
        self.input_mode = InputMode::Normal;
        self.message = Some(
            self.draft_commit_message(draft.kind, draft.target)
                .to_string(),
        );
        Ok(())
    }

    fn apply_draft(&mut self, draft: &PromptDraft, text: String) {
        self.packet.ensure_file(draft.path.clone());
        match draft.target {
            DraftTarget::New => self.apply_new_draft(draft, text),
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
                    question.related_note_ids = draft.related_note_ids.clone();
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
                    question.related_note_ids = draft.related_note_ids.clone();
                    if let Some(message) = question.conversation.get_mut(message_index) {
                        message.body = text;
                        message.updated_at = Utc::now();
                    }
                    question.status = QuestionStatus::Open;
                    question.updated_at = Utc::now();
                }
            }
        }
    }

    fn apply_new_draft(&mut self, draft: &PromptDraft, text: String) {
        match draft.kind {
            DraftKind::Question => {
                self.packet.questions.push(Question::new(
                    draft.path.clone(),
                    Some(draft.anchor),
                    text,
                    None,
                    draft.related_note_ids.clone(),
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
        }
    }

    fn draft_commit_message(&self, kind: DraftKind, target: DraftTarget) -> &'static str {
        match (self.is_diff_mode(), kind, target) {
            (true, DraftKind::Question, DraftTarget::New) => {
                "comment staged; press s to save or y/x to export the review"
            }
            (true, DraftKind::Question, DraftTarget::ContinueQuestion { .. }) => {
                "comment reply staged; press s to save or y/x to export the review"
            }
            (true, DraftKind::Question, _) => {
                "comment updated; press s to save or y/x to export the review"
            }
            (true, DraftKind::Note, DraftTarget::New) => {
                "review note staged; press s to save or keep reading"
            }
            (true, DraftKind::Note, _) => "review note updated; press s to save or keep reading",
            (false, DraftKind::Question, DraftTarget::New) => {
                "question staged; press s to save or x to save and export"
            }
            (false, DraftKind::Question, DraftTarget::ContinueQuestion { .. }) => {
                "follow-up staged; press s to save or x to save and export"
            }
            (false, DraftKind::Question, _) => {
                "question updated; press s to save or x to save and export"
            }
            (false, DraftKind::Note, DraftTarget::New) => {
                "note staged; press s to save or keep reading"
            }
            (false, DraftKind::Note, _) => "note updated; press s to save or keep reading",
        }
    }

    pub fn save(&mut self) -> Result<()> {
        storage::write_packet(&self.packet_path, &self.packet)?;
        self.dirty = false;
        self.discard_guard = false;
        self.message = Some(format!("saved {}", self.packet_path.display()));
        Ok(())
    }

    pub fn export_questions(&mut self) -> Result<()> {
        let export = match self.question_export_text() {
            Ok(export) => export,
            Err(error) => {
                self.message = Some(error.to_string());
                return Ok(());
            }
        };
        self.discard_guard = false;
        if self.output_to_stdout {
            self.message = Some(if self.is_diff_mode() {
                "diff review rendered to stdout on exit".to_string()
            } else {
                "open questions rendered to stdout on exit".to_string()
            });
            self.quit_export = Some(export);
        } else {
            let message = clipboard::copy_text(&export)?;
            self.message = Some(if self.is_diff_mode() {
                format!("diff review {message}")
            } else {
                format!("open questions {message}")
            });
        }
        Ok(())
    }

    pub fn save_and_quit(&mut self) -> Result<()> {
        self.save()?;
        let notice = match self.question_export_text() {
            Ok(export) => {
                if self.output_to_stdout {
                    self.quit_export = Some(export);
                    format!(
                        "saved {} and wrote the {} to stdout",
                        self.packet_path.display(),
                        if self.is_diff_mode() {
                            "diff review"
                        } else {
                            "open questions"
                        }
                    )
                } else {
                    let copy_result = clipboard::copy_text(&export)?;
                    format!(
                        "saved {} and exported the {} ({copy_result})",
                        self.packet_path.display(),
                        if self.is_diff_mode() {
                            "diff review"
                        } else {
                            "open questions"
                        }
                    )
                }
            }
            Err(_) => {
                format!("saved {}", self.packet_path.display())
            }
        };
        self.quit_notice = Some(notice);
        self.should_quit = true;
        Ok(())
    }

    fn question_export_text(&self) -> Result<String> {
        if self.is_diff_mode()
            && let Some(browser) = &self.diff_browser
            && let Some(selection) = browser.selection.clone()
        {
            return export::generate_review_question_export(
                &self.packet,
                &self.packet_path,
                &export::ReviewExportContext {
                    selection,
                    review_entries: browser.active_review_entries.clone(),
                    changed_paths: browser
                        .files
                        .iter()
                        .map(|file| file.display_path().to_string())
                        .collect(),
                    visible_question_ids: self.visible_diff_review_question_ids(),
                },
            );
        }
        export::generate_question_export(&self.packet, &self.packet_path)
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

        if let Some(index) = self.current_note_index() {
            let deleted = self.packet.notes.remove(index);
            self.packet.touch();
            self.dirty = true;
            self.discard_guard = false;
            self.message = Some(format!("deleted note {}", deleted.title));
            return true;
        }

        self.message = Some(if self.is_diff_mode() {
            "no note or comment thread is attached to the current line".to_string()
        } else {
            "no note or question is attached to the current line".to_string()
        });
        false
    }

    fn delete_latest_question_turn(&mut self, index: usize) -> String {
        let diff_mode = self.is_diff_mode();
        let Some(message_index) = self
            .packet
            .questions
            .get(index)
            .and_then(latest_user_message_index)
        else {
            let deleted = self.packet.questions.remove(index);
            return if diff_mode {
                format!("deleted comment thread {}", deleted.id)
            } else {
                format!("deleted question {}", deleted.id)
            };
        };

        let Some(question) = self.packet.questions.get_mut(index) else {
            return if diff_mode {
                "no comment thread is attached to the current line".to_string()
            } else {
                "no question is attached to the current line".to_string()
            };
        };

        let removed_count = question.conversation.len().saturating_sub(message_index);
        question.conversation.truncate(message_index);
        question.status = if question.needs_agent_reply() {
            QuestionStatus::Open
        } else {
            QuestionStatus::Answered
        };
        question.updated_at = Utc::now();
        let question_id = question.id.clone();
        if removed_count == 1 {
            if diff_mode {
                format!("deleted latest reply from comment thread {question_id}")
            } else {
                format!("deleted latest follow-up from question {question_id}")
            }
        } else if diff_mode {
            format!(
                "deleted latest reply from comment thread {} and {} dependent repl{}",
                question_id,
                removed_count - 1,
                if removed_count == 2 { "y" } else { "ies" }
            )
        } else {
            format!(
                "deleted latest follow-up from question {} and {} dependent repl{}",
                question_id,
                removed_count - 1,
                if removed_count == 2 { "y" } else { "ies" }
            )
        }
    }

    pub fn resolve_current_question(&mut self) -> bool {
        let Some(index) = self.current_open_question_index() else {
            self.message = Some(if self.is_diff_mode() {
                "there is no open comment thread on the current line".to_string()
            } else {
                "there is no open question thread on the current line".to_string()
            });
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
        self.message = Some(if self.is_diff_mode() {
            format!("closed comment thread {question_id}")
        } else {
            format!("closed question {question_id}")
        });
        true
    }

    pub fn reopen_current_question(&mut self) -> bool {
        let Some(index) = self.current_question_index() else {
            self.message = Some(if self.is_diff_mode() {
                "there is no comment thread on the current line".to_string()
            } else {
                "there is no question thread on the current line".to_string()
            });
            return false;
        };
        if self.packet.questions[index].status == QuestionStatus::Open {
            self.message = Some(if self.is_diff_mode() {
                "the current comment thread is already open".to_string()
            } else {
                "the current question thread is already open".to_string()
            });
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
        self.message = Some(if self.is_diff_mode() {
            format!("reopened comment thread {question_id}")
        } else {
            format!("reopened question {question_id}")
        });
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
        if self.is_diff_mode() {
            return self.reload_diff_selection();
        }
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
        if self.is_diff_mode() {
            return;
        }
        metrics.viewport_height = metrics.viewport_height.max(1);
        self.view_metrics = metrics;
        self.ensure_cursor_visible();
    }

    pub fn update_diff_view_metrics(&mut self, mut metrics: DiffViewMetrics) {
        metrics.viewport_height = metrics.viewport_height.max(1);
        if let Some(browser) = &mut self.diff_browser {
            browser.view_metrics = metrics;
        }
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

    pub fn active_thread_view_question(&self) -> Option<&Question> {
        self.thread_view
            .as_ref()
            .and_then(|view| self.packet.questions.get(view.question_index))
    }

    pub fn open_current_thread_viewer(&mut self) -> bool {
        let Some(index) = self.current_question_index() else {
            self.message = Some(if self.is_diff_mode() {
                "there is no comment thread on the current line".to_string()
            } else {
                "there is no question thread on the current line".to_string()
            });
            return false;
        };
        self.thread_view = Some(ThreadViewState {
            question_index: index,
            scroll: usize::MAX,
            total_rows: 0,
            viewport_height: 1,
        });
        self.input_mode = InputMode::ThreadView;
        self.message = Some(if self.is_diff_mode() {
            "readonly comment thread view; j/k scroll, Esc closes".to_string()
        } else {
            "readonly question thread view; j/k scroll, Esc closes".to_string()
        });
        true
    }

    pub fn close_thread_viewer(&mut self) {
        self.thread_view = None;
        self.input_mode = InputMode::Normal;
        self.message = Some(if self.is_diff_mode() {
            "closed the comment thread view".to_string()
        } else {
            "closed the question thread view".to_string()
        });
    }

    pub fn update_thread_view_metrics(&mut self, total_rows: usize, viewport_height: usize) {
        let Some(view) = &mut self.thread_view else {
            return;
        };
        view.total_rows = total_rows;
        view.viewport_height = viewport_height.max(1);
        let max_scroll = view.total_rows.saturating_sub(view.viewport_height);
        view.scroll = view.scroll.min(max_scroll);
    }

    pub fn thread_view_scroll(&self) -> usize {
        self.thread_view
            .as_ref()
            .map(|view| view.scroll)
            .unwrap_or(0)
    }

    pub fn scroll_thread_view(&mut self, delta: isize) {
        let Some(view) = &mut self.thread_view else {
            return;
        };
        let max_scroll = view.total_rows.saturating_sub(view.viewport_height) as isize;
        view.scroll = (view.scroll as isize + delta).clamp(0, max_scroll) as usize;
    }

    pub fn page_thread_view_down(&mut self) {
        let step = self
            .thread_view
            .as_ref()
            .map(|view| view.viewport_height.max(1))
            .unwrap_or(1);
        self.scroll_thread_view(step as isize);
    }

    pub fn page_thread_view_up(&mut self) {
        let step = self
            .thread_view
            .as_ref()
            .map(|view| view.viewport_height.max(1))
            .unwrap_or(1);
        self.scroll_thread_view(-(step as isize));
    }

    pub fn half_page_thread_view_down(&mut self) {
        let step = self
            .thread_view
            .as_ref()
            .map(|view| (view.viewport_height / 2).max(1))
            .unwrap_or(1);
        self.scroll_thread_view(step as isize);
    }

    pub fn half_page_thread_view_up(&mut self) {
        let step = self
            .thread_view
            .as_ref()
            .map(|view| (view.viewport_height / 2).max(1))
            .unwrap_or(1);
        self.scroll_thread_view(-(step as isize));
    }

    pub fn thread_view_to_top(&mut self) {
        if let Some(view) = &mut self.thread_view {
            view.scroll = 0;
        }
    }

    pub fn thread_view_to_bottom(&mut self) {
        if let Some(view) = &mut self.thread_view {
            view.scroll = view.total_rows.saturating_sub(view.viewport_height);
        }
    }

    pub fn notes_for_current_line(&self) -> Vec<&Note> {
        let Some((path, line)) = self.current_annotation_target() else {
            return Vec::new();
        };
        self.packet
            .notes
            .iter()
            .filter(|note| note.path == path && note_covers_line(note, line))
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
        let (path, line) = self.current_annotation_target()?;
        self.packet
            .notes
            .iter()
            .rposition(|note| note.path == path && note_covers_line(note, line))
    }

    fn current_question_index(&self) -> Option<usize> {
        let (path, line) = self.current_annotation_target()?;
        self.packet
            .questions
            .iter()
            .rposition(|question| question.path == path && question_covers_line(question, line))
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
        let (path, line) = self.current_annotation_target()?;
        self.packet.questions.iter().rposition(|question| {
            question.path == path
                && question.status == QuestionStatus::Open
                && question_covers_line(question, line)
        })
    }

    fn related_note_ids_for_anchor(&self, path: &str, anchor: Anchor) -> Vec<String> {
        self.notes_for_path(path)
            .into_iter()
            .filter(|note| anchors_overlap(note.anchor, anchor))
            .map(|note| note.id.clone())
            .collect()
    }

    fn ensure_cursor_visible(&mut self) {
        if self.is_diff_mode() {
            self.ensure_diff_cursor_visible();
            return;
        }
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

impl DiffBrowserState {
    fn new(root: &Path) -> Result<Self> {
        let loader = GitDiffLoader::discover(root)?;
        let commit_options = loader.selection_options(DEFAULT_COMMIT_LIMIT)?;
        Ok(Self {
            loader,
            commit_options,
            active_review_entries: Vec::new(),
            commit_cursor: 0,
            commit_selection_range: Some((0, 0)),
            selection: None,
            files: Vec::new(),
            current_file: 0,
            cursor_row: 0,
            scroll: 0,
            view_metrics: DiffViewMetrics::default(),
            expanded_gaps: HashSet::new(),
            expanded_content: HashMap::new(),
            reviewed_paths: BTreeSet::new(),
            visual_anchor_row: None,
        })
    }
}

impl App {
    pub fn reopen_diff_commit_selector(&mut self) -> Result<()> {
        let Some(browser) = &mut self.diff_browser else {
            return Ok(());
        };
        let previous_options = browser.commit_options.clone();
        let previous_cursor = browser.commit_cursor;
        let previous_selection = browser.commit_selection_range;
        let refreshed_options = browser.loader.selection_options(DEFAULT_COMMIT_LIMIT)?;
        let (restored_cursor, restored_selection) = restore_commit_selector_state(
            &previous_options,
            previous_cursor,
            previous_selection,
            &refreshed_options,
        );
        browser.commit_options = refreshed_options;
        browser.commit_cursor = restored_cursor;
        browser.commit_selection_range = restored_selection;
        self.input_mode = InputMode::CommitSelect;
        self.message = Some(
            "select uncommitted changes or a contiguous commit range, then press Enter".to_string(),
        );
        Ok(())
    }

    pub fn move_diff_commit_cursor(&mut self, delta: isize) {
        let Some(browser) = &mut self.diff_browser else {
            return;
        };
        if browser.commit_options.is_empty() {
            browser.commit_cursor = 0;
            return;
        }
        browser.commit_cursor = (browser.commit_cursor as isize + delta)
            .clamp(0, browser.commit_options.len().saturating_sub(1) as isize)
            as usize;
    }

    pub fn toggle_diff_commit_selection(&mut self) {
        let Some(browser) = &mut self.diff_browser else {
            return;
        };
        if browser.commit_options.is_empty() {
            return;
        }

        let cursor = browser.commit_cursor;
        match browser.commit_selection_range {
            None => browser.commit_selection_range = Some((cursor, cursor)),
            Some((start, end)) => {
                if cursor < start {
                    browser.commit_selection_range = Some((cursor, end));
                } else if cursor > end {
                    browser.commit_selection_range = Some((start, cursor));
                } else if start == end {
                    browser.commit_selection_range = None;
                } else if cursor == start {
                    browser.commit_selection_range = Some((start + 1, end));
                } else if cursor == end {
                    browser.commit_selection_range = Some((start, end - 1));
                } else {
                    browser.commit_selection_range = Some((start, cursor));
                }
            }
        }
    }

    pub fn confirm_diff_commit_selection(&mut self) -> Result<()> {
        let Some(browser) = &mut self.diff_browser else {
            return Ok(());
        };
        let Some((start, end)) = browser.commit_selection_range else {
            self.message = Some("select at least one entry before opening the diff".to_string());
            return Ok(());
        };
        let Some(selected_slice) = browser.commit_options.get(start..=end) else {
            self.message = Some("the current commit selection is invalid".to_string());
            return Ok(());
        };
        browser.active_review_entries = selected_slice.to_vec();

        let includes_working_tree = selected_slice
            .first()
            .map(CommitInfo::is_working_tree)
            .unwrap_or(false);
        let commit_ids = selected_slice
            .iter()
            .filter(|entry| !entry.is_working_tree())
            .map(|entry| entry.id.clone())
            .rev()
            .collect::<Vec<_>>();

        let selection = if includes_working_tree {
            if commit_ids.is_empty() {
                DiffSelection::WorkingTree
            } else {
                DiffSelection::WorkingTreeAndCommits(commit_ids)
            }
        } else {
            DiffSelection::CommitRange(commit_ids)
        };

        browser.files = browser.loader.diff_for_selection(&selection)?;
        browser.selection = Some(selection);
        browser.current_file = 0;
        browser.cursor_row = 0;
        browser.scroll = 0;
        browser.view_metrics = DiffViewMetrics::default();
        browser.expanded_gaps.clear();
        browser.expanded_content.clear();
        browser.visual_anchor_row = None;
        self.input_mode = InputMode::Normal;
        self.focus = FocusPane::Source;
        self.message = Some("diff loaded".to_string());
        Ok(())
    }

    fn reload_diff_selection(&mut self) -> Result<()> {
        let Some(browser) = &mut self.diff_browser else {
            return Ok(());
        };
        let Some(selection) = browser.selection.clone() else {
            self.message = Some("choose a diff range first from the commit selector".to_string());
            return Ok(());
        };
        browser.files = browser.loader.diff_for_selection(&selection)?;
        browser.current_file = browser
            .current_file
            .min(browser.files.len().saturating_sub(1));
        browser.cursor_row = 0;
        browser.scroll = 0;
        browser.view_metrics = DiffViewMetrics::default();
        browser.expanded_gaps.clear();
        browser.expanded_content.clear();
        browser.visual_anchor_row = None;
        self.message = Some("reloaded diff selection".to_string());
        Ok(())
    }

    fn move_diff_file(&mut self, delta: isize) {
        let Some(browser) = &mut self.diff_browser else {
            return;
        };
        if browser.files.is_empty() {
            return;
        }
        let next = (browser.current_file as isize + delta)
            .clamp(0, browser.files.len().saturating_sub(1) as isize) as usize;
        self.select_diff_file(next);
    }

    fn select_diff_file(&mut self, index: usize) {
        let Some(browser) = &mut self.diff_browser else {
            return;
        };
        if browser.files.is_empty() {
            browser.current_file = 0;
            browser.cursor_row = 0;
            browser.scroll = 0;
            return;
        }
        browser.current_file = index.min(browser.files.len().saturating_sub(1));
        browser.cursor_row = browser
            .view_metrics
            .file_rows
            .get(browser.current_file)
            .copied()
            .unwrap_or(0);
        self.ensure_cursor_visible();
    }

    fn move_diff_cursor(&mut self, delta: isize) {
        let Some(browser) = &mut self.diff_browser else {
            return;
        };
        if browser.view_metrics.total_rows == 0 {
            browser.cursor_row = 0;
            browser.scroll = 0;
            return;
        }
        browser.cursor_row = (browser.cursor_row as isize + delta).clamp(
            0,
            browser.view_metrics.total_rows.saturating_sub(1) as isize,
        ) as usize;
        self.ensure_cursor_visible();
        self.update_current_diff_file_from_cursor();
    }

    fn go_to_first_diff_row(&mut self) {
        let Some(browser) = &mut self.diff_browser else {
            return;
        };
        browser.cursor_row = 0;
        self.ensure_cursor_visible();
        self.update_current_diff_file_from_cursor();
    }

    fn go_to_last_diff_row(&mut self) {
        let Some(browser) = &mut self.diff_browser else {
            return;
        };
        browser.cursor_row = browser.view_metrics.total_rows.saturating_sub(1);
        self.ensure_cursor_visible();
        self.update_current_diff_file_from_cursor();
    }

    fn jump_to_next_diff_annotation(&mut self) {
        let Some(browser) = &mut self.diff_browser else {
            return;
        };
        if let Some(row) = browser
            .view_metrics
            .annotation_rows
            .iter()
            .copied()
            .find(|row| *row > browser.cursor_row)
            .or_else(|| browser.view_metrics.annotation_rows.first().copied())
        {
            browser.cursor_row = row;
            self.ensure_cursor_visible();
            self.update_current_diff_file_from_cursor();
        } else {
            self.message = Some("there are no notes or comment threads in this diff".to_string());
        }
    }

    fn jump_to_previous_diff_annotation(&mut self) {
        let Some(browser) = &mut self.diff_browser else {
            return;
        };
        if let Some(row) = browser
            .view_metrics
            .annotation_rows
            .iter()
            .copied()
            .rev()
            .find(|row| *row < browser.cursor_row)
            .or_else(|| browser.view_metrics.annotation_rows.last().copied())
        {
            browser.cursor_row = row;
            self.ensure_cursor_visible();
            self.update_current_diff_file_from_cursor();
        } else {
            self.message = Some("there are no notes or comment threads in this diff".to_string());
        }
    }

    fn ensure_diff_cursor_visible(&mut self) {
        let Some(browser) = &mut self.diff_browser else {
            return;
        };
        if browser.view_metrics.total_rows == 0 {
            browser.scroll = 0;
            return;
        }
        let viewport_height = browser.view_metrics.viewport_height.max(1);
        if browser.cursor_row < browser.scroll {
            browser.scroll = browser.cursor_row;
        } else if browser.cursor_row >= browser.scroll + viewport_height {
            browser.scroll = browser
                .cursor_row
                .saturating_add(1)
                .saturating_sub(viewport_height);
        }
        let max_scroll = browser
            .view_metrics
            .total_rows
            .saturating_sub(viewport_height);
        browser.scroll = browser.scroll.min(max_scroll);
    }

    fn update_current_diff_file_from_cursor(&mut self) {
        let Some(browser) = &mut self.diff_browser else {
            return;
        };
        if browser.view_metrics.file_rows.is_empty() {
            browser.current_file = 0;
            return;
        }
        browser.current_file = browser
            .view_metrics
            .file_rows
            .iter()
            .enumerate()
            .filter(|(_, row)| **row <= browser.cursor_row)
            .map(|(index, _)| index)
            .next_back()
            .unwrap_or(0);
    }

    pub fn toggle_diff_gap_at_cursor(&mut self) -> Result<()> {
        let Some(row) = self.current_diff_row().cloned() else {
            return Ok(());
        };
        match row {
            DiffRow::GapExpander { gap_id } => {
                if self
                    .diff_browser
                    .as_ref()
                    .is_some_and(|browser| browser.expanded_gaps.contains(&gap_id))
                {
                    self.collapse_diff_gap(gap_id);
                } else {
                    self.expand_diff_gap(gap_id)?;
                }
            }
            DiffRow::ExpandedContext { gap_id, .. } => {
                self.collapse_diff_gap(gap_id);
            }
            _ => {
                self.message =
                    Some("move to a collapsed or expanded context row first".to_string());
            }
        }
        Ok(())
    }

    fn expand_diff_gap(&mut self, gap_id: GapId) -> Result<()> {
        let Some(browser) = &mut self.diff_browser else {
            return Ok(());
        };
        if browser.expanded_gaps.contains(&gap_id) {
            return Ok(());
        }
        let file = browser
            .files
            .get(gap_id.file_idx)
            .context("invalid diff file index for context expansion")?;
        let hunk = file
            .hunks
            .get(gap_id.hunk_idx)
            .context("invalid diff hunk index for context expansion")?;
        let previous_hunk = gap_id
            .hunk_idx
            .checked_sub(1)
            .and_then(|index| file.hunks.get(index));
        let (start_line, end_line) = match previous_hunk {
            None => (1, hunk.new_start.saturating_sub(1)),
            Some(previous) => (
                previous.new_start.saturating_add(previous.new_count),
                hunk.new_start.saturating_sub(1),
            ),
        };
        if start_line > end_line {
            return Ok(());
        }
        let lines = browser
            .loader
            .fetch_context_lines(file, start_line, end_line)?;
        browser.expanded_content.insert(gap_id.clone(), lines);
        browser.expanded_gaps.insert(gap_id);
        self.message = Some("expanded hidden context".to_string());
        Ok(())
    }

    fn collapse_diff_gap(&mut self, gap_id: GapId) {
        let Some(browser) = &mut self.diff_browser else {
            return;
        };
        browser.expanded_gaps.remove(&gap_id);
        browser.expanded_content.remove(&gap_id);
        self.message = Some("collapsed expanded context".to_string());
    }

    pub fn mark_current_diff_file_reviewed_and_next(&mut self) {
        let Some(browser) = &mut self.diff_browser else {
            return;
        };
        let Some(path) = browser
            .files
            .get(browser.current_file)
            .map(|file| file.display_path().to_string())
        else {
            return;
        };
        browser.reviewed_paths.insert(path.clone());
        let current = browser.current_file;
        if current + 1 < browser.files.len() {
            self.select_diff_file(current + 1);
            self.message = Some(format!("marked {path} reviewed and moved to the next file"));
        } else {
            self.message = Some(format!("marked {path} reviewed"));
        }
    }

    fn visible_diff_review_question_ids(&self) -> Vec<String> {
        let Some(browser) = self.diff_browser.as_ref() else {
            return Vec::new();
        };
        let mut seen = HashSet::new();
        let mut ids = Vec::new();

        for row in &browser.view_metrics.rows {
            let DiffRow::Annotation { path, line_no } = row else {
                continue;
            };

            for question in self
                .questions_for_path(path)
                .into_iter()
                .filter(|question| question.anchor.map(anchor_display_line) == Some(*line_no))
                .filter(|question| question.needs_agent_reply())
            {
                if seen.insert(question.id.clone()) {
                    ids.push(question.id.clone());
                }
            }
        }

        ids
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

fn restore_commit_selector_state(
    previous_options: &[CommitInfo],
    previous_cursor: usize,
    previous_selection: Option<(usize, usize)>,
    refreshed_options: &[CommitInfo],
) -> (usize, Option<(usize, usize)>) {
    if refreshed_options.is_empty() {
        return (0, None);
    }

    let previous_cursor_id = previous_options
        .get(previous_cursor)
        .map(|entry| entry.id.as_str());
    let matched_selection_indices = previous_selection
        .and_then(|(start, end)| previous_options.get(start..=end))
        .map(|selected_entries| {
            selected_entries
                .iter()
                .filter_map(|entry| {
                    refreshed_options
                        .iter()
                        .position(|candidate| candidate.id == entry.id)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let restored_selection = if matched_selection_indices.is_empty() {
        None
    } else {
        Some((
            *matched_selection_indices
                .iter()
                .min()
                .expect("matched selection indices should not be empty"),
            *matched_selection_indices
                .iter()
                .max()
                .expect("matched selection indices should not be empty"),
        ))
    };

    let restored_cursor = previous_cursor_id
        .and_then(|id| refreshed_options.iter().position(|entry| entry.id == id))
        .or_else(|| restored_selection.map(|(_, end)| end))
        .unwrap_or(0);

    (restored_cursor, restored_selection)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use chrono::Utc;
    use tempfile::{TempDir, tempdir};

    use crate::diff::CommitInfo;
    use crate::model::{Note, NoteKind, NoteSource, Packet, Question, QuestionStatus, TrackedFile};

    use super::{
        App, DraftKind, DraftTarget, FocusPane, InputMode, TextBuffer,
        restore_commit_selector_state,
    };
    use crate::model::Anchor;

    fn write_workspace_file(root: &Path, relative_path: &str, contents: &str) {
        let absolute = root.join(relative_path);
        if let Some(parent) = absolute.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(absolute, contents).unwrap();
    }

    fn source_app(
        files: &[(&str, &str)],
        configure_packet: impl FnOnce(&mut Packet),
    ) -> (TempDir, App) {
        let temp = tempdir().unwrap();
        let tracked_files = files
            .iter()
            .map(|(path, contents)| {
                write_workspace_file(temp.path(), path, contents);
                TrackedFile::new(*path)
            })
            .collect();
        let mut packet = Packet::new(
            "tour",
            "Tour",
            temp.path().display().to_string(),
            tracked_files,
        );
        configure_packet(&mut packet);
        let app = App::load(temp.path().join("tour.toml"), packet, false).unwrap();
        (temp, app)
    }

    fn single_file_app(
        contents: &str,
        configure_packet: impl FnOnce(&mut Packet),
    ) -> (TempDir, App) {
        source_app(&[("main.rs", contents)], configure_packet)
    }

    fn fake_commit(id: &str) -> CommitInfo {
        CommitInfo {
            id: id.to_string(),
            short_id: id.to_string(),
            branch_name: None,
            summary: format!("commit {id}"),
            body: None,
            author: "Test".to_string(),
            time: Utc::now(),
        }
    }

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
        let (_temp, mut app) = single_file_app("fn main() {}\n", |packet| {
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
        });
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
        let (_temp, mut app) = single_file_app("fn main() {}\n", |_| {});
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
        let (_temp, mut app) = single_file_app("fn main() {}\n", |_| {});
        app.begin_question();
        let buffer = app.active_draft_buffer_mut();
        buffer.text = "What is this?".to_string();
        buffer.cursor = buffer.text.len();
        app.request_close_draft();
        assert_eq!(app.input_mode, InputMode::DraftConfirm);
    }

    #[test]
    fn can_reopen_and_edit_existing_question() {
        let (_temp, mut app) = single_file_app("fn main() {}\n", |packet| {
            packet.questions.push(Question::new(
                "main.rs",
                Some(Anchor::new(1, None)),
                "Original question?",
                None,
                Vec::new(),
            ));
        });
        app.begin_edit_current_annotation(false);
        let buffer = app.active_draft_buffer_mut();
        buffer.text = "Updated question?".to_string();
        buffer.cursor = buffer.text.len();
        app.commit_draft().unwrap();
        assert_eq!(app.packet.questions[0].prompt, "Updated question?");
    }

    #[test]
    fn editing_prompt_preserves_existing_conversation() {
        let (_temp, mut app) = single_file_app("fn main() {}\n", |packet| {
            let mut question = Question::new(
                "main.rs",
                Some(Anchor::new(1, None)),
                "Original question?",
                None,
                Vec::new(),
            );
            question.add_message(
                crate::model::QuestionMessageRole::Agent,
                "This already has an answer.",
            );
            packet.questions.push(question);
        });
        app.begin_edit_current_annotation(false);
        let buffer = app.active_draft_buffer_mut();
        buffer.text = "Updated question?".to_string();
        buffer.cursor = buffer.text.len();
        app.commit_draft().unwrap();

        assert_eq!(app.packet.questions[0].prompt, "Updated question?");
        assert_eq!(app.packet.questions[0].conversation.len(), 1);
        assert_eq!(
            app.packet.questions[0].conversation[0].body,
            "This already has an answer."
        );
    }

    #[test]
    fn delete_annotation_prefers_question_threads_before_notes() {
        let (_temp, mut app) = single_file_app("fn main() {}\n", |packet| {
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
        });
        assert!(app.delete_annotation_at_cursor());
        assert!(app.packet.questions.is_empty());
        assert_eq!(app.packet.notes.len(), 1);
        assert!(app.delete_annotation_at_cursor());
        assert!(app.packet.notes.is_empty());
    }

    #[test]
    fn deleting_current_file_purges_related_state() {
        let (_temp, mut app) = source_app(
            &[
                ("src/main.rs", "fn main() {}\n"),
                ("src/lib.rs", "pub fn helper() {}\n"),
            ],
            |packet| {
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
            },
        );
        assert!(app.delete_current_file());
        assert_eq!(app.files.len(), 1);
        assert_eq!(app.current_path(), "src/lib.rs");
        assert!(app.packet.notes.is_empty());
        assert!(app.packet.questions.is_empty());
    }

    #[test]
    fn empty_session_opens_file_picker_and_adds_file() {
        let temp = tempdir().unwrap();
        write_workspace_file(temp.path(), "src/main.rs", "fn main() {}\n");
        let packet = Packet::new("tour", "Tour", temp.path().display().to_string(), vec![]);
        let mut app = App::load(temp.path().join("tour.toml"), packet, false).unwrap();
        assert_eq!(app.input_mode, InputMode::FilePicker);
        assert!(app.commit_file_picker_selection());
        assert_eq!(app.files.len(), 1);
        assert_eq!(app.current_path(), "src/main.rs");
    }

    #[test]
    fn half_page_navigation_moves_by_half_the_viewport_height() {
        let (_temp, mut app) = single_file_app(
            &(1..=20)
                .map(|line| format!("line {line}\n"))
                .collect::<String>(),
            |_| {},
        );
        app.view_metrics.viewport_height = 6;

        app.half_page_down();
        assert_eq!(app.cursor_line, 4);

        app.half_page_up();
        assert_eq!(app.cursor_line, 1);
    }

    #[test]
    fn half_page_navigation_uses_a_minimum_step_of_one_line() {
        let (_temp, mut app) = single_file_app("one\ntwo\n", |_| {});
        app.view_metrics.viewport_height = 1;

        app.half_page_down();
        assert_eq!(app.cursor_line, 2);

        app.half_page_up();
        assert_eq!(app.cursor_line, 1);
    }

    #[test]
    fn visual_selection_creates_range_note_anchor() {
        let (_temp, mut app) = single_file_app("one\ntwo\nthree\n", |_| {});
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
        let mut note_id = String::new();
        let (_temp, mut app) = single_file_app("one\ntwo\nthree\n", |packet| {
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
            note_id = packet.notes[0].id.clone();
        });
        app.enter_visual_mode();
        app.move_cursor(2);
        app.begin_question();
        let draft = app.draft.as_ref().expect("question draft should exist");
        assert_eq!(draft.anchor, Anchor::new(1, Some(3)));
        assert_eq!(draft.related_note_ids, vec![note_id]);
    }

    #[test]
    fn continuing_question_appends_user_follow_up() {
        let (_temp, mut app) = single_file_app("one\ntwo\nthree\n", |packet| {
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
        });
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
    fn begin_question_or_follow_up_prefers_open_thread_under_cursor() {
        let (_temp, mut app) = single_file_app("one\ntwo\nthree\n", |packet| {
            packet.questions.push(Question::new(
                "main.rs",
                Some(Anchor::new(2, None)),
                "Why is this separate?",
                None,
                vec![],
            ));
        });
        app.cursor_line = 2;
        app.begin_question_or_follow_up();

        let draft = app.draft.as_ref().expect("question draft should exist");
        assert_eq!(draft.target, DraftTarget::ContinueQuestion { index: 0 });
        assert_eq!(app.input_mode, InputMode::Draft);
    }

    #[test]
    fn can_open_question_in_readonly_thread_view() {
        let (_temp, mut app) = single_file_app("one\ntwo\nthree\n", |packet| {
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
        });
        app.cursor_line = 2;

        assert!(app.open_current_thread_viewer());
        assert_eq!(app.input_mode, InputMode::ThreadView);
        assert_eq!(
            app.active_thread_view_question()
                .map(|question| question.prompt.as_str()),
            Some("Why is this separate?")
        );

        app.update_thread_view_metrics(20, 6);
        assert_eq!(app.thread_view_scroll(), 14);
        app.scroll_thread_view(-3);
        assert_eq!(app.thread_view_scroll(), 11);

        app.close_thread_viewer();
        assert_eq!(app.input_mode, InputMode::Normal);
        assert!(app.thread_view.is_none());
    }

    #[test]
    fn editing_question_targets_latest_user_follow_up() {
        let (_temp, mut app) = single_file_app("one\ntwo\nthree\n", |packet| {
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
        });
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
        let (_temp, mut app) = single_file_app("one\ntwo\nthree\n", |packet| {
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
        });
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
    fn editing_user_follow_up_preserves_later_agent_reply() {
        let (_temp, mut app) = single_file_app("one\ntwo\nthree\n", |packet| {
            let mut question = Question::new(
                "main.rs",
                Some(Anchor::new(2, None)),
                "Why is this separate?",
                None,
                vec![],
            );
            question.add_message(
                crate::model::QuestionMessageRole::User,
                "What invariant depends on that split?",
            );
            question.add_message(
                crate::model::QuestionMessageRole::Agent,
                "The scheduler is already frozen by then.",
            );
            packet.questions.push(question);
        });
        app.cursor_line = 2;
        app.begin_edit_current_annotation(false);
        let buffer = app.active_draft_buffer_mut();
        buffer.text = "Which invariant depends on that split?".to_string();
        buffer.cursor = buffer.text.len();
        app.commit_draft().unwrap();

        assert_eq!(app.packet.questions[0].conversation.len(), 2);
        assert_eq!(
            app.packet.questions[0].conversation[0].body,
            "Which invariant depends on that split?"
        );
        assert_eq!(
            app.packet.questions[0].conversation[1].body,
            "The scheduler is already frozen by then."
        );
    }

    #[test]
    fn resolving_question_marks_it_answered() {
        let (_temp, mut app) = single_file_app("fn main() {}\n", |packet| {
            packet.questions.push(Question::new(
                "main.rs",
                Some(Anchor::new(1, None)),
                "Why is main empty?",
                None,
                vec![],
            ));
        });
        assert!(app.resolve_current_question());
        assert_eq!(app.packet.questions[0].status, QuestionStatus::Answered);
    }

    #[test]
    fn reopening_question_marks_it_open_again() {
        let (_temp, mut app) = single_file_app("fn main() {}\n", |packet| {
            let mut question = Question::new(
                "main.rs",
                Some(Anchor::new(1, None)),
                "Why is main empty?",
                None,
                vec![],
            );
            question.status = QuestionStatus::Answered;
            packet.questions.push(question);
        });
        assert!(app.reopen_current_question());
        assert_eq!(app.packet.questions[0].status, QuestionStatus::Open);
    }

    #[test]
    fn next_annotation_wraps_back_to_the_head() {
        let (_temp, mut app) = single_file_app("one\ntwo\nthree\nfour\n", |_| {});
        app.view_metrics.annotation_lines = vec![2, 4];
        app.cursor_line = 4;
        app.jump_to_next_annotation();
        assert_eq!(app.cursor_line, 2);
    }

    #[test]
    fn previous_annotation_wraps_back_to_the_tail() {
        let (_temp, mut app) = single_file_app("one\ntwo\nthree\nfour\n", |_| {});
        app.view_metrics.annotation_lines = vec![2, 4];
        app.cursor_line = 2;
        app.jump_to_previous_annotation();
        assert_eq!(app.cursor_line, 4);
    }

    #[test]
    fn restoring_commit_selector_state_preserves_selected_range_and_cursor() {
        let previous = vec![
            CommitInfo::working_tree_entry(),
            fake_commit("c3"),
            fake_commit("c2"),
            fake_commit("c1"),
        ];
        let refreshed = vec![
            CommitInfo::working_tree_entry(),
            fake_commit("c4"),
            fake_commit("c3"),
            fake_commit("c2"),
            fake_commit("c1"),
        ];

        let (cursor, selection) =
            restore_commit_selector_state(&previous, 2, Some((0, 2)), &refreshed);

        assert_eq!(cursor, 3);
        assert_eq!(selection, Some((0, 3)));
    }

    #[test]
    fn restoring_commit_selector_state_drops_missing_worktree_entry_but_keeps_commits() {
        let previous = vec![
            CommitInfo::working_tree_entry(),
            fake_commit("c3"),
            fake_commit("c2"),
        ];
        let refreshed = vec![fake_commit("c4"), fake_commit("c3"), fake_commit("c2")];

        let (cursor, selection) =
            restore_commit_selector_state(&previous, 0, Some((0, 2)), &refreshed);

        assert_eq!(cursor, 2);
        assert_eq!(selection, Some((1, 2)));
    }

    #[test]
    fn export_without_open_questions_sets_message_instead_of_erroring() {
        let (_temp, mut app) = single_file_app("fn main() {}\n", |_| {});

        app.export_questions().unwrap();

        assert_eq!(
            app.message.as_deref(),
            Some("packet has no open questions waiting for an agent reply")
        );
        assert!(!app.should_quit);
        assert!(app.quit_export.is_none());
    }
}
