// SPDX-License-Identifier: GPL-3.0-or-later

//! Installation helpers for syld integrations (systemd timer, package manager hooks).

pub mod hook_install;
pub mod service;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use anyhow::{Context, Result};
use directories::BaseDirs;

/// Why an elevated operation could not be completed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElevationError {
    /// sudo authentication failed (wrong password, declined, not in sudoers,
    /// or no terminal available for the prompt).
    AuthFailed,
    /// The user cancelled at the sudo password prompt (Ctrl-C / signal).
    Cancelled,
    /// Authentication succeeded but the elevated command itself failed.
    OperationFailed,
}

impl std::fmt::Display for ElevationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AuthFailed => write!(f, "sudo authentication failed"),
            Self::Cancelled => write!(f, "cancelled at sudo password prompt"),
            Self::OperationFailed => {
                write!(f, "elevated command failed (sudo authentication succeeded)")
            }
        }
    }
}

impl std::error::Error for ElevationError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SudoOutcome {
    Success,
    Cancelled,
    Failed,
}

/// Map a sudo exit status to an outcome. 130 is sudo's exit code for SIGINT
/// at the password prompt; a `None` code means the process was killed by a
/// signal directly. Never inspect sudo's stderr — it is gettext-localized.
fn classify_sudo_status(status: ExitStatus) -> SudoOutcome {
    match status.code() {
        Some(0) => SudoOutcome::Success,
        Some(130) | None => SudoOutcome::Cancelled,
        Some(_) => SudoOutcome::Failed,
    }
}

/// Run `sudo <flags...> <args...>`, piping `stdin_content` to the child if
/// given (stdout is nulled in that case to suppress `tee` echo). stderr is
/// always inherited so sudo's prompt and error messages reach the terminal.
fn run_sudo(flags: &[&str], args: &[&str], stdin_content: Option<&str>) -> Result<ExitStatus> {
    let mut cmd = Command::new("sudo");
    cmd.args(flags).args(args);

    let status = if let Some(content) = stdin_content {
        cmd.stdin(Stdio::piped()).stdout(Stdio::null());
        let mut child = cmd.spawn()?;
        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            // The child may exit before reading (e.g. refused auth); the
            // exit status is the source of truth, not the pipe write.
            match stdin.write_all(content.as_bytes()) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {}
                Err(e) => return Err(e.into()),
            }
        }
        child.wait()?
    } else {
        cmd.status()?
    };

    Ok(status)
}

/// Run an elevated command via sudo, separating authentication from the
/// operation so failures are attributable: `sudo -v` to authenticate, then
/// `sudo -n` for the operation, with one prompting retry to cover sudoers
/// configs where credentials are not cached (`timestamp_timeout=0`).
///
/// Only idempotent commands belong here — the retry re-runs the operation.
fn run_elevated(args: &[&str], stdin_content: Option<&str>) -> Result<()> {
    let status = run_sudo(&["-v"], &[], None).context("Failed to run sudo — is it installed?")?;
    match classify_sudo_status(status) {
        SudoOutcome::Success => {}
        SudoOutcome::Cancelled => return Err(ElevationError::Cancelled.into()),
        SudoOutcome::Failed => return Err(ElevationError::AuthFailed.into()),
    }

    let status = run_sudo(&["-n"], args, stdin_content).context("Failed to run sudo")?;
    if classify_sudo_status(status) == SudoOutcome::Success {
        return Ok(());
    }

    // Credentials may not have been cached by `sudo -v`; retry with prompting.
    // If authentication fails here it is reported as OperationFailed — `sudo -v`
    // already proved the user can authenticate, and sudo's own stderr is visible.
    let status = run_sudo(&[], args, stdin_content).context("Failed to run sudo")?;
    match classify_sudo_status(status) {
        SudoOutcome::Success => Ok(()),
        SudoOutcome::Cancelled => Err(ElevationError::Cancelled.into()),
        SudoOutcome::Failed => Err(ElevationError::OperationFailed.into()),
    }
}

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
/// fall back to `sudo tee`.
pub fn write_with_elevated(path: &Path, content: &str) -> Result<()> {
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

    run_elevated(&["tee", "--", &path.to_string_lossy()], Some(content))
        .with_context(|| format!("Failed to write {}", path.display()))?;
    eprintln!("Wrote {} (via sudo)", path.display());
    Ok(())
}

/// Remove `path` if it exists. If the direct removal fails due to
/// permissions, fall back to `sudo rm`.
pub fn remove_with_elevated(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }

    match std::fs::remove_file(path) {
        Ok(()) => {
            eprintln!("Removed {}", path.display());
            return Ok(());
        }
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            eprintln!(
                "Direct removal of {} failed (permission denied), trying sudo...",
                path.display()
            );
        }
        Err(e) => return Err(e).with_context(|| format!("Failed to remove {}", path.display())),
    }

    run_elevated(&["rm", "--", &path.to_string_lossy()], None)
        .with_context(|| format!("Failed to remove {}", path.display()))?;
    eprintln!("Removed {} (via sudo)", path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // The sudo paths (`run_sudo`, `run_elevated`) are deliberately untested:
    // exercising them requires a real sudoers config, a TTY, and interactive
    // authentication — non-hermetic and impossible in CI. That is why
    // `classify_sudo_status` is factored out: every non-process decision is
    // covered without spawning sudo.

    use std::os::unix::process::ExitStatusExt;

    #[test]
    fn resolve_binary_path_returns_existing_path() {
        let path = resolve_binary_path().unwrap();
        assert!(path.exists(), "resolved binary path should exist");
    }

    // Wait-status encoding: normal exit code n => n << 8; killed by signal s => s.
    #[test]
    fn classify_sudo_status_zero_is_success() {
        assert_eq!(
            classify_sudo_status(ExitStatus::from_raw(0)),
            SudoOutcome::Success
        );
    }

    #[test]
    fn classify_sudo_status_130_is_cancelled() {
        assert_eq!(
            classify_sudo_status(ExitStatus::from_raw(130 << 8)),
            SudoOutcome::Cancelled
        );
    }

    #[test]
    fn classify_sudo_status_signal_death_is_cancelled() {
        assert_eq!(
            classify_sudo_status(ExitStatus::from_raw(2)),
            SudoOutcome::Cancelled
        );
    }

    #[test]
    fn classify_sudo_status_nonzero_is_failed() {
        assert_eq!(
            classify_sudo_status(ExitStatus::from_raw(1 << 8)),
            SudoOutcome::Failed
        );
    }

    #[test]
    fn write_with_elevated_direct_write_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hook.conf");
        write_with_elevated(&path, "content").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "content");
    }

    #[test]
    fn remove_with_elevated_direct_removal_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hook.conf");
        std::fs::write(&path, "content").unwrap();
        remove_with_elevated(&path).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn remove_with_elevated_missing_path_is_ok() {
        let dir = tempfile::tempdir().unwrap();
        remove_with_elevated(&dir.path().join("nonexistent")).unwrap();
    }
}
