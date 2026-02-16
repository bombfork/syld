// SPDX-License-Identifier: GPL-3.0-or-later

//! Hook file generation and installation.

use std::path::{Path, PathBuf};

use anyhow::Result;

use super::{resolve_binary_path, write_with_elevated};

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
pub fn generate_pacman_hook(binary_path: &Path) -> String {
    format!(
        "\
[Trigger]
Operation = Install
Operation = Upgrade
Type = Package
Target = *

[Action]
Description = Checking open source contribution opportunities...
When = PostTransaction
Exec = {} hook run pacman-post-transaction
NeedsTargets
",
        binary_path.display()
    )
}

/// Install the pacman post-transaction hook.
pub fn install_pacman_hook() -> Result<()> {
    let binary = resolve_binary_path()?;
    let content = generate_pacman_hook(&binary);
    let path = PathBuf::from("/usr/share/libalpm/hooks/syld.hook");
    write_with_elevated(&path, &content)
}

/// Return the registry of hooks that can be installed.
pub fn installable_hooks() -> Vec<InstallableHook> {
    vec![InstallableHook {
        name: "pacman-post-transaction",
        description: "Run syld after pacman installs/upgrades",
        available: Path::new("/var/lib/pacman/local").is_dir(),
        install_fn: install_pacman_hook,
    }]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn generate_pacman_hook_interpolates_binary_path() {
        let path = PathBuf::from("/home/user/.cargo/bin/syld");
        let content = generate_pacman_hook(&path);
        assert!(
            content.contains("Exec = /home/user/.cargo/bin/syld hook run pacman-post-transaction")
        );
        assert!(content.contains("NeedsTargets"));
    }

    #[test]
    fn installable_hooks_contains_pacman() {
        let hooks = installable_hooks();
        assert_eq!(hooks.len(), 1);
        assert_eq!(hooks[0].name, "pacman-post-transaction");
    }
}
