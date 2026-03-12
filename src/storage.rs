use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use directories::BaseDirs;

use crate::model::{Packet, TrackedFile};
use crate::util::slugify;

#[derive(Debug, Clone)]
pub struct SessionPaths {
    pub data_dir: PathBuf,
    pub sessions_dir: PathBuf,
}

impl SessionPaths {
    pub fn discover() -> Result<Self> {
        let base_dirs = BaseDirs::new().context("failed to discover the user data directory")?;
        let data_dir = base_dirs.data_local_dir().join("copanion");
        let sessions_dir = data_dir.join("sessions");
        Ok(Self {
            data_dir,
            sessions_dir,
        })
    }

    pub fn ensure_initialized(&self) -> Result<()> {
        fs::create_dir_all(&self.sessions_dir).with_context(|| {
            format!(
                "failed to create the session directory at {}",
                self.sessions_dir.display()
            )
        })?;
        Ok(())
    }

    pub fn session_path(&self, session_ref: &str) -> PathBuf {
        self.sessions_dir
            .join(format!("{}.toml", slugify(session_ref)))
    }
}

pub fn default_session_id(root: &Path) -> String {
    let stem = root
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("session");
    let mut hasher = DefaultHasher::new();
    root.hash(&mut hasher);
    let suffix = hasher.finish() as u32;
    format!("{}-{suffix:08x}", slugify(stem))
}

pub fn normalize_repo_path(path: &Path, root: &Path) -> String {
    let display_path = if path.is_absolute() {
        path.strip_prefix(root).unwrap_or(path).to_path_buf()
    } else {
        path.to_path_buf()
    };
    display_path.to_string_lossy().replace('\\', "/")
}

pub fn read_packet(path: &Path) -> Result<Packet> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read session from {}", path.display()))?;
    toml::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))
}

pub fn read_packet_if_exists(path: &Path) -> Result<Option<Packet>> {
    if path.exists() {
        read_packet(path).map(Some)
    } else {
        Ok(None)
    }
}

pub fn write_packet(path: &Path, packet: &Packet) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let raw = toml::to_string_pretty(packet).context("failed to render session as TOML")?;
    fs::write(path, raw).with_context(|| format!("failed to write {}", path.display()))
}

pub fn merge_files(packet: &mut Packet, root: &Path, files: &[PathBuf]) -> bool {
    let mut changed = false;
    for file in files {
        let normalized = normalize_repo_path(file, root);
        if packet
            .files
            .iter()
            .all(|tracked| tracked.path != normalized)
        {
            packet.files.push(TrackedFile::new(normalized));
            changed = true;
        }
    }
    changed
}

pub fn workspace_root_string(root: &Path) -> String {
    root.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use tempfile::tempdir;

    use super::{
        SessionPaths, default_session_id, merge_files, normalize_repo_path, read_packet_if_exists,
        workspace_root_string, write_packet,
    };
    use crate::model::Packet;

    #[test]
    fn session_path_lives_under_user_data_dir() {
        let temp = tempdir().unwrap();
        let paths = SessionPaths {
            data_dir: temp.path().join("copanion"),
            sessions_dir: temp.path().join("copanion/sessions"),
        };
        let packet = paths.session_path("learning-pass");
        assert!(packet.ends_with("copanion/sessions/learning-pass.toml"));
    }

    #[test]
    fn default_session_ids_are_stable_and_slugged() {
        let root = Path::new("/workspace/copanion");
        let first = default_session_id(root);
        let second = default_session_id(root);
        assert_eq!(first, second);
        assert!(first.starts_with("copanion-"));
    }

    #[test]
    fn merge_files_adds_new_entries_once() {
        let mut packet = Packet::new("demo", "Demo", "/repo", vec![]);
        let changed = merge_files(
            &mut packet,
            Path::new("/repo"),
            &[Path::new("src/main.rs").into()],
        );
        assert!(changed);
        assert_eq!(packet.files.len(), 1);
        let changed_again = merge_files(
            &mut packet,
            Path::new("/repo"),
            &[Path::new("src/main.rs").into()],
        );
        assert!(!changed_again);
    }

    #[test]
    fn normalize_absolute_paths_against_root() {
        let root = Path::new("/repo");
        let actual = normalize_repo_path(Path::new("/repo/src/lib.rs"), root);
        assert_eq!(actual, "src/lib.rs");
    }

    #[test]
    fn session_roundtrip_preserves_workspace_root() {
        let temp = tempdir().unwrap();
        let session_path = temp.path().join("sessions/demo.toml");
        let packet = Packet::new(
            "demo",
            "Demo",
            workspace_root_string(Path::new("/repo")),
            vec![],
        );
        write_packet(&session_path, &packet).unwrap();
        let loaded = read_packet_if_exists(&session_path).unwrap().unwrap();
        assert_eq!(loaded.workspace_root, "/repo");
    }
}
