use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Parser, ValueHint};

use crate::answer_plan;
use crate::clipboard;
use crate::config;
use crate::export;
use crate::model::{PACKET_VERSION, Packet};
use crate::storage::{self, StoragePaths};
use crate::theme::{self, ThemeName};
use crate::tui;
use crate::util::human_title;

#[derive(Debug, Parser)]
#[command(
    name = "copanion",
    version,
    about = "Open a persistent source-learning packet with inline notes, question threads, and agent write-back.",
    after_help = "Each project uses a single canonical packet stored under Copanion's user data directory. `copanion` discovers the project root, loads that packet, and reopens it across runs."
)]
pub struct Cli {
    /// Source files to attach to this learning packet.
    #[arg(value_name = "FILE", value_hint = ValueHint::FilePath)]
    files: Vec<PathBuf>,
    /// Title to use when a new packet is created.
    #[arg(long)]
    title: Option<String>,
    /// Recreate the saved packet while preserving tracked files when no new files are supplied.
    #[arg(long)]
    fresh: bool,
    /// Print the canonical packet path and exit.
    #[arg(long = "print-packet-path", alias = "print-session-path")]
    print_packet_path: bool,
    /// Export only the open questions that still need an agent reply.
    #[arg(long)]
    export: bool,
    /// Print exports to stdout instead of copying them to the clipboard.
    #[arg(long)]
    stdout: bool,
    /// Apply an agent response plan from a JSON file or `-` for stdin, then exit.
    #[arg(
        long = "apply-agent-response",
        value_name = "PLAN",
        conflicts_with_all = ["files", "title", "fresh", "export", "print_packet_path", "stdout"]
    )]
    apply_agent_response: Option<String>,
    /// Built-in UI theme.
    #[arg(long, value_enum)]
    theme: Option<ThemeName>,
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let config_outcome = config::load_config()?;
    for warning in &config_outcome.warnings {
        eprintln!("{warning}");
    }
    let resolved_theme = resolve_theme(cli.theme, config_outcome.config.as_ref());
    theme::set_active(resolved_theme);

    let cwd = std::env::current_dir().context("failed to read the current directory")?;
    let project_root = storage::discover_project_root(&cwd);
    let paths = StoragePaths::discover()?;
    paths.ensure_initialized()?;
    let packet_path = paths.project_packet_path(&project_root);

    let existing = load_existing_packet(&packet_path, &project_root)?;
    if cli.apply_agent_response.is_some() && existing.is_none() {
        bail!(
            "no copanion packet exists for {}; run `copanion` there first",
            project_root.display()
        );
    }

    let mut packet = build_packet(&project_root, cli.title, cli.fresh, existing);

    if let Some(plan_arg) = cli.apply_agent_response.as_deref() {
        let plan = answer_plan::read_plan(plan_arg)?;
        let summary = answer_plan::apply_plan(&mut packet, &project_root, plan)?;
        storage::write_packet(&packet_path, &packet)?;
        println!(
            "updated {}: {} question replies, {} notes",
            packet_path.display(),
            summary.answered_questions,
            summary.added_notes
        );
        return Ok(());
    }

    let resolved_files = cli
        .files
        .iter()
        .map(|file| {
            if file.is_absolute() {
                file.clone()
            } else {
                cwd.join(file)
            }
        })
        .collect::<Vec<_>>();
    let merged = storage::merge_files(&mut packet, &project_root, &resolved_files);
    if merged {
        packet.touch();
    }

    storage::write_packet(&packet_path, &packet)?;

    if cli.print_packet_path {
        println!("{}", packet_path.display());
        return Ok(());
    }

    if cli.export {
        let export = export::generate_question_export(&packet, &packet_path)?;
        if cli.stdout {
            print!("{export}");
        } else {
            let result = clipboard::copy_text(&export)?;
            println!("{result}");
        }
        return Ok(());
    }

    tui::run(&packet_path, cli.stdout)
}

