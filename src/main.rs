// SPDX-License-Identifier: GPL-3.0-or-later

use std::env;
use std::fs;
use std::process::Command;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use syld::config::Config;
use syld::discover::{self, InstalledPackage};
use syld::enrich::EnrichmentMap;
use syld::hook::{self, HookContext};
use syld::install;
use syld::report::{ContributionMap, ContributionSummary, html, json, terminal};
use syld::storage::Storage;

#[derive(Parser)]
#[command(
    name = "syld",
    about = "Support Your Linux Desktop — discover and support the open source you use",
    version,
    after_help = "\
Workflow:
  1. First time     syld setup
  2. Discover       syld scan
  3. Review         syld report"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Discover installed open source packages
    Scan {
        /// Maximum number of projects to display (0 for all)
        #[arg(long, default_value = "20")]
        limit: usize,

        /// Scan only — save results to the database without printing the summary table
        #[arg(long)]
        silent: bool,
    },

    /// Generate a report from the last scan
    Report {
        /// Output format
        #[arg(long, default_value = "terminal")]
        format: ReportFormat,

        /// Force re-enrichment, bypassing the cache
        #[arg(long)]
        force_refresh: bool,

        /// Number of parallel enrichment threads
        #[arg(short = 'j', long)]
        jobs: Option<usize>,

        /// Run scan and enrichment, showing progress, but skip the final report output
        #[arg(long)]
        progress_only: bool,
    },

    /// Manage the local cache
    Cache {
        #[command(subcommand)]
        command: CacheCommands,
    },

    /// Show or edit configuration
    Config {
        #[command(subcommand)]
        command: Option<ConfigCommands>,
    },

    /// Manage package manager hooks (internal plumbing for ALPM hook integration)
    #[command(hide = true)]
    Hook {
        #[command(subcommand)]
        command: HookCommands,
    },

    /// Interactive first-time setup wizard
    Setup,

    /// Install syld integrations (systemd timer, package manager hooks)
    Install {
        #[command(subcommand)]
        command: InstallCommands,
    },
}

#[derive(Clone, clap::ValueEnum)]
enum ReportFormat {
    Terminal,
    Json,
    Html,
}

#[derive(Subcommand)]
enum CacheCommands {
    /// Clear the enrichment cache
    Clear,
}

#[derive(Subcommand)]
enum HookCommands {
    /// Run a named hook (reads target packages from stdin)
    Run {
        /// Hook name (e.g. pacman-post-transaction)
        name: String,
    },

    /// List all hooks with their availability status
    List,
}

#[derive(Subcommand)]
enum InstallCommands {
    /// Install systemd user service and timer for periodic scans
    Service {
        /// Timer frequency (daily, weekly, monthly)
        #[arg(long, default_value = "weekly")]
        frequency: String,

        /// Enable and start the timer immediately
        #[arg(long)]
        enable: bool,
    },

    /// Install package manager hook(s)
    Hook {
        /// Hook name (omit for interactive selection)
        name: Option<String>,
    },
}

#[derive(Subcommand)]
enum ConfigCommands {
    /// Show current configuration
    Show,

    /// Open configuration file in $EDITOR
    Edit,

