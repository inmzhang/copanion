use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use clap::{Args, Parser, Subcommand};

use crate::clipboard;
use crate::export;
use crate::model::{Anchor, Note, NoteKind, NoteSource, Question};
use crate::storage::{self, ProjectPaths};
use crate::tui;
use crate::util::human_title;

#[derive(Debug, Parser)]
#[command(
    name = "copanion",
    version,
    about = "Attach code learning notes to source files and explore them in a terminal UI."
)]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Create the local packet directory under .copanion/
    Init,
    /// Create a new packet describing the files you want to learn
    New(NewArgs),
    /// Add a structured guidance note to a packet
    Note {
        #[command(subcommand)]
        command: NoteCommand,
    },
    /// Add a follow-up question to a packet
    Question {
        #[command(subcommand)]
        command: QuestionCommand,
    },
    /// Print a packet summary
    Show(ShowArgs),
    /// Copy open questions into a structured agent prompt
    Export(ExportArgs),
    /// Open the packet in the interactive TUI
    Open { packet: String },
}

#[derive(Debug, Args)]
struct NewArgs {
    /// Packet name or path. Bare names resolve to .copanion/packets/<name>.toml
    packet: String,
    /// Human-readable title shown in the TUI and exports
    #[arg(long)]
    title: Option<String>,
    /// Files to preload into the packet
    #[arg(short = 'f', long = "file")]
    files: Vec<PathBuf>,
    /// Overwrite the existing packet if it already exists
    #[arg(long)]
    force: bool,
}

#[derive(Debug, Subcommand)]
enum NoteCommand {
    Add(AddNoteArgs),
}

#[derive(Debug, Args)]
struct AddNoteArgs {
    packet: String,
    #[arg(long)]
    path: PathBuf,
    #[arg(long)]
    line: usize,
    #[arg(long)]
    end_line: Option<usize>,
    #[arg(long)]
    title: String,
    #[arg(long, conflicts_with = "body_file")]
    body: Option<String>,
    #[arg(long, value_name = "PATH", conflicts_with = "body")]
    body_file: Option<PathBuf>,
    #[arg(long, value_delimiter = ',')]
    tags: Vec<String>,
    #[arg(long, default_value = "overview")]
    kind: NoteKind,
    #[arg(long, default_value = "agent")]
    source: NoteSource,
    #[arg(long)]
    author: Option<String>,
}

#[derive(Debug, Subcommand)]
enum QuestionCommand {
    Add(AddQuestionArgs),
}

#[derive(Debug, Args)]
struct AddQuestionArgs {
    packet: String,
    #[arg(long)]
    path: PathBuf,
    #[arg(long)]
    line: Option<usize>,
    #[arg(long)]
    end_line: Option<usize>,
    #[arg(long, conflicts_with = "prompt_file")]
    prompt: Option<String>,
    #[arg(long, value_name = "PATH", conflicts_with = "prompt")]
    prompt_file: Option<PathBuf>,
    #[arg(long)]
    why: Option<String>,
    #[arg(long, value_delimiter = ',')]
    related_note_ids: Vec<String>,
}

#[derive(Debug, Args)]
struct ShowArgs {
    packet: String,
    #[arg(long)]
    full: bool,
}

#[derive(Debug, Args)]
struct ExportArgs {
    packet: String,
    /// Print to stdout instead of copying to clipboard
    #[arg(long)]
    stdout: bool,
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let cwd = std::env::current_dir().context("failed to read current directory")?;
    let paths = ProjectPaths::from_root(cwd);

    match cli.command {
        Command::Init => cmd_init(&paths),
        Command::New(args) => cmd_new(&paths, args),
        Command::Note { command } => match command {
            NoteCommand::Add(args) => cmd_note_add(&paths, args),
        },
        Command::Question { command } => match command {
            QuestionCommand::Add(args) => cmd_question_add(&paths, args),
        },
        Command::Show(args) => cmd_show(&paths, args),
        Command::Export(args) => cmd_export(&paths, args),
        Command::Open { packet } => {
            let packet_path = paths.packet_path(&packet);
            tui::run(&packet_path)
        }
    }
}

fn cmd_init(paths: &ProjectPaths) -> Result<()> {
    paths.ensure_initialized()?;
    println!("initialized {}", paths.copanion_dir.display());
    Ok(())
}

