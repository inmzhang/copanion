use std::path::Path;

use anyhow::{Result, bail};

use crate::diff::{CommitInfo, DiffSelection};
use crate::model::{Anchor, Note, Packet, QuestionMessageRole};

#[derive(Debug, Clone)]
pub struct ReviewExportContext {
    pub selection: DiffSelection,
    pub review_entries: Vec<CommitInfo>,
    pub changed_paths: Vec<String>,
    pub visible_question_ids: Vec<String>,
}

pub fn generate_question_export(packet: &Packet, packet_path: &Path) -> Result<String> {
    let open_questions: Vec<_> = packet.questions_requiring_reply().collect();

    if open_questions.is_empty() {
        bail!("packet has no open questions waiting for an agent reply");
    }

    let mut output = String::new();
    output.push_str("# Copanion Follow-up\n\n");
    output.push_str("Please answer the open question threads below.\n\n");
    output.push_str(&format!("Packet: {}\n", packet.title));
    output.push_str(&format!(
        "Canonical packet path: {}\n",
        packet_path.display()
    ));
    output.push_str(&format!("Project root: {}\n\n", packet.workspace_root));

    output.push_str("Files in focus:\n");
    for file in &packet.files {
        output.push_str(&format!("- {}\n", file.path));
    }
    output.push('\n');

    output.push_str("Existing guidance notes:\n");
    if packet.notes.is_empty() {
        output.push_str("- none yet\n");
    } else {
        for note in &packet.notes {
            output.push_str(&format!(
                "- [{}:{}] {} ({:?}, {:?})\n",
                note.path, note.anchor, note.title, note.kind, note.source
            ));
            for line in note.body.lines() {
                output.push_str(&format!("  {}\n", line));
            }
        }
    }
    output.push('\n');

    output.push_str("Questions:\n");
    for (index, question) in open_questions.iter().enumerate() {
        output.push_str(&format!(
            "{}. id={} [{}{}] {}\n",
            index + 1,
            question.id,
            question.path,
            format_anchor(question.anchor),
            question.prompt
        ));
        if let Some(why) = &question.why {
            output.push_str(&format!("   Why unclear: {}\n", why));
        }
        if !question.related_note_ids.is_empty() {
            output.push_str(&format!(
                "   Related notes: {}\n",
                question.related_note_ids.join(", ")
            ));
        }
        if !question.conversation.is_empty() {
            output.push_str("   Conversation so far:\n");
            for message in &question.conversation {
                output.push_str(&format!(
                    "   - {}: {}\n",
                    conversation_role_label(message.role),
                    message.body.replace('\n', "\n     ")
                ));
            }
        }
    }
    output.push('\n');

    Ok(output)
}

pub fn generate_review_question_export(
    packet: &Packet,
    packet_path: &Path,
    review: &ReviewExportContext,
) -> Result<String> {
    let visible_question_ids = review
        .visible_question_ids
        .iter()
        .collect::<std::collections::BTreeSet<_>>();
    let open_questions = packet
        .questions_requiring_reply()
        .filter(|question| visible_question_ids.contains(&question.id))
        .collect::<Vec<_>>();

    if open_questions.is_empty() {
        bail!("the current diff review has no open comment threads waiting for an agent reply");
    }

    let mut output = String::new();
    output.push_str("# Copanion Diff Review\n\n");
    output.push_str(
        "Please answer the review comments below. Any review notes are listed separately.\n\n",
    );
    output.push_str(&format!("Packet: {}\n", packet.title));
    output.push_str(&format!(
        "Canonical packet path: {}\n",
        packet_path.display()
    ));
    output.push_str(&format!("Project root: {}\n", packet.workspace_root));
    output.push_str(&format!(
        "Review scope: {}\n\n",
        review_scope_label(&review.selection)
    ));

    output.push_str("Selected revisions:\n");
    for entry in &review.review_entries {
        if entry.is_working_tree() {
            output.push_str("- Uncommitted changes\n");
        } else {
            output.push_str(&format!("- {} {}\n", entry.short_id, entry.summary));
        }
    }
    output.push('\n');

    output.push_str("Files under review:\n");
    for path in &review.changed_paths {
        output.push_str(&format!("- {path}\n"));
    }
    output.push('\n');

    output.push_str("Review comments:\n");
    for (index, question) in open_questions.iter().enumerate() {
        output.push_str(&format!(
            "{}. id={} [{}{}] {}\n",
            index + 1,
            question.id,
            question.path,
            format_anchor(question.anchor),
            question.prompt
        ));
        if !question.conversation.is_empty() {
            output.push_str("   Conversation so far:\n");
            for message in &question.conversation {
                output.push_str(&format!(
                    "   - {}: {}\n",
                    conversation_role_label(message.role),
                    message.body.replace('\n', "\n     ")
                ));
            }
        }
    }
    output.push('\n');

    Ok(output)
}

pub fn summarize_note(note: &Note) -> String {
    format!("[{}:{}] {}", note.path, note.anchor, note.title)
}

fn format_anchor(anchor: Option<Anchor>) -> String {
    match anchor {
        Some(anchor) => format!(":{}", anchor),
        None => String::new(),
    }
}

fn conversation_role_label(role: QuestionMessageRole) -> &'static str {
    match role {
        QuestionMessageRole::User => "user",
        QuestionMessageRole::Agent => "agent",
    }
}

