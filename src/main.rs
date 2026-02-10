// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::Result;
use clap::{Parser, Subcommand};

use syld::config::Config;
use syld::discover;
use syld::report::terminal;

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
    Scan,

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
        None | Some(Commands::Scan) => cmd_scan(&config),
        Some(Commands::Report { format, enrich: _ }) => cmd_report(&config, &format),
        Some(Commands::Budget { command }) => cmd_budget(&config, &command),
        Some(Commands::Config { command }) => cmd_config(&config, &command),
    }
}

fn cmd_scan(config: &Config) -> Result<()> {
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
    terminal::print_summary(&all_packages);

    Ok(())
}

fn cmd_report(_config: &Config, _format: &ReportFormat) -> Result<()> {
    eprintln!("Report generation not yet implemented.");
    Ok(())
}

fn cmd_budget(_config: &Config, _command: &BudgetCommands) -> Result<()> {
    eprintln!("Budget management not yet implemented.");
    Ok(())
}

fn cmd_config(_config: &Config, _command: &Option<ConfigCommands>) -> Result<()> {
    eprintln!("Configuration management not yet implemented.");
    Ok(())
}