    /// Set a configuration value
    Set {
        /// Configuration key (e.g. enrich, budget.amount)
        key: String,

        /// Value to set
        value: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = Config::load()?;

    match cli.command {
        None => cmd_scan(&config, 20, false),
        Some(Commands::Scan { limit, silent }) => cmd_scan(&config, limit, silent),
        Some(Commands::Report {
            format,
            force_refresh,
            jobs,
            progress_only,
        }) => cmd_report(&config, &format, force_refresh, jobs, progress_only),
        Some(Commands::Cache { command }) => cmd_cache(&command),
        Some(Commands::Config { command }) => cmd_config(&config, &command),
        Some(Commands::Hook { command }) => cmd_hook(&config, &command),
        Some(Commands::Setup) => cmd_setup(&config),
        Some(Commands::Install { command }) => cmd_install(&command),
    }
}

fn run_scan(config: &Config, silent: bool) -> Result<Vec<InstalledPackage>> {
    let discoverers = discover::active_discoverers(config);

    if discoverers.is_empty() {
        if !silent {
            eprintln!("No supported package managers detected on this system.");
        }
        return Ok(Vec::new());
    }

    let mut all_packages = Vec::new();
    for d in &discoverers {
        if !silent {
            eprintln!("Scanning {} packages...", d.name());
        }
        match d.discover() {
            Ok(packages) => {
                if !silent {
                    eprintln!("  Found {} packages", packages.len());
                }
                all_packages.extend(packages);
            }
            Err(e) => {
                eprintln!("  Error scanning {}: {}", d.name(), e);
            }
        }
    }

    if !silent {
        eprintln!("\nTotal: {} packages discovered", all_packages.len());
    }

    match Storage::open() {
        Ok(storage) => match storage.save_scan(&all_packages) {
            Ok(_) => {
                if !silent {
                    eprintln!("Scan saved ({} packages)", all_packages.len());
                }
            }
            Err(e) => eprintln!("Warning: failed to save scan: {e}"),
        },
        Err(e) => eprintln!("Warning: failed to open database: {e}"),
    }

    Ok(all_packages)
}

fn cmd_scan(config: &Config, limit: usize, silent: bool) -> Result<()> {
    let all_packages = run_scan(config, silent)?;

    if !silent {
        let mut sorted = all_packages;
        terminal::sort_packages(&mut sorted);
        terminal::print_summary(
            &sorted,
            limit,
            chrono::Utc::now(),
            &ContributionMap::new(),
            &EnrichmentMap::new(),
            None,
        );
    }

    Ok(())
}

fn cmd_report(
    config: &Config,
    format: &ReportFormat,
    force_refresh: bool,
    jobs: Option<usize>,
    progress_only: bool,
) -> Result<()> {
    let storage = Storage::open().context("Failed to open database")?;
    let scan = storage
        .latest_scan()
        .context("Failed to read latest scan")?;

    let scan = match scan {
        Some(s) => s,
        None => {
            eprintln!("No previous scan found. Running scan first\u{2026}");
            run_scan(config, true)?;
            let fresh = storage
                .latest_scan()
                .context("Failed to read scan after auto-scan")?;
            match fresh {
                Some(s) => s,
                None => {
                    eprintln!("Scan completed but no data was saved.");
                    return Ok(());
                }
            }
        }
    };

    // Run enrichment if enabled in config; --force-refresh bypasses cache
    let enrichment = if config.enrich || force_refresh {
        syld::enrich::enrich_packages(&scan.packages, &storage, config, force_refresh, jobs)?
    } else {
        syld::enrich::EnrichmentMap::new()
    };

    if progress_only {
        return Ok(());
    }

    let contributions = ContributionMap::new();

    // Build contribution summary from stored records
    let contribution_summary = storage
        .get_contributions(None, None)
        .ok()
        .map(|records| ContributionSummary::from_records(&records))
        .filter(|s| !s.is_empty());

    match format {
        ReportFormat::Terminal => {
            let mut packages = scan.packages;
            terminal::sort_packages(&mut packages);
            terminal::print_summary(
                &packages,
                0,
                scan.timestamp,
                &contributions,
                &enrichment,
                contribution_summary.as_ref(),
            );
        }
        ReportFormat::Json => {
            json::print_json(
                &scan.packages,
                scan.timestamp,
                &contributions,
                &enrichment,
                contribution_summary,
            )?;
        }
        ReportFormat::Html => {
            html::print_html(
                &scan.packages,
                scan.timestamp,
                &contributions,
                &enrichment,
                contribution_summary.as_ref(),
            );
        }
    }

    Ok(())
}

fn cmd_cache(command: &CacheCommands) -> Result<()> {
    match command {
        CacheCommands::Clear => {
            let storage = Storage::open().context("Failed to open database")?;
            storage.clear_cache()?;
            eprintln!("Cache cleared. Run `syld scan` to rebuild.");
            Ok(())
        }
    }
}

fn cmd_hook(config: &Config, command: &HookCommands) -> Result<()> {
    match command {
        HookCommands::Run { name } => cmd_hook_run(config, name),
        HookCommands::List => cmd_hook_list(),
    }
}

fn cmd_hook_run(config: &Config, name: &str) -> Result<()> {
    let hook = match hook::find_hook(name) {
        Some(h) => h,
        None => {
            anyhow::bail!("Unknown hook '{name}'. Run `syld hook list` to see available hooks.");
        }
    };

    if !hook.is_available() {
        anyhow::bail!("Hook '{name}' is not available on this system (missing prerequisites).");
    }

    let stdin = std::io::stdin();
    let targets: Vec<String> = std::io::BufRead::lines(stdin.lock())
        .filter_map(|line| {
            let line = line.ok()?;
            let trimmed = line.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        })
        .collect();

    let ctx = HookContext { config, targets };

    hook.run(&ctx)
}

fn cmd_hook_list() -> Result<()> {
    let hooks = hook::all_hooks();

    if hooks.is_empty() {
        eprintln!("No hooks registered.");
        return Ok(());
    }

    for h in &hooks {
        let status = if h.is_available() {
            "available"
        } else {
            "not available"
        };
        println!("  {:<30} {} [{}]", h.name(), h.description(), status);
    }

    Ok(())
}

fn cmd_config(config: &Config, command: &Option<ConfigCommands>) -> Result<()> {
    match command {
        None | Some(ConfigCommands::Show) => cmd_config_show(config),
        Some(ConfigCommands::Edit) => cmd_config_edit(),
        Some(ConfigCommands::Set { key, value }) => cmd_config_set(key, value),
    }
}

fn cmd_config_show(config: &Config) -> Result<()> {
    let path = Config::config_path()?;
    eprintln!("# {}", path.display());

    let toml = toml::to_string_pretty(config).context("Failed to serialize config")?;
    print!("{toml}");
    Ok(())
}

fn cmd_config_set(key: &str, value: &str) -> Result<()> {
    let mut config = Config::load()?;

    match key {
        "enrich" => {
            config.enrich = value
                .parse::<bool>()
                .with_context(|| format!("Invalid boolean '{value}'. Use 'true' or 'false'."))?;
        }
        "enrich_jobs" => {
            config.enrich_jobs = Some(
                value
                    .parse::<usize>()
                    .with_context(|| format!("Invalid number '{value}'."))?,
            );
        }
        _ => {
            anyhow::bail!("Unknown config key '{key}'. Valid keys: enrich, enrich_jobs");
        }
    }

    config.save()?;
    eprintln!("Set {key} = {value}");
    Ok(())
}

fn cmd_config_edit() -> Result<()> {
    let path = Config::config_path()?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory {}", parent.display()))?;
    }

