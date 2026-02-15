// SPDX-License-Identifier: GPL-3.0-or-later

use std::env;
use std::fs;
use std::process::Command;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use syld::config::Config;
use syld::discover;
use syld::report::{ContributionMap, html, json, terminal};
use syld::storage::Storage;

#[derive(Parser)]
#[command(
    name = "syld",
    about = "Support Your Linux Desktop — discover and support the open source you use",
    version
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
    },

    /// Generate a report from the last scan
    Report {
        /// Output format
        #[arg(long, default_value = "terminal")]
        format: ReportFormat,

        /// Fetch additional info from the network (donation links, etc.)
        #[arg(long)]
        enrich: bool,
    },

    /// Manage your support budget
    Budget {
        #[command(subcommand)]
        command: BudgetCommands,
    },

    /// Show or edit configuration
    Config {
        #[command(subcommand)]
        command: Option<ConfigCommands>,
    },
}

#[derive(Clone, clap::ValueEnum)]
enum ReportFormat {
    Terminal,
    Json,
    Html,
}

#[derive(Subcommand)]
enum BudgetCommands {
    /// Set your monthly or yearly support budget
    Set {
        /// Amount in your local currency
        amount: f64,

        /// Budget cadence
        #[arg(long, default_value = "monthly")]
        cadence: BudgetCadence,
    },

    /// Generate a donation plan based on your budget
    Plan {
        /// Allocation strategy
        #[arg(long, default_value = "equal")]
        strategy: AllocationStrategy,
    },

    /// Show current budget settings
    Show,
}

#[derive(Clone, clap::ValueEnum)]
enum BudgetCadence {
    Monthly,
    Yearly,
}

#[derive(Clone, clap::ValueEnum)]
enum AllocationStrategy {
    Equal,
    Weighted,
}

#[derive(Subcommand)]
enum ConfigCommands {
    /// Show current configuration
    Show,

    /// Open configuration file in $EDITOR
    Edit,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = Config::load()?;

    match cli.command {
        None => cmd_scan(&config, 20),
        Some(Commands::Scan { limit }) => cmd_scan(&config, limit),
        Some(Commands::Report { format, enrich: _ }) => cmd_report(&config, &format),
        Some(Commands::Budget { command }) => cmd_budget(&config, &command),
        Some(Commands::Config { command }) => cmd_config(&config, &command),
    }
}

fn cmd_scan(config: &Config, limit: usize) -> Result<()> {
    let discoverers = discover::active_discoverers(config);

    if discoverers.is_empty() {
        eprintln!("No supported package managers detected on this system.");
        return Ok(());
    }

    let mut all_packages = Vec::new();
    for d in &discoverers {
        eprintln!("Scanning {} packages...", d.name());
        match d.discover() {
            Ok(packages) => {
                eprintln!("  Found {} packages", packages.len());
                all_packages.extend(packages);
            }
            Err(e) => {
                eprintln!("  Error scanning {}: {}", d.name(), e);
            }
        }
    }

    eprintln!("\nTotal: {} packages discovered", all_packages.len());

    match Storage::open() {
        Ok(storage) => match storage.save_scan(&all_packages) {
            Ok(_) => eprintln!("Scan saved ({} packages)", all_packages.len()),
            Err(e) => eprintln!("Warning: failed to save scan: {e}"),
        },
        Err(e) => eprintln!("Warning: failed to open database: {e}"),
    }

    terminal::sort_packages(&mut all_packages);
    terminal::print_summary(
        &all_packages,
        limit,
        chrono::Utc::now(),
        &ContributionMap::new(),
    );

    Ok(())
}

fn cmd_report(_config: &Config, format: &ReportFormat) -> Result<()> {
    let storage = Storage::open().context("Failed to open database")?;
    let scan = storage
        .latest_scan()
        .context("Failed to read latest scan")?;

    let scan = match scan {
        Some(s) => s,
        None => {
            eprintln!("No scan data found. Run `syld scan` first.");
            return Ok(());
        }
    };

    let contributions = ContributionMap::new();

    match format {
        ReportFormat::Terminal => {
            let mut packages = scan.packages;
            terminal::sort_packages(&mut packages);
            terminal::print_summary(&packages, 0, scan.timestamp, &contributions);
        }
        ReportFormat::Json => {
            json::print_json(&scan.packages, scan.timestamp, &contributions)?;
        }
        ReportFormat::Html => {
            html::print_html(&scan.packages, scan.timestamp, &contributions);
        }
    }

    Ok(())
}

fn cmd_budget(_config: &Config, _command: &BudgetCommands) -> Result<()> {
    eprintln!("Budget management not yet implemented.");
    Ok(())
}

fn cmd_config(config: &Config, command: &Option<ConfigCommands>) -> Result<()> {
    match command {
        None | Some(ConfigCommands::Show) => cmd_config_show(config),
        Some(ConfigCommands::Edit) => cmd_config_edit(),
    }
}

fn cmd_config_show(config: &Config) -> Result<()> {
    let path = Config::config_path()?;
    eprintln!("# {}", path.display());

    let toml = toml::to_string_pretty(config).context("Failed to serialize config")?;
    print!("{toml}");
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