fn review_scope_label(selection: &DiffSelection) -> &'static str {
    match selection {
        DiffSelection::WorkingTree => "working tree",
        DiffSelection::CommitRange(_) => "selected commits",
        DiffSelection::WorkingTreeAndCommits(_) => "working tree plus selected commits",
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::diff::{CommitInfo, DiffSelection};
    use crate::model::{Anchor, Note, NoteKind, NoteSource, Packet, Question, QuestionMessageRole};

    use super::{
        ReviewExportContext, generate_question_export, generate_review_question_export,
        summarize_note,
    };

    #[test]
    fn export_requires_questions_waiting_for_reply() {
        let mut packet = Packet::new("test", "Test", "/repo", vec![]);
        let mut question = Question::new(
            "src/main.rs",
            Some(Anchor::new(1, None)),
            "What is happening here?",
            None,
            vec![],
        );
        question.add_message(
            QuestionMessageRole::Agent,
            "This is already answered and waiting on the user.",
        );
        packet.questions.push(question);
        assert!(generate_question_export(&packet, Path::new("/tmp/packet.toml")).is_err());
    }

    #[test]
    fn export_includes_notes_and_question_ids_without_embedding_write_back_command() {
        let mut packet = Packet::new("tour", "Tour", "/repo", vec![]);
        packet.notes.push(Note::new(
            "src/main.rs",
            Anchor::new(10, Some(12)),
            NoteKind::Flow,
            "Startup path",
            "Control reaches this branch after argument parsing.",
            vec![],
            Some("codex".to_string()),
            NoteSource::Agent,
        ));
        let mut question = Question::new(
            "src/main.rs",
            Some(Anchor::new(11, None)),
            "Why is this branch separate from the fast path?",
            Some("The note explains the flow but not the design reason.".to_string()),
            vec![packet.notes[0].id.clone()],
        );
        question.add_message(
            QuestionMessageRole::User,
            "I still do not understand the invariant behind the split.",
        );
        let question_id = question.id.clone();
        packet.questions.push(question);

        let export = generate_question_export(&packet, Path::new("/tmp/packet.toml")).unwrap();
        assert!(export.contains("Startup path"));
        assert!(export.contains(&question_id));
        assert!(!export.contains("copanion --apply-agent-response -"));
        assert!(summarize_note(&packet.notes[0]).contains("src/main.rs:10-12"));
    }

    #[test]
    fn review_export_mentions_scope_and_filters_to_review_paths() {
        let mut packet = Packet::new("tour", "Tour", "/repo", vec![]);
        packet.notes.push(Note::new(
            "src/main.rs",
            Anchor::new(10, None),
            NoteKind::Overview,
            "Looks risky",
            "Please double-check the branch condition.",
            vec![],
            None,
            NoteSource::Human,
        ));
        packet.notes.push(Note::new(
            "src/lib.rs",
            Anchor::new(3, None),
            NoteKind::Overview,
            "Ignore me",
            "This note is outside the current diff.",
            vec![],
            None,
            NoteSource::Human,
        ));
        let mut question = Question::new(
            "src/main.rs",
            Some(Anchor::new(11, None)),
            "Why is this branch separate?",
            None,
            vec![],
        );
        question.add_message(QuestionMessageRole::User, "What invariant depends on it?");
        packet.questions.push(question);

        let export = generate_review_question_export(
            &packet,
            Path::new("/tmp/packet.toml"),
            &ReviewExportContext {
                selection: DiffSelection::WorkingTreeAndCommits(vec!["abc".to_string()]),
                review_entries: vec![
                    CommitInfo::working_tree_entry(),
                    CommitInfo {
                        id: "abc".to_string(),
                        short_id: "abc1234".to_string(),
                        branch_name: None,
                        summary: "Refine scheduler".to_string(),
                        body: None,
                        author: "Test User".to_string(),
                        time: chrono::Utc::now(),
                    },
                ],
                changed_paths: vec!["src/main.rs".to_string()],
                visible_question_ids: vec![packet.questions[0].id.clone()],
            },
        )
        .unwrap();

        assert!(export.contains("Copanion Diff Review"));
        assert!(export.contains("working tree plus selected commits"));
        assert!(export.contains("Uncommitted changes"));
        assert!(export.contains("abc1234 Refine scheduler"));
        assert!(export.contains("Review comments:"));
        assert!(export.contains("Why is this branch separate?"));
        assert!(!export.contains("Review notes:"));
        assert!(!export.contains("Looks risky"));
        assert!(!export.contains("Ignore me"));
    }

    #[test]
    fn review_export_only_includes_visible_diff_questions() {
        let mut packet = Packet::new("tour", "Tour", "/repo", vec![]);
        let visible_question = Question::new(
            "src/main.rs",
            Some(Anchor::new(11, None)),
            "Visible diff comment",
            None,
            vec![],
        );
        let hidden_question = Question::new(
            "src/main.rs",
            Some(Anchor::new(99, None)),
            "Tracked-mode question outside current diff context",
            None,
            vec![],
        );
        let visible_id = visible_question.id.clone();
        packet.questions.push(visible_question);
        packet.questions.push(hidden_question);

        let export = generate_review_question_export(
            &packet,
            Path::new("/tmp/packet.toml"),
            &ReviewExportContext {
                selection: DiffSelection::WorkingTree,
                review_entries: vec![CommitInfo::working_tree_entry()],
                changed_paths: vec!["src/main.rs".to_string()],
                visible_question_ids: vec![visible_id],
            },
        )
        .unwrap();

        assert!(export.contains("Visible diff comment"));
        assert!(!export.contains("Tracked-mode question outside current diff context"));
    }
}
