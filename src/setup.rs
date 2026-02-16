// SPDX-License-Identifier: GPL-3.0-or-later

//! Interactive first-run setup wizard.

use anyhow::{Context, Result};
use dialoguer::{Confirm, Select};

use crate::config::Config;
use crate::install;

/// Run the interactive setup wizard.
pub fn run_setup(config: &Config) -> Result<()> {
    eprintln!("Welcome to syld setup!\n");

    // ── Stage 1: Configuration ──────────────────────────────────────────

    eprintln!("── Configuration ──\n");

    let enrich = Confirm::new()
        .with_prompt("Enable network enrichment? (fetches project metadata, stars, etc.)")
        .default(config.enrich)
        .interact()
        .context("Failed to read enrichment preference")?;

    // Save config
    let mut new_config = Config::load()?;
    new_config.enrich = enrich;
    new_config.save()?;
    eprintln!("\nConfiguration saved.\n");

    // ── Stage 2: Systemd timer ──────────────────────────────────────────

    eprintln!("── Systemd timer ──\n");

    let install_timer = Confirm::new()
        .with_prompt("Install a systemd user timer for periodic scans?")
        .default(true)
        .interact()
        .context("Failed to read timer preference")?;

    if install_timer {
        let freq_options = ["daily", "weekly", "monthly"];
        let freq_idx = Select::new()
            .with_prompt("Scan frequency")
            .items(&freq_options)
            .default(1) // weekly
            .interact()
            .context("Failed to read frequency")?;
        let frequency = freq_options[freq_idx];

        let enable_now = Confirm::new()
            .with_prompt("Enable and start the timer now?")
            .default(true)
            .interact()
            .context("Failed to read enable preference")?;

        install::service::install_service(frequency, enable_now)?;
        eprintln!();
    }

    // ── Stage 3: Hooks ──────────────────────────────────────────────────

    eprintln!("── Package manager hooks ──\n");

    let hooks: Vec<_> = install::hook_install::installable_hooks()
        .into_iter()
        .filter(|h| h.available)
        .collect();

    if hooks.is_empty() {
        eprintln!("No installable hooks detected for this system.\n");
    } else {
        for hook in &hooks {
            let install_hook = Confirm::new()
                .with_prompt(format!(
                    "Install {} hook? ({})",
                    hook.name, hook.description
                ))
                .default(true)
                .interact()
                .context("Failed to read hook preference")?;

            if install_hook {
                (hook.install_fn)()?;
            }
        }
        eprintln!();
    }

    // ── Stage 4: Initial report ────────────────────────────────────────

    eprintln!("── Initial report ──\n");

    let run_report = Confirm::new()
        .with_prompt(
            "Generate an initial report now? (scans your system and fetches project metadata \
             — may take a few minutes on first run)",
        )
        .default(true)
        .interact()
        .context("Failed to read report preference")?;

    if run_report {
        let binary = install::resolve_binary_path()?;
        let status = std::process::Command::new(&binary)
            .arg("report")
            .status()
            .with_context(|| format!("Failed to run {}", binary.display()))?;

        if !status.success() {
            eprintln!("Warning: report exited with {status}");
        }
    }

    eprintln!("\nSetup complete! Next steps:");
    eprintln!("  syld report — review discovered packages");

    Ok(())
}
