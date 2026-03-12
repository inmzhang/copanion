use anyhow::{Result, bail};

use crate::model::{Anchor, Note, Packet, QuestionStatus};

pub fn generate_question_export(packet: &Packet) -> Result<String> {
    let open_questions: Vec<_> = packet
        .questions
        .iter()
        .filter(|question| question.status == QuestionStatus::Open)
        .collect();

    if open_questions.is_empty() {
        bail!("packet has no open questions to export");
    }

    let mut output = String::new();
    output.push_str("# Copanion Follow-up\n\n");
    output.push_str("I am using `copanion` to study this codebase.\n");
    output.push_str("Please answer the open questions, resolve the confusion precisely, and suggest any new notes that should be attached back into the packet.\n\n");
    output.push_str(&format!("Packet: {}\n", packet.title));
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

    output.push_str("Open questions:\n");
    for (index, question) in open_questions.iter().enumerate() {
        output.push_str(&format!(
            "{}. [{}{}] {}\n",
            index + 1,
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
    }
    output.push('\n');

    output.push_str("Preferred reply format:\n");
    output.push_str("1. Answer each open question in order.\n");
    output.push_str("2. If useful, propose additional notes as flat bullets with `path`, `start_line`, `end_line`, `title`, and `body`.\n");
    output.push_str("3. Call out any ambiguities that require reading neighboring files.\n");

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

#[cfg(test)]
mod tests {
    use crate::model::{Anchor, Note, NoteKind, NoteSource, Packet, Question};

    use super::{generate_question_export, summarize_note};

    #[test]
    fn export_requires_open_questions() {
        let packet = Packet::new("test", "Test", "/repo", vec![]);
        assert!(generate_question_export(&packet).is_err());
    }

    #[test]
    fn export_includes_notes_and_questions() {
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
        packet.questions.push(Question::new(
            "src/main.rs",
            Some(Anchor::new(11, None)),
            "Why is this branch separate from the fast path?",
            Some("The note explains the flow but not the design reason.".to_string()),
            vec![packet.notes[0].id.clone()],
        ));

        let export = generate_question_export(&packet).unwrap();
        assert!(export.contains("Startup path"));
        assert!(export.contains("Why is this branch separate from the fast path?"));
        assert!(summarize_note(&packet.notes[0]).contains("src/main.rs:10-12"));
    }
}
