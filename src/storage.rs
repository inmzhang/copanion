use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use directories::BaseDirs;

use crate::model::Packet;
use crate::util::slugify;

pub const PACKETS_DIR_NAME: &str = "packets";

#[derive(Debug, Clone)]
pub struct StoragePaths {
    pub packets_dir: PathBuf,
}

impl StoragePaths {
    pub fn discover() -> Result<Self> {
        let base_dirs = BaseDirs::new().context("failed to discover the user data directory")?;
        let data_dir = base_dirs.data_local_dir().join("copanion");
        let packets_dir = data_dir.join(PACKETS_DIR_NAME);
        Ok(Self { packets_dir })
    }

    pub fn ensure_initialized(&self) -> Result<()> {
        fs::create_dir_all(&self.packets_dir).with_context(|| {
            format!(
                "failed to create the packet directory at {}",
                self.packets_dir.display()
            )
        })?;
        Ok(())
    }

    pub fn project_packet_path(&self, root: &Path) -> PathBuf {
        self.packets_dir
            .join(format!("{}.toml", project_packet_id(root)))
    }
}

pub fn discover_project_root(start: &Path) -> PathBuf {
    let normalized = start.canonicalize().unwrap_or_else(|_| start.to_path_buf());
    for candidate in normalized.ancestors() {
        let git_marker = candidate.join(".git");
        if git_marker.is_dir() || git_marker.is_file() {
            return candidate.to_path_buf();
        }
    }
    normalized
}

pub fn project_packet_id(root: &Path) -> String {
    let stem = root
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("packet");
    let normalized_root = root.to_string_lossy().replace('\\', "/");
    let suffix = stable_path_hash(&normalized_root) as u32;
    format!("{}-{suffix:08x}", slugify(stem))
}

pub fn legacy_default_session_path(root: &Path) -> Result<Option<PathBuf>> {
    let base_dirs = BaseDirs::new().context("failed to discover the user data directory")?;
    let data_dir = base_dirs.data_local_dir().join("copanion");
    let sessions_dir = data_dir.join("sessions");
    let session_path = sessions_dir.join(format!("{}.toml", project_packet_id(root)));
    Ok(session_path.exists().then_some(session_path))
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
        .with_context(|| format!("failed to read packet from {}", path.display()))?;
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
    let raw = toml::to_string_pretty(packet).context("failed to render packet as TOML")?;
    fs::write(path, raw).with_context(|| format!("failed to write {}", path.display()))
}

pub fn workspace_root_string(root: &Path) -> String {
    root.to_string_lossy().into_owned()
}

fn stable_path_hash(path: &str) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x00000100000001b3;

    let mut hash = FNV_OFFSET_BASIS;
    for byte in path.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use tempfile::tempdir;

    use super::{
        StoragePaths, discover_project_root, normalize_repo_path, project_packet_id,
        read_packet_if_exists, workspace_root_string, write_packet,
    };
    use crate::model::Packet;

    #[test]
    fn project_root_falls_back_to_git_marker() {
        let temp = tempdir().unwrap();
        let root = temp.path().join("repo");
        let nested = root.join("src/tui");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir(root.join(".git")).unwrap();
        assert_eq!(discover_project_root(&nested), root);
    }

    #[test]
    fn packet_ids_are_stable_and_slugged() {
        let root = Path::new("/workspace/My Repo");
        let first = project_packet_id(root);
        let second = project_packet_id(root);
        assert_eq!(first, second);
        assert_eq!(first, "my-repo-0f85f5c2");
    }

    #[test]
    fn project_packet_path_lives_under_user_data_dir() {
        let temp = tempdir().unwrap();
        let paths = StoragePaths {
            packets_dir: temp.path().join("copanion/packets"),
        };
        let packet = paths.project_packet_path(Path::new("/workspace/repo"));
        assert!(packet.starts_with(paths.packets_dir));
        assert_eq!(
            packet.extension().and_then(|ext| ext.to_str()),
            Some("toml")
        );
    }

    #[test]
    fn normalize_absolute_paths_against_root() {
        let root = Path::new("/repo");
        let actual = normalize_repo_path(Path::new("/repo/src/lib.rs"), root);
        assert_eq!(actual, "src/lib.rs");
    }

    #[test]
    fn packet_roundtrip_preserves_workspace_root() {
        let temp = tempdir().unwrap();
        let packet_path = temp.path().join("packet.toml");
        let packet = Packet::new(
            "demo",
            "Demo",
            workspace_root_string(Path::new("/repo")),
            vec![],
        );
        write_packet(&packet_path, &packet).unwrap();
        let loaded = read_packet_if_exists(&packet_path).unwrap().unwrap();
        assert_eq!(loaded.workspace_root, "/repo");
    }
}
