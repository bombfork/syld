// SPDX-License-Identifier: GPL-3.0-or-later

use std::env;
use std::fs;
use std::process::Command;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use syld::budget::{self, FundableGroup};
use syld::config::{BudgetConfig, Cadence, Config};
use syld::discover::{self, InstalledPackage};
use syld::enrich::EnrichmentMap;
use syld::hook::{self, HookContext};
use syld::install;
use syld::project::FundingChannel;
use syld::report::{ContributionMap, html, json, lookup_enrichment, terminal};
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
  3. Review         syld report
  4. Budget         syld budget set 10 && syld budget plan"
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
    },

    /// Manage the local cache
    Cache {
        #[command(subcommand)]
        command: CacheCommands,
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

    /// Manage package manager hooks
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
        }) => cmd_report(&config, &format, force_refresh, jobs),
        Some(Commands::Cache { command }) => cmd_cache(&command),
        Some(Commands::Budget { command }) => cmd_budget(&config, &command),
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
        );
    }

    Ok(())
}

fn cmd_report(
    config: &Config,
    format: &ReportFormat,
    force_refresh: bool,
    jobs: Option<usize>,
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
    let contributions = ContributionMap::new();

    match format {
        ReportFormat::Terminal => {
            let mut packages = scan.packages;
            terminal::sort_packages(&mut packages);
            terminal::print_summary(&packages, 0, scan.timestamp, &contributions, &enrichment);
        }
        ReportFormat::Json => {
            json::print_json(&scan.packages, scan.timestamp, &contributions, &enrichment)?;
        }
        ReportFormat::Html => {
            html::print_html(&scan.packages, scan.timestamp, &contributions, &enrichment);
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

fn cmd_budget(config: &Config, command: &BudgetCommands) -> Result<()> {
    match command {
        BudgetCommands::Set { amount, cadence } => cmd_budget_set(config, *amount, cadence),
        BudgetCommands::Show => cmd_budget_show(),
        BudgetCommands::Plan { strategy } => cmd_budget_plan(config, strategy),
    }
}

fn cmd_budget_set(config: &Config, amount: f64, cadence: &BudgetCadence) -> Result<()> {
    let storage = Storage::open().context("Failed to open database")?;
    let budget = BudgetConfig {
        amount: Some(amount),
        currency: config.budget.currency.clone(),
        cadence: match cadence {
            BudgetCadence::Monthly => Cadence::Monthly,
            BudgetCadence::Yearly => Cadence::Yearly,
        },
    };
    storage.save_budget(&budget)?;
    let cadence_label = match cadence {
        BudgetCadence::Monthly => "monthly",
        BudgetCadence::Yearly => "yearly",
    };
    eprintln!(
        "Budget set: {} {} {}",
        budget.currency, amount, cadence_label
    );
    Ok(())
}

fn cmd_budget_show() -> Result<()> {
    let storage = Storage::open().context("Failed to open database")?;
    match storage.get_budget()? {
        None => {
            eprintln!("No budget configured. Use `syld budget set <amount>` to set one.");
        }
        Some(budget) => {
            let cadence_label = match budget.cadence {
                Cadence::Monthly => "monthly",
                Cadence::Yearly => "yearly",
            };
            match budget.amount {
                Some(amount) => {
                    println!("{} {:.2} {}", budget.currency, amount, cadence_label);
                }
                None => {
                    println!(
                        "Budget configured but no amount set. Use `syld budget set <amount>`."
                    );
                }
            }
        }
    }
    Ok(())
}

fn cmd_budget_plan(config: &Config, strategy: &AllocationStrategy) -> Result<()> {
    let storage = Storage::open().context("Failed to open database")?;

    // Load budget
    let budget = match storage.get_budget()? {
        Some(b) if b.amount.is_some() => b,
        _ => {
            eprintln!("No budget configured. Use `syld budget set <amount>` first.");
            return Ok(());
        }
    };

    // Load latest scan; auto-scan if none
    let scan = match storage.latest_scan()? {
        Some(s) => s,
        None => {
            eprintln!("No previous scan found. Running scan first\u{2026}");
            run_scan(config, true)?;
            match storage.latest_scan()? {
                Some(s) => s,
                None => {
                    eprintln!("Scan completed but no data was saved.");
                    return Ok(());
                }
            }
        }
    };

    // Try loading cached enrichment data for each package
    let mut enrichment = EnrichmentMap::new();
    for pkg in &scan.packages {
        if let Some(url) = &pkg.url {
            let normalized = terminal::normalize_url(url);
            if !normalized.is_empty()
                && let Ok(Some(proj)) = storage.get_enrichment(url)
            {
                enrichment.insert(normalized, proj);
            }
        }
    }

    // If no enrichment data found, auto-trigger enrichment
    if enrichment.is_empty() {
        eprintln!("No enrichment data found. Running enriched scan\u{2026}");
        enrichment = syld::enrich::enrich_packages(&scan.packages, &storage, config, false, None)?;
    }

    // Group packages by org/ancestor
    let groups = terminal::group_by_project(&scan.packages);

    // Build fundable groups from those with funding channels
    let mut fundable_groups: Vec<FundableGroup> = Vec::new();
    for group in &groups {
        if group.url.is_empty() {
            continue;
        }

        let enriched = lookup_enrichment(&group.url, &group.project_urls, &enrichment);
        let funding: Vec<FundingChannel> = enriched.map(|e| e.funding.clone()).unwrap_or_default();

        if funding.is_empty() {
            continue;
        }

        let total_stars: u64 = enriched.and_then(|e| e.stars).unwrap_or(0);

        // Collect all enriched projects in this group
        let mut projects = Vec::new();
        if let Some(proj) = enriched {
            projects.push(proj.clone());
        }
        // Also collect any per-child-url enrichments for ancestor groups
        for child_url in &group.project_urls {
            if let Some(proj) = enrichment.get(child_url.as_str())
                && !projects
                    .iter()
                    .any(|p: &syld::project::UpstreamProject| p.repo_url == proj.repo_url)
            {
                projects.push(proj.clone());
            }
        }

        let label = if group.project_urls.is_empty() {
            group.url.clone()
        } else {
            format!("{}/*", group.url)
        };

        fundable_groups.push(FundableGroup {
            label,
            projects,
            funding,
            total_stars,
        });
    }

    if fundable_groups.is_empty() {
        eprintln!("No projects with funding channels found.");
        eprintln!("Try running `syld report --enrich` to discover funding links.");
        return Ok(());
    }

    let strat = match strategy {
        AllocationStrategy::Equal => budget::AllocationStrategy::Equal,
        AllocationStrategy::Weighted => budget::AllocationStrategy::Weighted,
    };

    let plan = budget::generate_plan(&budget, fundable_groups, strat);

    // Display plan as a table
    let cadence_label = match budget.cadence {
        Cadence::Monthly => "monthly",
        Cadence::Yearly => "yearly",
    };

    println!();
    println!(
        "Donation plan: {} {:.2} {}",
        budget.currency,
        budget.amount.unwrap_or(0.0),
        cadence_label
    );

    let strategy_label = match strategy {
        AllocationStrategy::Equal => "equal",
        AllocationStrategy::Weighted => "weighted",
    };
    println!("Strategy: {strategy_label}");
    println!();

    let mut table = comfy_table::Table::new();
    table.set_content_arrangement(comfy_table::ContentArrangement::Dynamic);
    table.set_header(vec!["Project", "Amount", "Frequency", "Via"]);

    for alloc in &plan.allocations {
        let freq = if alloc.every_n_months == 1 {
            "monthly".to_string()
        } else {
            format!("every {} months", alloc.every_n_months)
        };
        let via = alloc.via.as_deref().unwrap_or("-");
        table.add_row(vec![
            &alloc.project.name,
            &format!("{} {:.2}", budget.currency, alloc.amount),
            &freq,
            via,
        ]);
    }

    println!("{table}");

    Ok(())
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
        "budget.amount" => {
            config.budget.amount = Some(
                value
                    .parse::<f64>()
                    .with_context(|| format!("Invalid number '{value}'."))?,
            );
        }
        "budget.currency" => {
            config.budget.currency = value.to_uppercase();
        }
        "budget.cadence" => {
            config.budget.cadence = match value.to_lowercase().as_str() {
                "monthly" => Cadence::Monthly,
                "yearly" => Cadence::Yearly,
                _ => anyhow::bail!("Invalid cadence '{value}'. Valid values: monthly, yearly."),
            };
        }
        _ => {
            anyhow::bail!(
                "Unknown config key '{key}'. Valid keys: enrich, enrich_jobs, budget.amount, budget.currency, budget.cadence"
            );
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
