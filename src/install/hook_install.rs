// SPDX-License-Identifier: GPL-3.0-or-later

//! Hook file generation and installation.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::config::Config;

use super::{remove_with_elevated, resolve_binary_path, write_with_elevated};

/// Install path for the pacman ALPM hook.
///
/// Named `99-syld.hook` so it runs after all other ALPM hooks, ensuring its
/// output appears last and isn't buried among other messages.
pub const PACMAN_HOOK_PATH: &str = "/usr/share/libalpm/hooks/99-syld.hook";

/// Install path for the APT post-invoke configuration.
pub const APT_HOOK_PATH: &str = "/etc/apt/apt.conf.d/99-syld";

/// Install path for the DNF post-transaction-actions action file.
///
/// Requires the `post-transaction-actions` DNF plugin.
pub const DNF_HOOK_PATH: &str = "/etc/dnf/plugins/post-transaction-actions.d/syld.action";

/// An installable hook descriptor.
///
/// Each hook declares where it is installed and how its file contents are
/// generated, so install, uninstall, and installed-detection are generic
/// operations over descriptors.
pub struct InstallableHook {
    /// Hook identifier (e.g. `"pacman-post-transaction"`).
    pub name: &'static str,
    /// Short description.
    pub description: &'static str,
    /// Whether the hook is relevant on this system.
    pub available: bool,
    /// Path the hook file is installed to.
    pub install_path: &'static str,
    /// Function generating the hook file contents.
    pub content_fn: fn() -> Result<String>,
}

impl InstallableHook {
    /// Whether the hook file is present on disk.
    pub fn is_installed(&self) -> bool {
        Path::new(self.install_path).exists()
    }

    /// Write the hook file to its install path.
    pub fn install(&self) -> Result<()> {
        let content = (self.content_fn)()?;
        write_with_elevated(Path::new(self.install_path), &content)
    }

    /// Remove the hook file from its install path.
    pub fn uninstall(&self) -> Result<()> {
        remove_with_elevated(Path::new(self.install_path))
    }
}

/// Generate the contents of a pacman ALPM hook file.
///
/// `db_path` is baked into the `Exec` line so the hook works correctly
/// even when pacman runs as root (where `$HOME` would resolve to `/root`).
pub fn generate_pacman_hook(binary_path: &Path, db_path: &Path) -> String {
    format!(
        "\
[Trigger]
Operation = Install
Operation = Upgrade
Type = Package
Target = *

[Action]
Description = Displaying open source contribution opportunities...
When = PostTransaction
Exec = {} hook run pacman-post-transaction --db-path {}
NeedsTargets
",
        binary_path.display(),
        db_path.display()
    )
}

/// Generate the contents of an APT post-invoke configuration file.
///
/// `db_path` is baked into the command so the hook works correctly
/// even when APT runs as root (where `$HOME` would resolve to `/root`).
/// The `|| true` suffix ensures hook errors never break APT operations.
pub fn generate_apt_hook(binary_path: &Path, db_path: &Path) -> String {
    format!(
        "DPkg::Post-Invoke {{\"{} hook run apt-post-invoke --db-path {} || true\";}};",
        binary_path.display(),
        db_path.display()
    )
}

/// Generate the contents of a DNF post-transaction-actions action file.
///
/// `db_path` is baked into the command so the hook works correctly
/// even when DNF runs as root (where `$HOME` would resolve to `/root`).
/// The `|| true` suffix ensures hook errors never break DNF operations.
pub fn generate_dnf_hook(binary_path: &Path, db_path: &Path) -> String {
    format!(
        "*:any:{} hook run dnf-post-transaction --db-path {} || true",
        binary_path.display(),
        db_path.display()
    )
}

/// Resolve the syld binary and database paths baked into hook commands.
fn binary_and_db_path() -> Result<(PathBuf, PathBuf)> {
    let binary = resolve_binary_path()?;
    let data_dir = Config::data_dir().context("Failed to resolve data directory for hook")?;
    Ok((binary, data_dir.join("syld.db")))
}

