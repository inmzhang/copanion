use std::fs;
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;

use crate::model::{
    Anchor, Note, NoteKind, NoteSource, Packet, QuestionMessageRole, QuestionStatus,
};
use crate::storage;

#[derive(Debug, Default, Deserialize)]
pub struct AgentResponsePlan {
    #[serde(default)]
    pub answers: Vec<AgentAnswer>,
    #[serde(default)]
    pub notes: Vec<AgentNote>,
}

#[derive(Debug, Deserialize)]
pub struct AgentAnswer {
    pub question_id: String,
    pub answer: String,
}

#[derive(Debug, Deserialize)]
pub struct AgentNote {
    pub path: String,
    pub start_line: usize,
    #[serde(default)]
    pub end_line: Option<usize>,
    #[serde(default)]
    pub kind: Option<NoteKind>,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub author: Option<String>,
}

#[derive(Debug, Default, Eq, PartialEq)]
pub struct ApplySummary {
    pub answered_questions: usize,
    pub added_notes: usize,
}

pub fn read_plan(path_arg: &str) -> Result<AgentResponsePlan> {
    let raw = if path_arg == "-" {
        use std::io::Read;

        let mut stdin = std::io::stdin().lock();
        let mut buf = String::new();
        stdin
            .read_to_string(&mut buf)
            .context("failed to read the response plan from stdin")?;
        buf
    } else {
        fs::read_to_string(path_arg)
            .with_context(|| format!("failed to read the response plan from {path_arg}"))?
    };
    serde_json::from_str(&raw).context("failed to parse the response plan as JSON")
}

pub fn apply_plan(
    packet: &mut Packet,
    repo_root: &Path,
    plan: AgentResponsePlan,
) -> Result<ApplySummary> {
    let mut summary = ApplySummary::default();

    for answer in plan.answers {
        let body = answer.answer.trim();
        if body.is_empty() {
            return Err(anyhow!(
                "agent answers must be non-empty (question {})",
                answer.question_id
            ));
        }
        let question = packet
            .questions
            .iter_mut()
            .find(|question| question.id == answer.question_id)
            .ok_or_else(|| anyhow!("unknown question id {}", answer.question_id))?;
        if question.status == QuestionStatus::Archived {
            return Err(anyhow!(
                "cannot append an answer to archived question {}",
                answer.question_id
            ));
        }
        if !question.needs_agent_reply() {
            return Err(anyhow!(
                "question {} is not waiting for an agent reply",
                answer.question_id
            ));
        }
        question.add_message(QuestionMessageRole::Agent, body.to_string());
        summary.answered_questions += 1;
    }

    for note in plan.notes {
        if note.start_line == 0 {
            return Err(anyhow!("note anchors must start at line 1 or later"));
        }
        if note
            .end_line
            .is_some_and(|end_line| end_line < note.start_line)
        {
            return Err(anyhow!(
                "note anchors must not end before they start ({}..{:?})",
                note.start_line,
                note.end_line
            ));
        }
        let path = storage::normalize_repo_path(Path::new(&note.path), repo_root);
        packet.ensure_file(path.clone());
        packet.notes.push(Note::new(
            path,
            Anchor::new(note.start_line, note.end_line),
            note.kind.unwrap_or(NoteKind::Reference),
            note.title,
            note.body,
            note.tags,
            note.author,
            NoteSource::Agent,
        ));
        summary.added_notes += 1;
    }

    if summary.answered_questions > 0 || summary.added_notes > 0 {
        packet.touch();
    }

    Ok(summary)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::model::{Packet, Question, QuestionMessageRole};

    use super::{AgentAnswer, AgentNote, AgentResponsePlan, apply_plan};

    #[test]
    fn apply_plan_appends_agent_replies_and_notes() {
        let mut packet = Packet::new("demo", "Demo", "/repo", vec![]);
        packet.questions.push(Question::new(
            "src/main.rs",
            None,
            "Why is this empty?",
            None,
            vec![],
        ));
        let question_id = packet.questions[0].id.clone();

        let summary = apply_plan(
            &mut packet,
            Path::new("/repo"),
            AgentResponsePlan {
                answers: vec![AgentAnswer {
                    question_id,
                    answer: "It is a placeholder for future initialization.".to_string(),
                }],
                notes: vec![AgentNote {
                    path: "/repo/src/main.rs".to_string(),
                    start_line: 1,
                    end_line: None,
                    kind: None,
                    title: "placeholder".to_string(),
                    body: "The empty body is intentional here.".to_string(),
                    tags: vec![],
                    author: None,
                }],
            },
        )
        .unwrap();

        assert_eq!(summary.answered_questions, 1);
        assert_eq!(summary.added_notes, 1);
        assert_eq!(packet.questions[0].conversation.len(), 1);
        assert_eq!(
            packet.questions[0].conversation[0].role,
            QuestionMessageRole::Agent
        );
        assert_eq!(packet.notes.len(), 1);
    }

    #[test]
    fn apply_plan_rejects_answers_for_threads_not_waiting_for_reply() {
        let mut packet = Packet::new("demo", "Demo", "/repo", vec![]);
        let mut question = Question::new("src/main.rs", None, "Why is this empty?", None, vec![]);
        question.add_message(QuestionMessageRole::Agent, "Already answered.");
        let question_id = question.id.clone();
        packet.questions.push(question);

        let error = apply_plan(
            &mut packet,
            Path::new("/repo"),
            AgentResponsePlan {
                answers: vec![AgentAnswer {
                    question_id,
                    answer: "Another answer".to_string(),
                }],
                notes: vec![],
            },
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("is not waiting for an agent reply")
        );
    }

    #[test]
    fn apply_plan_preserves_user_follow_ups_when_appending_agent_replies() {
        let mut packet = Packet::new("demo", "Demo", "/repo", vec![]);
        let mut question = Question::new("src/main.rs", None, "Why is this empty?", None, vec![]);
        question.add_message(QuestionMessageRole::Agent, "The branch is a placeholder.");
        question.add_message(
            QuestionMessageRole::User,
            "What invariant depends on leaving it empty?",
        );
        let question_id = question.id.clone();
        packet.questions.push(question);

        let summary = apply_plan(
            &mut packet,
            Path::new("/repo"),
            AgentResponsePlan {
                answers: vec![AgentAnswer {
                    question_id,
                    answer: "The fast path assumes the branch never allocates.".to_string(),
                }],
                notes: vec![],
            },
        )
        .unwrap();

        assert_eq!(summary.answered_questions, 1);
        assert_eq!(packet.questions[0].conversation.len(), 3);
        assert_eq!(
            packet.questions[0].conversation[1].body,
            "What invariant depends on leaving it empty?"
        );
        assert_eq!(
            packet.questions[0].conversation[2].role,
            QuestionMessageRole::Agent
        );
    }

    #[test]
    fn apply_plan_rejects_inverted_note_ranges() {
        let mut packet = Packet::new("demo", "Demo", "/repo", vec![]);

        let error = apply_plan(
            &mut packet,
            Path::new("/repo"),
            AgentResponsePlan {
                answers: vec![],
                notes: vec![AgentNote {
                    path: "src/main.rs".to_string(),
                    start_line: 5,
                    end_line: Some(3),
                    kind: None,
                    title: "broken".to_string(),
                    body: "bad range".to_string(),
                    tags: vec![],
                    author: None,
                }],
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("must not end before they start"));
    }
}
