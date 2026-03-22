// SPDX-License-Identifier: GPL-3.0-or-later

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    /// Whether to enable network-based enrichment by default
    #[serde(default)]
    pub enrich: bool,

    /// Number of parallel enrichment threads (default: 4)
    #[serde(default)]
    pub enrich_jobs: Option<usize>,

    /// Custom beginner-friendly labels for discovering issues
    /// If not specified, defaults to common labels like "good first issue"
    #[serde(default)]
    pub beginner_labels: Option<Vec<String>>,
}

impl Config {
    /// Load configuration from XDG config directory.
    /// Returns default config if the file doesn't exist yet.
    pub fn load() -> Result<Self> {
        let path = Self::config_path()?;

        if !path.exists() {
            return Ok(Config::default());
        }

        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config from {}", path.display()))?;

        toml::from_str(&content)
            .with_context(|| format!("Failed to parse config from {}", path.display()))
    }

    /// Path to the configuration file.
    pub fn config_path() -> Result<PathBuf> {
        let dirs = project_dirs()?;
        Ok(dirs.config_dir().join("config.toml"))
    }

    /// Save configuration to the XDG config path.
    pub fn save(&self) -> Result<()> {
        let path = Self::config_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory {}", parent.display()))?;
        }
        let toml = toml::to_string_pretty(self).context("Failed to serialize config")?;
        fs::write(&path, &toml)
            .with_context(|| format!("Failed to write config to {}", path.display()))?;
        Ok(())
    }

    /// Path to the data directory.
    pub fn data_dir() -> Result<PathBuf> {
        let dirs = project_dirs()?;
        Ok(dirs.data_dir().to_path_buf())
    }

    /// Path to the cache directory.
    pub fn cache_dir() -> Result<PathBuf> {
        let dirs = project_dirs()?;
        Ok(dirs.cache_dir().to_path_buf())
    }
}

fn project_dirs() -> Result<ProjectDirs> {
    ProjectDirs::from("", "", "syld").context("Could not determine home directory")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_full_config() {
        let toml = r#"
enrich = true
enrich_jobs = 8
beginner_labels = ["good first issue", "help wanted", "beginner friendly"]
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert!(config.enrich);
        assert_eq!(config.enrich_jobs, Some(8));
        assert_eq!(
            config.beginner_labels,
            Some(vec![
                "good first issue".to_string(),
                "help wanted".to_string(),
                "beginner friendly".to_string()
            ])
        );
    }

    #[test]
    fn parse_empty_config() {
        let config: Config = toml::from_str("").unwrap();
        assert!(!config.enrich);
        assert_eq!(config.enrich_jobs, None);
        assert_eq!(config.beginner_labels, None);
    }

    #[test]
    fn config_paths_are_under_syld() {
        let path = Config::config_path().unwrap();
        assert!(path.to_string_lossy().contains("syld"));
        assert!(path.to_string_lossy().ends_with("config.toml"));
    }
}
