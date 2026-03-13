use std::fmt;

use chrono::{DateTime, Utc};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const PACKET_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Packet {
    pub version: u32,
    pub session_id: String,
    pub title: String,
    pub workspace_root: String,
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
    #[serde(default)]
    pub conversation: Vec<QuestionMessage>,
    pub status: QuestionStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionMessage {
    pub id: String,
    pub role: QuestionMessageRole,
    pub body: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, Ord, PartialOrd)]
#[serde(rename_all = "kebab-case")]
pub enum QuestionMessageRole {
    User,
    Agent,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum QuestionTurnKind {
    Prompt,
    UserFollowUp,
    AgentReply,
}

#[derive(Debug, Clone, Copy)]
pub struct QuestionTurnRef<'a> {
    pub kind: QuestionTurnKind,
    pub body: &'a str,
}

impl Packet {
    pub fn new(
        session_id: impl Into<String>,
        title: impl Into<String>,
        workspace_root: impl Into<String>,
        files: Vec<TrackedFile>,
    ) -> Self {
        let now = Utc::now();
        Self {
            version: PACKET_VERSION,
            session_id: session_id.into(),
            title: title.into(),
            workspace_root: workspace_root.into(),
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

    pub fn ensure_file(&mut self, path: impl Into<String>) -> bool {
        let path = path.into();
        if self.files.iter().all(|file| file.path != path) {
            self.files.push(TrackedFile {
                path,
                label: None,
                purpose: None,
            });
            return true;
        }
        false
    }

    pub fn open_questions(&self) -> impl Iterator<Item = &Question> {
        self.questions
            .iter()
            .filter(|question| question.status == QuestionStatus::Open)
    }

    pub fn questions_requiring_reply(&self) -> impl Iterator<Item = &Question> {
        self.open_questions()
            .filter(|question| question.needs_agent_reply())
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
    #[allow(clippy::too_many_arguments)]
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
            conversation: Vec::new(),
            status: QuestionStatus::Open,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn add_message(&mut self, role: QuestionMessageRole, body: impl Into<String>) {
        self.conversation.push(QuestionMessage::new(role, body));
        self.updated_at = Utc::now();
    }

    pub fn turn_count(&self) -> usize {
        1 + self.conversation.len()
    }

    pub fn turns(&self) -> impl Iterator<Item = QuestionTurnRef<'_>> {
        std::iter::once(QuestionTurnRef {
            kind: QuestionTurnKind::Prompt,
            body: self.prompt.as_str(),
        })
        .chain(self.conversation.iter().map(|message| QuestionTurnRef {
            kind: match message.role {
                QuestionMessageRole::User => QuestionTurnKind::UserFollowUp,
                QuestionMessageRole::Agent => QuestionTurnKind::AgentReply,
            },
            body: message.body.as_str(),
        }))
    }

    pub fn needs_agent_reply(&self) -> bool {
        if self.status != QuestionStatus::Open {
            return false;
        }
        match self.conversation.last() {
            Some(message) => message.role == QuestionMessageRole::User,
            None => true,
        }
    }
}

impl QuestionMessage {
    pub fn new(role: QuestionMessageRole, body: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: format!("question-message-{}", Uuid::new_v4().simple()),
            role,
            body: body.into(),
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

impl QuestionMessageRole {
    pub const fn label(self) -> &'static str {
        match self {
            Self::User => "Follow-up",
            Self::Agent => "Agent Reply",
        }
    }
}
