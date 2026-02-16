// SPDX-License-Identifier: GPL-3.0-or-later

//! Systemd user service and timer generation and installation.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};

use super::{resolve_binary_path, systemd_user_dir};

/// Generate the contents of a systemd user service unit file.
pub fn generate_service(binary_path: &Path) -> String {
    format!(
        "\
[Unit]
Description=Support Your Linux Desktop — periodic scan
Documentation=https://github.com/bombfork/syld

[Service]
Type=oneshot
ExecStart={} scan
",
        binary_path.display()
    )
}

/// Generate the contents of a systemd user timer unit file.
pub fn generate_timer(calendar: &str) -> String {
    format!(
        "\
[Unit]
Description=Support Your Linux Desktop — {calendar} scan timer
Documentation=https://github.com/bombfork/syld

[Timer]
OnCalendar={calendar}
Persistent=true
RandomizedDelaySec=3600

[Install]
WantedBy=timers.target
"
    )
}

/// Install the systemd user service and timer.
///
/// Writes `syld.service` and `syld.timer` to `~/.config/systemd/user/`,
/// runs `systemctl --user daemon-reload`, and optionally enables the timer.
pub fn install_service(calendar: &str, enable: bool) -> Result<()> {
    let binary = resolve_binary_path()?;
    let dir = systemd_user_dir()?;

    let service_path = dir.join("syld.service");
    let timer_path = dir.join("syld.timer");

    std::fs::write(&service_path, generate_service(&binary))
        .with_context(|| format!("Failed to write {}", service_path.display()))?;
    eprintln!("Wrote {}", service_path.display());

    std::fs::write(&timer_path, generate_timer(calendar))
        .with_context(|| format!("Failed to write {}", timer_path.display()))?;
    eprintln!("Wrote {}", timer_path.display());

    let status = Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status()
        .context("Failed to run systemctl --user daemon-reload")?;

    if !status.success() {
        eprintln!("Warning: systemctl --user daemon-reload exited with {status}");
    }

    if enable {
        let status = Command::new("systemctl")
            .args(["--user", "enable", "--now", "syld.timer"])
            .status()
            .context("Failed to run systemctl --user enable --now syld.timer")?;

        if status.success() {
            eprintln!("Timer enabled and started.");
        } else {
            eprintln!("Warning: failed to enable timer (exit {status})");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn generate_service_interpolates_binary_path() {
        let path = PathBuf::from("/usr/local/bin/syld");
        let content = generate_service(&path);
        assert!(content.contains("ExecStart=/usr/local/bin/syld scan"));
    }

    #[test]
    fn generate_timer_uses_calendar_value() {
        let content = generate_timer("daily");
        assert!(content.contains("OnCalendar=daily"));
        assert!(content.contains("daily scan timer"));
    }

    #[test]
    fn generate_timer_weekly() {
        let content = generate_timer("weekly");
        assert!(content.contains("OnCalendar=weekly"));
    }
}
