// SPDX-License-Identifier: GPL-3.0-or-later

//! Hook file generation and installation.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::config::Config;

use super::{remove_with_elevated, resolve_binary_path, write_with_elevated};

/// An installable hook descriptor.
pub struct InstallableHook {
    /// Hook identifier (e.g. `"pacman-post-transaction"`).
    pub name: &'static str,
    /// Short description.
    pub description: &'static str,
    /// Whether the hook is relevant on this system.
    pub available: bool,
    /// Function to run the installation.
    pub install_fn: fn() -> Result<()>,
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

/// Install the pacman post-transaction hook.
///
/// Installs as `99-syld.hook` so it runs after all other ALPM hooks,
/// ensuring its output appears last and isn't buried among other messages.
/// Removes the old `syld.hook` if present to avoid running twice.
pub fn install_pacman_hook() -> Result<()> {
    let binary = resolve_binary_path()?;
    let data_dir = Config::data_dir().context("Failed to resolve data directory for hook")?;
    let db_path = data_dir.join("syld.db");
    let content = generate_pacman_hook(&binary, &db_path);

    // Remove the old hook path if it exists (renamed to 99-syld.hook)
    let old_path = PathBuf::from("/usr/share/libalpm/hooks/syld.hook");
    if old_path.exists() {
        let _ = remove_with_elevated(&old_path);
    }

    let path = PathBuf::from("/usr/share/libalpm/hooks/99-syld.hook");
    write_with_elevated(&path, &content)
}

/// Install the APT post-invoke hook.
///
/// Installs as `99-syld` in `/etc/apt/apt.conf.d/` so it runs after APT
/// finishes installing or upgrading packages. The `|| true` in the command
/// ensures hook errors never break APT operations.
pub fn install_apt_hook() -> Result<()> {
    let binary = resolve_binary_path()?;
    let data_dir = Config::data_dir().context("Failed to resolve data directory for hook")?;
    let db_path = data_dir.join("syld.db");
    let content = generate_apt_hook(&binary, &db_path);

    let path = PathBuf::from("/etc/apt/apt.conf.d/99-syld");
    write_with_elevated(&path, &content)
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

/// Install the DNF post-transaction hook.
///
/// Installs as `syld.action` in `/etc/dnf/plugins/post-transaction-actions.d/`
/// so it runs after DNF finishes installing or upgrading packages. This uses
/// the `post-transaction-actions` DNF plugin. The `|| true` in the command
/// ensures hook errors never break DNF operations.
pub fn install_dnf_hook() -> Result<()> {
    let binary = resolve_binary_path()?;
    let data_dir = Config::data_dir().context("Failed to resolve data directory for hook")?;
    let db_path = data_dir.join("syld.db");
    let content = generate_dnf_hook(&binary, &db_path);

    let path = PathBuf::from("/etc/dnf/plugins/post-transaction-actions.d/syld.action");
    write_with_elevated(&path, &content)
}

/// Return the registry of hooks that can be installed.
pub fn installable_hooks() -> Vec<InstallableHook> {
    vec![
        InstallableHook {
            name: "apt-post-invoke",
            description: "Run syld after APT installs/upgrades",
            available: Path::new("/var/lib/dpkg/status").is_file(),
            install_fn: install_apt_hook,
        },
        InstallableHook {
            name: "dnf-post-transaction",
            description: "Run syld after DNF installs/upgrades",
            available: Path::new("/var/lib/dnf").is_dir(),
            install_fn: install_dnf_hook,
        },
        InstallableHook {
            name: "pacman-post-transaction",
            description: "Run syld after pacman installs/upgrades",
            available: Path::new("/var/lib/pacman/local").is_dir(),
            install_fn: install_pacman_hook,
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
}
