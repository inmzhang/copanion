use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use toml::Value;

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct AppConfig {
    pub theme: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigLoadOutcome {
    pub config: Option<AppConfig>,
    pub warnings: Vec<String>,
}

pub fn config_path() -> Result<PathBuf> {
    let xdg_config_home = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from);
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let appdata = std::env::var_os("APPDATA").map(PathBuf::from);

    config_path_from_parts(xdg_config_home, home, appdata)
}

pub fn config_path_hint() -> &'static str {
    #[cfg(windows)]
    {
        r"%APPDATA%\copanion\config.toml"
    }

    #[cfg(not(windows))]
    {
        "$XDG_CONFIG_HOME/copanion/config.toml (default: ~/.config/copanion/config.toml)"
    }
}

fn config_path_from_parts(
    xdg_config_home: Option<PathBuf>,
    home: Option<PathBuf>,
    _appdata: Option<PathBuf>,
) -> Result<PathBuf> {
    #[cfg(windows)]
    {
        let base = _appdata
            .filter(|path| !path.as_os_str().is_empty())
            .ok_or_else(|| anyhow!("Could not determine APPDATA for config directory"))?;
        return Ok(base.join("copanion").join("config.toml"));
    }

    #[cfg(not(windows))]
    {
        if let Some(base) = xdg_config_home.filter(|path| !path.as_os_str().is_empty()) {
            return Ok(base.join("copanion").join("config.toml"));
        }

        let home = home
            .filter(|path| !path.as_os_str().is_empty())
            .ok_or_else(|| anyhow!("Could not determine HOME for config directory"))?;
        Ok(home.join(".config").join("copanion").join("config.toml"))
    }
}

pub fn load_config() -> Result<ConfigLoadOutcome> {
    let path = config_path()?;
    load_config_from_path(&path)
}

fn load_config_from_path(path: &Path) -> Result<ConfigLoadOutcome> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(ConfigLoadOutcome::default()),
        Err(err) => return Err(err.into()),
    };

    let value: Value = toml::from_str(&contents)?;
    let table = value
        .as_table()
        .ok_or_else(|| anyhow!("Config root must be a TOML table"))?;

    let mut config = AppConfig::default();
    let mut warnings = Vec::new();

    if let Some(theme) = table.get("theme") {
        if let Some(theme_str) = theme.as_str() {
            config.theme = Some(theme_str.to_string());
        } else {
            warnings
                .push("Warning: Config key 'theme' must be a string; ignoring value".to_string());
        }
    }

    for key in table.keys() {
        if key != "theme" {
            warnings.push(format!("Warning: Unknown config key '{key}', ignoring"));
        }
    }

    Ok(ConfigLoadOutcome {
        config: Some(config),
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn missing_config_is_not_an_error() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let outcome = load_config_from_path(&path).unwrap();
        assert_eq!(outcome.config, None);
        assert!(outcome.warnings.is_empty());
    }

    #[test]
    fn loads_theme_value() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "theme = \"gruvbox-dark\"\n").unwrap();

        let outcome = load_config_from_path(&path).unwrap();
        assert_eq!(
            outcome
                .config
                .as_ref()
                .and_then(|config| config.theme.as_deref()),
            Some("gruvbox-dark")
        );
        assert!(outcome.warnings.is_empty());
    }

    #[test]
    fn warns_on_unknown_keys() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "theme = \"dark\"\nextra = true\n").unwrap();

        let outcome = load_config_from_path(&path).unwrap();
        assert_eq!(outcome.warnings.len(), 1);
        assert_eq!(
            outcome.warnings[0],
            "Warning: Unknown config key 'extra', ignoring"
        );
    }

    #[test]
    fn invalid_toml_fails() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "theme =\n").unwrap();
        assert!(load_config_from_path(&path).is_err());
    }
}