fn pacman_hook_content() -> Result<String> {
    let (binary, db_path) = binary_and_db_path()?;
    Ok(generate_pacman_hook(&binary, &db_path))
}

fn apt_hook_content() -> Result<String> {
    let (binary, db_path) = binary_and_db_path()?;
    Ok(generate_apt_hook(&binary, &db_path))
}

fn dnf_hook_content() -> Result<String> {
    let (binary, db_path) = binary_and_db_path()?;
    Ok(generate_dnf_hook(&binary, &db_path))
}

/// Return the registry of hooks that can be installed.
pub fn installable_hooks() -> Vec<InstallableHook> {
    vec![
        InstallableHook {
            name: "apt-post-invoke",
            description: "Run syld after APT installs/upgrades",
            available: Path::new("/var/lib/dpkg/status").is_file(),
            install_path: APT_HOOK_PATH,
            content_fn: apt_hook_content,
        },
        InstallableHook {
            name: "dnf-post-transaction",
            description: "Run syld after DNF installs/upgrades",
            available: Path::new("/var/lib/dnf").is_dir(),
            install_path: DNF_HOOK_PATH,
            content_fn: dnf_hook_content,
        },
        InstallableHook {
            name: "pacman-post-transaction",
            description: "Run syld after pacman installs/upgrades",
            available: Path::new("/var/lib/pacman/local").is_dir(),
            install_path: PACMAN_HOOK_PATH,
            content_fn: pacman_hook_content,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn generate_pacman_hook_interpolates_binary_and_db_path() {
        let binary = PathBuf::from("/home/user/.cargo/bin/syld");
        let db = PathBuf::from("/home/user/.local/share/syld/syld.db");
        let content = generate_pacman_hook(&binary, &db);
        assert!(content.contains(
            "Exec = /home/user/.cargo/bin/syld hook run pacman-post-transaction --db-path /home/user/.local/share/syld/syld.db"
        ));
        assert!(content.contains("NeedsTargets"));
    }

    #[test]
    fn generate_apt_hook_interpolates_binary_and_db_path() {
        let binary = PathBuf::from("/home/user/.cargo/bin/syld");
        let db = PathBuf::from("/home/user/.local/share/syld/syld.db");
        let content = generate_apt_hook(&binary, &db);
        assert!(content.contains(
            "/home/user/.cargo/bin/syld hook run apt-post-invoke --db-path /home/user/.local/share/syld/syld.db"
        ));
        assert!(content.contains("|| true"));
    }

    #[test]
    fn generate_dnf_hook_interpolates_binary_and_db_path() {
        let binary = PathBuf::from("/home/user/.cargo/bin/syld");
        let db = PathBuf::from("/home/user/.local/share/syld/syld.db");
        let content = generate_dnf_hook(&binary, &db);
        assert!(content.contains(
            "/home/user/.cargo/bin/syld hook run dnf-post-transaction --db-path /home/user/.local/share/syld/syld.db"
        ));
        assert!(content.contains("|| true"));
    }

    #[test]
    fn installable_hooks_contains_all() {
        let hooks = installable_hooks();
        assert_eq!(hooks.len(), 3);
        assert!(hooks.iter().any(|h| h.name == "apt-post-invoke"));
        assert!(hooks.iter().any(|h| h.name == "dnf-post-transaction"));
        assert!(hooks.iter().any(|h| h.name == "pacman-post-transaction"));
    }

    #[test]
    fn installable_hooks_have_absolute_unique_paths() {
        let hooks = installable_hooks();
        for hook in &hooks {
            assert!(
                hook.install_path.starts_with('/'),
                "install path for {} is not absolute: {}",
                hook.name,
                hook.install_path
            );
        }
        let mut paths: Vec<_> = hooks.iter().map(|h| h.install_path).collect();
        paths.sort_unstable();
        paths.dedup();
        assert_eq!(paths.len(), hooks.len(), "install paths must be unique");
        let mut names: Vec<_> = hooks.iter().map(|h| h.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), hooks.len(), "hook names must be unique");
    }
}
