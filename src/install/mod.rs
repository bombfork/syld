// SPDX-License-Identifier: GPL-3.0-or-later

//! Installation helpers for syld integrations (systemd timer, package manager hooks).

pub mod hook_install;
pub mod service;

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use directories::BaseDirs;

/// Resolve the path to the currently running syld binary.
pub fn resolve_binary_path() -> Result<PathBuf> {
    std::env::current_exe()
        .context("Failed to determine current executable path")?
        .canonicalize()
        .context("Failed to canonicalize executable path")
}

/// Return the systemd user unit directory (`~/.config/systemd/user/`), creating
/// it if it does not exist.
pub fn systemd_user_dir() -> Result<PathBuf> {
    let base = BaseDirs::new().context("Could not determine home directory")?;
    let dir = base.config_dir().join("systemd/user");
    std::fs::create_dir_all(&dir).with_context(|| format!("Failed to create {}", dir.display()))?;
    Ok(dir)
}

/// Write `content` to `path`. If the direct write fails due to permissions,
/// fall back to `sudo tee`. If that also fails, print manual instructions.
pub fn write_with_elevated(path: &Path, content: &str) -> Result<()> {
    // Try direct write first
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match std::fs::write(path, content) {
        Ok(()) => {
            eprintln!("Wrote {}", path.display());
            return Ok(());
        }
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            eprintln!(
                "Direct write to {} failed (permission denied), trying sudo...",
                path.display()
            );
        }
        Err(e) => return Err(e).with_context(|| format!("Failed to write {}", path.display())),
    }

    // Try sudo tee
    let status = Command::new("sudo")
        .args(["tee", &path.to_string_lossy()])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(ref mut stdin) = child.stdin {
                stdin.write_all(content.as_bytes())?;
            }
            child.wait()
        });

    match status {
        Ok(s) if s.success() => {
            eprintln!("Wrote {} (via sudo)", path.display());
            Ok(())
        }
        _ => {
            eprintln!(
                "\nCould not write {}. Create it manually with:\n",
                path.display()
            );
            eprintln!("sudo tee {} << 'EOF'\n{}EOF\n", path.display(), content);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_binary_path_returns_existing_path() {
        let path = resolve_binary_path().unwrap();
        assert!(path.exists(), "resolved binary path should exist");
    }
}