fn cmd_new(paths: &ProjectPaths, args: NewArgs) -> Result<()> {
    let title = args.title.unwrap_or_else(|| human_title(&args.packet));
    let packet_path = storage::create_packet(paths, &args.packet, title, args.files, args.force)?;
    println!("created packet {}", packet_path.display());
    Ok(())
}

fn cmd_note_add(paths: &ProjectPaths, args: AddNoteArgs) -> Result<()> {
    let packet_path = paths.packet_path(&args.packet);
    let mut packet = storage::read_packet(&packet_path)?;
    let body = load_text(args.body, args.body_file, "note body")?;
    let path = storage::normalize_repo_path(&args.path, &paths.root);
    packet.ensure_file(path.clone());
    packet.notes.push(Note::new(
        path,
        Anchor::new(args.line, args.end_line),
        args.kind,
        args.title,
        body,
        args.tags,
        args.author,
        args.source,
    ));
    packet.touch();
    storage::write_packet(&packet_path, &packet)?;
    println!("added note to {}", packet_path.display());
    Ok(())
}

fn cmd_question_add(paths: &ProjectPaths, args: AddQuestionArgs) -> Result<()> {
    let packet_path = paths.packet_path(&args.packet);
    let mut packet = storage::read_packet(&packet_path)?;
    let prompt = load_text(args.prompt, args.prompt_file, "question prompt")?;
    let path = storage::normalize_repo_path(&args.path, &paths.root);
    packet.ensure_file(path.clone());
    let anchor = args.line.map(|line| Anchor::new(line, args.end_line));
    packet.questions.push(Question::new(
        path,
        anchor,
        prompt,
        args.why,
        args.related_note_ids,
    ));
    packet.touch();
    storage::write_packet(&packet_path, &packet)?;
    println!("added question to {}", packet_path.display());
    Ok(())
}

fn cmd_show(paths: &ProjectPaths, args: ShowArgs) -> Result<()> {
    let packet_path = paths.packet_path(&args.packet);
    let packet = storage::read_packet(&packet_path)?;

    println!("packet: {}", packet.title);
    println!("path: {}", packet_path.display());
    println!(
        "files: {}  notes: {}  questions: {} (open: {})",
        packet.files.len(),
        packet.notes.len(),
        packet.questions.len(),
        packet.open_questions().count()
    );

    for file in &packet.files {
        println!("- {}", file.path);
    }

    if args.full {
        if !packet.notes.is_empty() {
            println!("\nnotes:");
        }
        for note in &packet.notes {
            println!(
                "- [{}:{}] {} ({:?})",
                note.path, note.anchor, note.title, note.kind
            );
            for line in note.body.lines() {
                println!("  {line}");
            }
        }
        if !packet.questions.is_empty() {
            println!("\nquestions:");
        }
        for question in &packet.questions {
            let anchor = question
                .anchor
                .map(|anchor| format!(":{anchor}"))
                .unwrap_or_default();
            println!("- [{}{}] {}", question.path, anchor, question.prompt);
            if let Some(why) = &question.why {
                println!("  why: {why}");
            }
        }
    }

    Ok(())
}

fn cmd_export(paths: &ProjectPaths, args: ExportArgs) -> Result<()> {
    let packet_path = paths.packet_path(&args.packet);
    let packet = storage::read_packet(&packet_path)?;
    let export = export::generate_question_export(&packet)?;
    if args.stdout {
        print!("{export}");
        return Ok(());
    }

    let result = clipboard::copy_text(&export)?;
    println!("{result}");
    Ok(())
}

fn load_text(
    direct_text: Option<String>,
    file_path: Option<PathBuf>,
    field_label: &str,
) -> Result<String> {
    match (direct_text, file_path) {
        (Some(text), None) => Ok(text),
        (None, Some(path)) => fs::read_to_string(&path)
            .with_context(|| format!("failed to read {} from {}", field_label, path.display())),
        (None, None) => Err(anyhow!(
            "{} is required; pass the inline flag or a corresponding --*-file",
            field_label
        )),
        (Some(_), Some(_)) => unreachable!("clap enforces conflicts"),
    }
}

#[allow(dead_code)]
fn require_existing_source(root: &Path, path: &Path) -> Result<()> {
    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    if candidate.exists() {
        Ok(())
    } else {
        Err(anyhow!(
            "source file does not exist: {}",
            candidate.display()
        ))
    }
}
