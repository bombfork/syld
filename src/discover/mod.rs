// SPDX-License-Identifier: GPL-3.0-or-later

mod pacman;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::config::Config;

/// A discovered package installed on the system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledPackage {
    /// Package name as known by the package manager
    pub name: String,
    /// Installed version
    pub version: String,
    /// Human-readable description
    pub description: Option<String>,
    /// Upstream project URL (homepage)
    pub url: Option<String>,
    /// Which package manager provided this package
    pub source: PackageSource,
    /// Software license(s)
    pub licenses: Vec<String>,
}

/// The package manager that installed a package.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum PackageSource {
    Pacman,
    Apt,
    Dnf,
    Flatpak,
    Snap,
    Nix,
}

impl std::fmt::Display for PackageSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PackageSource::Pacman => write!(f, "pacman"),
            PackageSource::Apt => write!(f, "apt"),
            PackageSource::Dnf => write!(f, "dnf"),
            PackageSource::Flatpak => write!(f, "flatpak"),
            PackageSource::Snap => write!(f, "snap"),
            PackageSource::Nix => write!(f, "nix"),
        }
    }
}

/// Trait for package manager backends.
pub trait Discoverer {
    /// Human-readable name of this package manager.
    fn name(&self) -> &str;

    /// Whether this package manager is available on the current system.
    fn is_available(&self) -> bool;

    /// Discover all installed packages.
    fn discover(&self) -> Result<Vec<InstalledPackage>>;
}

/// Returns all discoverers that are available on the current system.
pub fn active_discoverers(_config: &Config) -> Vec<Box<dyn Discoverer>> {
    let candidates: Vec<Box<dyn Discoverer>> = vec![Box::new(pacman::PacmanDiscoverer)];

    candidates
        .into_iter()
        .filter(|d| d.is_available())
        .collect()
}
