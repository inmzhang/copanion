use std::fmt;

use chrono::{DateTime, Utc};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Packet {
    pub version: u32,
    pub title: String,
    #[serde(default)]
    pub files: Vec<TrackedFile>,
    #[serde(default)]
    pub notes: Vec<Note>,
    #[serde(default)]
    pub questions: Vec<Question>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackedFile {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    pub id: String,
    pub path: String,
    pub anchor: Anchor,
    pub kind: NoteKind,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    pub source: NoteSource,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Question {
    pub id: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<Anchor>,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    #[serde(default)]
    pub related_note_ids: Vec<String>,
    pub status: QuestionStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Anchor {
    pub start_line: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_line: Option<usize>,
}

#[derive(
    Debug, Clone, Copy, Default, Eq, PartialEq, Serialize, Deserialize, ValueEnum, Ord, PartialOrd,
)]
#[serde(rename_all = "kebab-case")]
pub enum NoteKind {
    #[default]
    Overview,
    Flow,
    Pitfall,
    Reference,
}

#[derive(
    Debug, Clone, Copy, Default, Eq, PartialEq, Serialize, Deserialize, ValueEnum, Ord, PartialOrd,
)]
#[serde(rename_all = "kebab-case")]
pub enum NoteSource {
    #[default]
    Agent,
    Human,
    Imported,
}

#[derive(
    Debug, Clone, Copy, Default, Eq, PartialEq, Serialize, Deserialize, ValueEnum, Ord, PartialOrd,
)]
#[serde(rename_all = "kebab-case")]
pub enum QuestionStatus {
    #[default]
    Open,
    Answered,
    Archived,
}

impl Packet {
    pub fn new(title: impl Into<String>, files: Vec<TrackedFile>) -> Self {
        let now = Utc::now();
        Self {
            version: 1,
            title: title.into(),
            files,
            notes: Vec::new(),
            questions: Vec::new(),
            created_at: now,
            updated_at: now,
        }
    }

    pub fn touch(&mut self) {
        self.updated_at = Utc::now();
    }

    pub fn ensure_file(&mut self, path: impl Into<String>) {
        let path = path.into();
        if self.files.iter().all(|file| file.path != path) {
            self.files.push(TrackedFile {
                path,
                label: None,
                purpose: None,
            });
        }
    }

    pub fn open_questions(&self) -> impl Iterator<Item = &Question> {
        self.questions
            .iter()
            .filter(|question| question.status == QuestionStatus::Open)
    }
}

impl TrackedFile {
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            label: None,
            purpose: None,
        }
    }
}

impl Note {
    pub fn new(
        path: impl Into<String>,
        anchor: Anchor,
        kind: NoteKind,
        title: impl Into<String>,
        body: impl Into<String>,
        tags: Vec<String>,
        author: Option<String>,
        source: NoteSource,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: format!("note-{}", Uuid::new_v4().simple()),
            path: path.into(),
            anchor,
            kind,
            title: title.into(),
            body: body.into(),
            tags,
            author,
            source,
            created_at: now,
            updated_at: now,
        }
    }
}

impl Question {
    pub fn new(
        path: impl Into<String>,
        anchor: Option<Anchor>,
        prompt: impl Into<String>,
        why: Option<String>,
        related_note_ids: Vec<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: format!("question-{}", Uuid::new_v4().simple()),
            path: path.into(),
            anchor,
            prompt: prompt.into(),
            why,
            related_note_ids,
            status: QuestionStatus::Open,
            created_at: now,
            updated_at: now,
        }
    }
}

impl Anchor {
    pub const fn new(start_line: usize, end_line: Option<usize>) -> Self {
        Self {
            start_line,
            end_line,
        }
    }

    pub fn display(self) -> String {
        match self.end_line {
            Some(end) if end != self.start_line => format!("{}-{}", self.start_line, end),
            _ => self.start_line.to_string(),
        }
    }
}

impl fmt::Display for Anchor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display())
    }
}