    if !path.exists() {
        let default_toml = toml::to_string_pretty(&Config::default())
            .context("Failed to serialize default config")?;
        fs::write(&path, &default_toml)
            .with_context(|| format!("Failed to write default config to {}", path.display()))?;
        eprintln!("Created default config at {}", path.display());
    }

    let editor = env::var("VISUAL")
        .or_else(|_| env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".to_string());

    let status = Command::new(&editor)
        .arg(&path)
        .status()
        .with_context(|| format!("Failed to launch editor '{editor}'"))?;

    if !status.success() {
        anyhow::bail!("Editor '{editor}' exited with {status}");
    }

    Ok(())
}

fn cmd_setup(config: &Config) -> Result<()> {
    syld::setup::run_setup(config)
}

fn cmd_install(command: &InstallCommands) -> Result<()> {
    match command {
        InstallCommands::Service { frequency, enable } => {
            install::service::install_service(frequency, *enable)
        }
        InstallCommands::Hook { name } => cmd_install_hook(name.as_deref()),
    }
}

fn cmd_install_hook(name: Option<&str>) -> Result<()> {
    let hooks = install::hook_install::installable_hooks();

    if let Some(name) = name {
        let hook = hooks.into_iter().find(|h| h.name == name);
        match hook {
            Some(h) => (h.install_fn)(),
            None => {
                anyhow::bail!(
                    "Unknown hook '{name}'. Available hooks: {}",
                    install::hook_install::installable_hooks()
                        .iter()
                        .map(|h| h.name)
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
        }
    } else {
        let available: Vec<_> = hooks.into_iter().filter(|h| h.available).collect();
        if available.is_empty() {
            eprintln!("No installable hooks detected for this system.");
            return Ok(());
        }

        let labels: Vec<String> = available
            .iter()
            .map(|h| format!("{} — {}", h.name, h.description))
            .collect();

        let selection = dialoguer::Select::new()
            .with_prompt("Select a hook to install")
            .items(&labels)
            .default(0)
            .interact()
            .context("Failed to read selection")?;

        (available[selection].install_fn)()
    }
}
