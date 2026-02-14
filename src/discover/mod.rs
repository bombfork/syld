// SPDX-License-Identifier: GPL-3.0-or-later

//! Package discovery system.
//!
//! This module provides a pluggable framework for discovering software packages
//! installed on the local system. Each supported package manager is represented
//! by a *backend* that implements the [`Discoverer`] trait. At startup the
//! application calls [`active_discoverers()`] to obtain the subset of backends
//! that are actually present on the current machine, and then queries each one
//! for its installed packages.
//!
//! # Adding a new backend
//!
//! 1. Create a new sub-module (e.g. `apt.rs`) and implement [`Discoverer`] for
//!    a unit struct representing the backend.
//! 2. Add a corresponding variant to [`PackageSource`] so packages can be
//!    attributed to the new manager.
//! 3. Register the backend in [`active_discoverers()`] by appending a
//!    `Box::new(...)` entry to the `candidates` vector.
//!
//! See [`pacman::PacmanDiscoverer`] for a reference implementation.

mod apt;
mod dnf;
mod flatpak;
mod mise;
mod nix;
mod pacman;
mod snap;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::config::Config;

/// A discovered package installed on the system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledPackage {
    /// Package name as reported by the package manager.
    ///
    /// This is the raw, un-normalised name (e.g. `linux-headers` for pacman,
    /// `linux-headers-6.1.0-18-amd64` for apt). Do not attempt to canonicalise
    /// names across different package managers.
    pub name: String,
    /// Version string as reported by the package manager.
    ///
    /// The format is manager-specific (e.g. `6.8.1-1` for pacman,
    /// `6.1.0-18` for apt). Comparisons across different backends are not
    /// meaningful.
    pub version: String,
    /// Optional short, human-readable description of the package.
    ///
    /// May be `None` if the backend does not provide description metadata.
    pub description: Option<String>,
    /// Optional upstream project homepage URL.
    ///
    /// Used for grouping packages that belong to the same upstream project.
    /// May be `None` when the backend does not expose homepage information.
    pub url: Option<String>,
    /// Which package manager discovered this package.
    ///
    /// Maps directly to a [`PackageSource`] variant so that downstream code
    /// (reports, storage) can partition results by origin.
    pub source: PackageSource,
    /// Software license(s) associated with the package.
    ///
    /// Entries should be SPDX license identifiers when the package manager
    /// provides them (e.g. `GPL-3.0-or-later`). When SPDX identifiers are
    /// not available, the raw license strings reported by the package manager
    /// are stored instead.
    pub licenses: Vec<String>,
}

/// The package manager that installed a package.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PackageSource {
    Pacman,
    Apt,
    Dnf,
    Flatpak,
    Snap,
    Nix,
    Mise,
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
            PackageSource::Mise => write!(f, "mise"),
        }
    }
}

/// Trait for package manager backends.
///
/// Each implementation represents a single package manager (e.g. pacman, apt).
/// The lifecycle is:
///
/// 1. The backend is instantiated unconditionally.
/// 2. [`Discoverer::is_available()`] is called to check whether the package
///    manager is present on this system.
/// 3. If available, [`Discoverer::discover()`] is called to enumerate every
///    installed package.
pub trait Discoverer {
    /// A stable, lowercase identifier for this package manager.
    ///
    /// The value is used as a key in reports, storage, and log output, so it
    /// **must not change** between releases. By convention it should match the
    /// [`PackageSource`] display string (e.g. `"pacman"`, `"apt"`).
    fn name(&self) -> &str;

    /// Returns `true` if this package manager is installed on the current
    /// system.
    ///
    /// This method is called at startup to filter the set of active backends.
    /// It must be **cheap and fast** -- ideally limited to checking whether a
    /// well-known path exists (e.g. `/var/lib/pacman`). Avoid spawning
    /// subprocesses or performing network I/O here.
    fn is_available(&self) -> bool;

    /// Enumerates every package currently installed by this package manager.
    ///
    /// Unlike [`is_available()`](Discoverer::is_available), this method is
    /// expected to perform real I/O (reading databases, parsing files, running
    /// helper commands). Implementations should report progress via `indicatif`
    /// when feasible.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying package database cannot be read or
    /// parsed. The caller will log the error and continue with other backends.
    fn discover(&self) -> Result<Vec<InstalledPackage>>;
}

/// Returns all discoverers that are available on the current system.
///
/// Every known backend is instantiated and then filtered through
/// [`Discoverer::is_available()`]. Only backends whose package manager is
/// actually present are returned.
///
/// # Registering a new backend
///
/// To add support for another package manager, append a `Box::new(YourDiscoverer)`
/// entry to the `candidates` vector below. The new backend will automatically
/// be included whenever its [`is_available()`](Discoverer::is_available)
/// check passes.
pub fn active_discoverers(_config: &Config) -> Vec<Box<dyn Discoverer>> {
    let candidates: Vec<Box<dyn Discoverer>> = vec![
        Box::new(apt::AptDiscoverer),
        Box::new(dnf::DnfDiscoverer),
        Box::new(pacman::PacmanDiscoverer),
        Box::new(flatpak::FlatpakDiscoverer),
        Box::new(snap::SnapDiscoverer),
        Box::new(nix::NixDiscoverer),
        Box::new(mise::MiseDiscoverer),
    ];

    candidates
        .into_iter()
        .filter(|d| d.is_available())
        .collect()
}
