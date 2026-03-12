use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::model::{Packet, TrackedFile};
use crate::util::slugify;

pub const COPANION_DIR: &str = ".copanion";
pub const PACKET_DIR: &str = "packets";

#[derive(Debug, Clone)]
pub struct ProjectPaths {
    pub root: PathBuf,
    pub copanion_dir: PathBuf,
    pub packets_dir: PathBuf,
}

impl ProjectPaths {
    pub fn from_root(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let copanion_dir = root.join(COPANION_DIR);
        let packets_dir = copanion_dir.join(PACKET_DIR);
        Self {
            root,
            copanion_dir,
            packets_dir,
        }
    }

    pub fn ensure_initialized(&self) -> Result<()> {
        fs::create_dir_all(&self.packets_dir).with_context(|| {
            format!(
                "failed to create packet directory at {}",
                self.packets_dir.display()
            )
        })?;
        Ok(())
    }

    pub fn packet_path(&self, packet_ref: &str) -> PathBuf {
        let candidate = Path::new(packet_ref);
        if candidate.components().count() > 1 || candidate.extension().is_some() {
            if candidate.is_absolute() {
                candidate.to_path_buf()
            } else {
                self.root.join(candidate)
            }
        } else {
            self.packets_dir
                .join(format!("{}.toml", slugify(packet_ref)))
        }
    }
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

pub fn write_packet(path: &Path, packet: &Packet) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let raw = toml::to_string_pretty(packet).context("failed to render packet as TOML")?;
    fs::write(path, raw).with_context(|| format!("failed to write {}", path.display()))
}

pub fn create_packet(
    paths: &ProjectPaths,
    packet_ref: &str,
    title: impl Into<String>,
    files: Vec<PathBuf>,
    overwrite: bool,
) -> Result<PathBuf> {
    paths.ensure_initialized()?;
    let packet_path = paths.packet_path(packet_ref);
    if packet_path.exists() && !overwrite {
        bail!(
            "packet already exists at {} (pass --force to overwrite)",
            packet_path.display()
        );
    }
    let tracked_files = files
        .into_iter()
        .map(|path| TrackedFile::new(normalize_repo_path(&path, &paths.root)))
        .collect();
    let packet = Packet::new(title, tracked_files);
    write_packet(&packet_path, &packet)?;
    Ok(packet_path)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use tempfile::tempdir;

    use super::{ProjectPaths, create_packet, normalize_repo_path, read_packet};

    #[test]
    fn packet_ref_without_extension_resolves_under_copanion() {
        let temp = tempdir().unwrap();
        let paths = ProjectPaths::from_root(temp.path());
        let packet = paths.packet_path("learning-pass");
        assert!(packet.ends_with(".copanion/packets/learning-pass.toml"));
    }

    #[test]
    fn packet_roundtrip_preserves_files() {
        let temp = tempdir().unwrap();
        let paths = ProjectPaths::from_root(temp.path());
        let packet_path = create_packet(
            &paths,
            "scheduler-tour",
            "Scheduler Tour",
            vec![Path::new("src/main.rs").to_path_buf()],
            false,
        )
        .unwrap();
        let packet = read_packet(&packet_path).unwrap();
        assert_eq!(packet.title, "Scheduler Tour");
        assert_eq!(packet.files.len(), 1);
        assert_eq!(packet.files[0].path, "src/main.rs");
    }

    #[test]
    fn normalize_absolute_paths_against_root() {
        let root = Path::new("/repo");
        let actual = normalize_repo_path(Path::new("/repo/src/lib.rs"), root);
        assert_eq!(actual, "src/lib.rs");
    }
}
