use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, ValueHint};

use crate::clipboard;
use crate::config;
use crate::export;
use crate::model::Packet;
use crate::storage::{self, SessionPaths};
use crate::theme::{self, ThemeName};
use crate::tui;
use crate::util::human_title;

#[derive(Debug, Parser)]
#[command(
    name = "copanion",
    version,
    about = "Open a persistent source-learning session with inline notes and follow-up questions.",
    after_help = "Sessions are always stored under the user data directory, usually ~/.local/share/copanion/sessions/."
)]
pub struct Cli {
    /// Source files to attach to this learning session.
    #[arg(value_name = "FILE", value_hint = ValueHint::FilePath)]
    files: Vec<PathBuf>,
    /// Stable session name. Defaults to a deterministic id derived from the current directory.
    #[arg(short, long)]
    session: Option<String>,
    /// Title to use when a new session is created.
    #[arg(long)]
    title: Option<String>,
    /// Recreate the saved session while preserving the tracked files when none are supplied.
    #[arg(long)]
    fresh: bool,
    /// Print the session file path and exit.
    #[arg(long = "print-session-path")]
    print_session_path: bool,
    /// Export the open questions without opening the TUI.
    #[arg(long)]
    export: bool,
    /// Print exports to stdout instead of copying them to the clipboard.
    #[arg(long)]
    stdout: bool,
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
    let paths = SessionPaths::discover()?;
    paths.ensure_initialized()?;

    let session_id = cli
        .session
        .clone()
        .unwrap_or_else(|| storage::default_session_id(&cwd));
    let session_path = paths.session_path(&session_id);
    let existing = storage::read_packet_if_exists(&session_path)?;
    let mut packet = build_session(&cwd, &session_id, cli.title, cli.fresh, existing);

    let merged = storage::merge_files(&mut packet, &cwd, &cli.files);
    if merged {
        packet.touch();
    }

    storage::write_packet(&session_path, &packet)?;

    if cli.print_session_path {
        println!("{}", session_path.display());
        return Ok(());
    }

    if cli.export {
        let export = export::generate_question_export(&packet)?;
        if cli.stdout {
            print!("{export}");
        } else {
            let result = clipboard::copy_text(&export)?;
            println!("{result}");
        }
        return Ok(());
    }

    tui::run(&session_path, cli.stdout)
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

fn build_session(
    cwd: &std::path::Path,
    session_id: &str,
    title_override: Option<String>,
    fresh: bool,
    existing: Option<Packet>,
) -> Packet {
    let workspace_root = storage::workspace_root_string(cwd);
    match (existing, fresh) {
        (Some(mut packet), false) => {
            if let Some(title) = title_override {
                packet.title = title;
                packet.touch();
            }
            packet
        }
        (Some(packet), true) => Packet::new(
            session_id,
            title_override.unwrap_or(packet.title),
            workspace_root,
            packet.files,
        ),
        (None, _) => Packet::new(
            session_id,
            title_override.unwrap_or_else(|| {
                human_title(
                    cwd.file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or(session_id),
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
    use crate::model::{Packet, TrackedFile};
    use crate::theme::ThemeName;

    use super::{build_session, resolve_theme};

    #[test]
    fn fresh_session_preserves_existing_files() {
        let packet = Packet::new(
            "tour",
            "Tour",
            "/repo",
            vec![TrackedFile::new("src/main.rs")],
        );
        let rebuilt = build_session(Path::new("/repo"), "tour", None, true, Some(packet));
        assert_eq!(rebuilt.files.len(), 1);
        assert_eq!(rebuilt.files[0].path, "src/main.rs");
    }

    #[test]
    fn title_override_updates_loaded_session() {
        let packet = Packet::new("tour", "Old", "/repo", vec![]);
        let rebuilt = build_session(
            Path::new("/repo"),
            "tour",
            Some("New".to_string()),
            false,
            Some(packet),
        );
        assert_eq!(rebuilt.title, "New");
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
}