fn resolve_theme(cli_theme: Option<ThemeName>, config: Option<&config::AppConfig>) -> ThemeName {
    if let Some(theme) = cli_theme {
        return theme;
    }
    if let Some(theme_str) = config.and_then(|config| config.theme.as_deref())
        && let Some(theme) = ThemeName::parse_config(theme_str)
    {
        return theme;
    }
    ThemeName::default()
}

fn load_existing_packet(packet_path: &Path, project_root: &Path) -> Result<Option<Packet>> {
    if let Some(packet) = storage::read_packet_if_exists(packet_path)? {
        return Ok(Some(packet));
    }
    if let Some(legacy_path) = storage::legacy_default_session_path(project_root)? {
        return storage::read_packet_if_exists(&legacy_path);
    }
    Ok(None)
}

fn build_packet(
    project_root: &Path,
    title_override: Option<String>,
    fresh: bool,
    existing: Option<Packet>,
) -> Packet {
    let workspace_root = storage::workspace_root_string(project_root);
    let packet_id = storage::project_packet_id(project_root);
    match (existing, fresh) {
        (Some(mut packet), false) => {
            let mut changed = false;
            if packet.version != PACKET_VERSION {
                packet.version = PACKET_VERSION;
                changed = true;
            }
            if packet.workspace_root != workspace_root {
                packet.workspace_root = workspace_root.clone();
                changed = true;
            }
            if packet.session_id != packet_id {
                packet.session_id = packet_id.clone();
                changed = true;
            }
            if let Some(title) = title_override
                && packet.title != title
            {
                packet.title = title;
                changed = true;
            }
            if changed {
                packet.touch();
            }
            packet
        }
        (Some(packet), true) => Packet::new(
            packet_id,
            title_override.unwrap_or(packet.title),
            workspace_root,
            packet.files,
        ),
        (None, _) => Packet::new(
            packet_id.clone(),
            title_override.unwrap_or_else(|| {
                human_title(
                    project_root
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or(packet_id.as_str()),
                )
            }),
            workspace_root,
            vec![],
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::config::AppConfig;
    use crate::model::{PACKET_VERSION, Packet, TrackedFile};
    use crate::theme::ThemeName;

    use super::{build_packet, resolve_theme};

    #[test]
    fn fresh_packet_preserves_existing_files() {
        let packet = Packet::new(
            "tour-12345678",
            "Tour",
            "/repo",
            vec![TrackedFile::new("src/main.rs")],
        );
        let rebuilt = build_packet(Path::new("/repo"), None, true, Some(packet));
        assert_eq!(rebuilt.files.len(), 1);
        assert_eq!(rebuilt.files[0].path, "src/main.rs");
    }

    #[test]
    fn title_override_updates_loaded_packet() {
        let packet = Packet::new("tour-12345678", "Old", "/repo", vec![]);
        let rebuilt = build_packet(
            Path::new("/repo"),
            Some("New".to_string()),
            false,
            Some(packet),
        );
        assert_eq!(rebuilt.title, "New");
    }

    #[test]
    fn build_packet_upgrades_existing_metadata() {
        let mut packet = Packet::new("wrong", "Old", "/somewhere-else", vec![]);
        packet.version = 1;
        let rebuilt = build_packet(Path::new("/repo"), None, false, Some(packet));
        assert_eq!(rebuilt.version, PACKET_VERSION);
        assert_eq!(rebuilt.workspace_root, "/repo");
        assert!(rebuilt.session_id.starts_with("repo-"));
    }

    #[test]
    fn cli_theme_overrides_config_theme() {
        let config = AppConfig {
            theme: Some("light".to_string()),
        };
        assert_eq!(
            resolve_theme(Some(ThemeName::GruvboxDark), Some(&config)),
            ThemeName::GruvboxDark
        );
    }

    #[test]
    fn config_theme_applies_when_cli_theme_missing() {
        let config = AppConfig {
            theme: Some("catppuccin-mocha".to_string()),
        };
        assert_eq!(
            resolve_theme(None, Some(&config)),
            ThemeName::CatppuccinMocha
        );
    }

    #[test]
    fn default_theme_is_gruvbox_dark_when_nothing_overrides_it() {
        assert_eq!(resolve_theme(None, None), ThemeName::GruvboxDark);
    }
}
