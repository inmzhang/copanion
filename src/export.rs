use std::borrow::Cow;
use std::path::Path;

use anyhow::{Result, bail};

use crate::diff::{CommitInfo, DiffSelection};
use crate::model::{Anchor, Packet, Question, QuestionTurnKind};

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
    write_header(
        &mut output,
        "# Copanion Follow-up",
        "Please answer the open question threads below.",
        packet,
        packet_path,
    );
    write_path_section(
        &mut output,
        "Files in focus",
        packet
            .files
            .iter()
            .map(|file| Cow::Borrowed(file.path.as_str())),
    );
    write_questions_section(&mut output, "Questions", &open_questions, true);

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
    write_header(
        &mut output,
        "# Copanion Diff Review",
        "Please answer the review comments below. Any review notes are listed separately.",
        packet,
        packet_path,
    );
    output.push_str(&format!(
        "Review scope: {}\n\n",
        review_scope_label(&review.selection)
    ));

    write_path_section(
        &mut output,
        "Selected revisions",
        review.review_entries.iter().map(review_entry_label),
    );
    write_path_section(
        &mut output,
        "Files under review",
        review
            .changed_paths
            .iter()
            .map(|path| Cow::Borrowed(path.as_str())),
    );
    write_questions_section(&mut output, "Review comments", &open_questions, false);

    Ok(output)
}

fn format_anchor(anchor: Option<Anchor>) -> String {
    match anchor {
        Some(anchor) => format!(":{}", anchor),
        None => String::new(),
    }
}

fn thread_turn_label(kind: QuestionTurnKind) -> &'static str {
    match kind {
        QuestionTurnKind::Prompt => "user",
        QuestionTurnKind::UserFollowUp => "user",
        QuestionTurnKind::AgentReply => "agent",
    }
}

fn review_scope_label(selection: &DiffSelection) -> &'static str {
    match selection {
        DiffSelection::WorkingTree => "working tree",
        DiffSelection::CommitRange(_) => "selected commits",
        DiffSelection::WorkingTreeAndCommits(_) => "working tree plus selected commits",
    }
}

fn write_header(
    output: &mut String,
    heading: &str,
    intro: &str,
    packet: &Packet,
    packet_path: &Path,
) {
    output.push_str(heading);
    output.push_str("\n\n");
    output.push_str(intro);
    output.push_str("\n\n");
    output.push_str(&format!("Packet: {}\n", packet.title));
    output.push_str(&format!(
        "Canonical packet path: {}\n",
        packet_path.display()
    ));
    output.push_str(&format!("Project root: {}\n\n", packet.workspace_root));
}

fn write_path_section<'a>(
    output: &mut String,
    title: &str,
    items: impl IntoIterator<Item = Cow<'a, str>>,
) {
    output.push_str(title);
    output.push_str(":\n");
    let mut saw_item = false;
    for item in items {
        saw_item = true;
        output.push_str("- ");
        output.push_str(&item);
        output.push('\n');
    }
    if !saw_item {
        output.push_str("- none\n");
    }
    output.push('\n');
}

fn write_questions_section(
    output: &mut String,
    title: &str,
    questions: &[&Question],
    include_note_context: bool,
) {
    output.push_str(title);
    output.push_str(":\n");
    for (index, question) in questions.iter().enumerate() {
        write_question(output, index + 1, question, include_note_context);
    }
    output.push('\n');
}

fn write_question(
    output: &mut String,
    index: usize,
    question: &Question,
    include_note_context: bool,
) {
    output.push_str(&format!(
        "{index}. id={} [{}{}] {} turn{}\n",
        question.id,
        question.path,
        format_anchor(question.anchor),
        question.turn_count(),
        if question.turn_count() == 1 { "" } else { "s" }
    ));
    if include_note_context && let Some(why) = &question.why {
        output.push_str(&format!("   Why unclear: {why}\n"));
    }
    write_thread(output, question);
}

fn write_thread(output: &mut String, question: &Question) {
    output.push_str("   Thread:\n");
    for turn in question.turns() {
        output.push_str(&format!(
            "   - {}: {}\n",
            thread_turn_label(turn.kind),
            turn.body.replace('\n', "\n     ")
        ));
    }
}

fn review_entry_label(entry: &CommitInfo) -> Cow<'_, str> {
    if entry.is_working_tree() {
        Cow::Borrowed("Uncommitted changes")
    } else {
        Cow::Owned(format!("{} {}", entry.short_id, entry.summary))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::diff::{CommitInfo, DiffSelection};
    use crate::model::{Anchor, Note, NoteKind, NoteSource, Packet, Question, QuestionMessageRole};

    use super::{ReviewExportContext, generate_question_export, generate_review_question_export};

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
    fn export_omits_notes_and_preserves_thread_order() {
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
            QuestionMessageRole::Agent,
            "The flow explains the setup, but not the design tradeoff.",
        );
        question.add_message(
            QuestionMessageRole::User,
            "I still do not understand the invariant behind the split.",
        );
        let question_id = question.id.clone();
        packet.questions.push(question);

        let export = generate_question_export(&packet, Path::new("/tmp/packet.toml")).unwrap();
        assert!(export.contains(&question_id));
        assert!(!export.contains("copanion --apply-agent-response -"));
        assert!(!export.contains("Existing guidance notes"));
        assert!(!export.contains("Startup path"));
        assert!(!export.contains("Control reaches this branch after argument parsing."));
        assert!(export.contains("Thread:"));
        assert!(export.contains("- user: Why is this branch separate from the fast path?"));
        assert!(
            export.contains("- agent: The flow explains the setup, but not the design tradeoff.")
        );
        assert!(
            export.contains("- user: I still do not understand the invariant behind the split.")
        );
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
