// SPDX-License-Identifier: GPL-3.0-or-later

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    /// Budget configuration
    #[serde(default)]
    pub budget: BudgetConfig,

    /// Whether to enable network-based enrichment by default
    #[serde(default)]
    pub enrich: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct BudgetConfig {
    /// Monthly budget amount (in user's currency)
    pub amount: Option<f64>,

    /// Currency code (e.g., "USD", "EUR")
    #[serde(default = "default_currency")]
    pub currency: String,

    /// Budget cadence
    #[serde(default)]
    pub cadence: Cadence,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Cadence {
    #[default]
    Monthly,
    Yearly,
}

fn default_currency() -> String {
    "USD".to_string()
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
