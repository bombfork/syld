// SPDX-License-Identifier: GPL-3.0-or-later

//! Interactive first-run setup wizard.

use anyhow::{Context, Result};
use dialoguer::{Confirm, Input, Select};

use crate::config::{Cadence, Config};
use crate::install;

/// Run the interactive setup wizard.
pub fn run_setup(config: &Config) -> Result<()> {
    eprintln!("Welcome to syld setup!\n");

    // ── Stage 1: Configuration ──────────────────────────────────────────

    eprintln!("── Configuration ──\n");

    let enrich = Confirm::new()
        .with_prompt("Enable network enrichment? (fetches donation links, stars, etc.)")
        .default(config.enrich)
        .interact()
        .context("Failed to read enrichment preference")?;

    let currency: String = Input::new()
        .with_prompt("Budget currency code")
        .default(config.budget.currency.clone())
        .interact_text()
        .context("Failed to read currency")?;

    let cadence_options = ["monthly", "yearly"];
    let cadence_default = match config.budget.cadence {
        Cadence::Monthly => 0,
        Cadence::Yearly => 1,
    };
    let cadence_idx = Select::new()
        .with_prompt("Budget cadence")
        .items(&cadence_options)
        .default(cadence_default)
        .interact()
        .context("Failed to read cadence")?;
    let cadence = match cadence_idx {
        0 => Cadence::Monthly,
        _ => Cadence::Yearly,
    };

    let set_amount = Confirm::new()
        .with_prompt("Set a budget amount now?")
        .default(config.budget.amount.is_some())
        .interact()
        .context("Failed to read budget preference")?;

    let amount = if set_amount {
        let default_amount = config.budget.amount.unwrap_or(5.0);
        let amt: f64 = Input::new()
            .with_prompt(format!("Budget amount ({currency})"))
            .default(default_amount)
            .interact_text()
            .context("Failed to read budget amount")?;
        Some(amt)
    } else {
        config.budget.amount
    };

    // Save config
    let mut new_config = Config::load()?;
    new_config.enrich = enrich;
    new_config.budget.currency = currency.to_uppercase();
    new_config.budget.cadence = cadence;
    new_config.budget.amount = amount;
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

    // ── Stage 4: Initial scan ───────────────────────────────────────────

    eprintln!("── Initial scan ──\n");

    let run_scan = Confirm::new()
        .with_prompt("Run an initial scan now?")
        .default(true)
        .interact()
        .context("Failed to read scan preference")?;

    if run_scan {
        let binary = install::resolve_binary_path()?;
        let status = std::process::Command::new(&binary)
            .arg("scan")
            .status()
            .with_context(|| format!("Failed to run {}", binary.display()))?;

        if !status.success() {
            eprintln!("Warning: scan exited with {status}");
        }
    }

    eprintln!("\nSetup complete! Next steps:");
    eprintln!("  syld report      — review discovered packages");
    eprintln!("  syld budget plan — generate a donation plan");

    Ok(())
}
