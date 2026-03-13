// SPDX-License-Identifier: GPL-3.0-or-later

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use syld::config::Config;
use syld::contribute::github_good_first_issues::{GhIssue, extract_github_owner_repo};
use syld::contribute::github_sync::is_gh_available;
use syld::contribute::suggest::{self, SuggestionKind};
use syld::contribute::{ContributionRecordKind, NewContribution};
use syld::discover::{self, InstalledPackage};
use syld::enrich::EnrichmentMap;
use syld::enrich::github::contributing_file_exists;
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
  3. Review         syld report
  4. Contribute     syld contribute"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Discover installed packages and enrich project metadata
    Scan {
        /// Force re-enrichment, bypassing the cache
        #[arg(long)]
        force_refresh: bool,

        /// Number of parallel enrichment threads
        #[arg(short = 'j', long)]
        jobs: Option<usize>,
    },

    /// Display a report from stored scan data
    Report {
        /// Output format
        #[arg(long, default_value = "terminal")]
        format: ReportFormat,

        /// Maximum number of projects to display (0 for all)
        #[arg(short = 'n', long, alias = "count", default_value = "0")]
        limit: usize,

        /// Interactively paginate through results when --limit is set
        #[arg(long)]
        paginate: bool,
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

    /// Suggest actionable ways to support open source projects you depend on
    #[command(args_conflicts_with_subcommands = true)]
    Contribute {
        /// Number of suggestions to show
        #[arg(short = 'n', long = "limit", alias = "count", default_value = "3")]
        limit: usize,

        /// Comma-separated contribution types to include (star, issue, donate, docs, spread)
        #[arg(long = "type", value_name = "TYPES")]
        types: Option<String>,

        #[command(subcommand)]
        command: Option<ContributeCommands>,
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

        /// Path to the syld database (overrides default resolution)
        #[arg(long)]
        db_path: Option<PathBuf>,
    },

    /// List all hooks with their availability status
    List,
}

#[derive(Subcommand)]
enum InstallCommands {
    /// Install systemd user service and timer for periodic scans
    Service {
        /// Timer frequency (daily, weekly, monthly)
        #[arg(long, default_value = "weekly", value_parser = ["daily", "weekly", "monthly"])]
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
enum ContributeCommands {
    /// Star a project on GitHub
    Star {
        /// Project URL or GitHub owner/repo (e.g. github.com/curl/curl or curl/curl)
        #[arg(long)]
        project: Option<String>,
    },

    /// List good first issues for a project on GitHub
    Issue {
        /// Project URL or GitHub owner/repo (e.g. github.com/curl/curl or curl/curl)
        #[arg(long)]
        project: Option<String>,
    },

    /// Open or print funding/donation links for a project
    Donate {
        /// Project URL or GitHub owner/repo (e.g. github.com/curl/curl or curl/curl)
        #[arg(long)]
        project: Option<String>,

        /// Amount donated (optional)
        #[arg(long)]
        amount: Option<f64>,

        /// Currency code (e.g. USD, EUR) (optional)
        #[arg(long)]
        currency: Option<String>,

        /// Funding channel/platform used (e.g. GitHub Sponsors, Patreon) (optional)
        #[arg(long)]
        via: Option<String>,
    },

    /// Open or print a project's contributing guide
    Docs {
        /// Project URL or GitHub owner/repo (e.g. github.com/curl/curl or curl/curl)
        #[arg(long)]
        project: Option<String>,
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
        /// Configuration key (e.g. enrich, enrich_jobs)
        key: String,

        /// Value to set
        value: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = Config::load()?;

    match cli.command {
        None => cmd_scan(&config, false, None),
        Some(Commands::Scan {
            force_refresh,
            jobs,
        }) => cmd_scan(&config, force_refresh, jobs),
        Some(Commands::Report {
            format,
            limit,
            paginate,
        }) => cmd_report(&config, &format, limit, paginate),
        Some(Commands::Cache { command }) => cmd_cache(&command),
        Some(Commands::Config { command }) => cmd_config(&config, &command),
        Some(Commands::Hook { command }) => cmd_hook(&config, &command),
        Some(Commands::Contribute {
            limit,
            types,
            command,
        }) => match command {
            None => cmd_contribute(&config, limit, types.as_deref()),
            Some(ContributeCommands::Star { project }) => cmd_contribute_star(project.as_deref()),
            Some(ContributeCommands::Issue { project }) => cmd_contribute_issue(project.as_deref()),
            Some(ContributeCommands::Donate {
                project,
                amount,
                currency,
                via,
            }) => cmd_contribute_donate(
                project.as_deref(),
                amount,
                currency.as_deref(),
                via.as_deref(),
            ),
            Some(ContributeCommands::Docs { project }) => cmd_contribute_docs(project.as_deref()),
        },
        Some(Commands::Setup) => cmd_setup(&config),
        Some(Commands::Install { command }) => cmd_install(&command),
    }
}

fn run_scan(config: &Config) -> Result<Vec<InstalledPackage>> {
    let discoverers = discover::active_discoverers(config);

    if discoverers.is_empty() {
        eprintln!("No supported package managers detected on this system.");
        return Ok(Vec::new());
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
            Ok(_) => {
                eprintln!("Scan saved ({} packages)", all_packages.len());
            }
            Err(e) => eprintln!("Warning: failed to save scan: {e}"),
        },
        Err(e) => eprintln!("Warning: failed to open database: {e}"),
    }

    Ok(all_packages)
}

fn cmd_scan(config: &Config, force_refresh: bool, jobs: Option<usize>) -> Result<()> {
    let packages = run_scan(config)?;

    if packages.is_empty() {
        return Ok(());
    }

    // Run enrichment
    let storage = Storage::open().context("Failed to open database")?;
    let should_enrich = config.enrich || force_refresh;
    if should_enrich {
        syld::enrich::enrich_packages(&packages, &storage, config, force_refresh, jobs)?;
        eprintln!("Enrichment complete.");
    }

    Ok(())
}

fn cmd_report(config: &Config, format: &ReportFormat, limit: usize, paginate: bool) -> Result<()> {
    let storage = Storage::open().context("Failed to open database")?;
    let scan = storage
        .latest_scan()
        .context("Failed to read latest scan")?;

    let scan = match scan {
        Some(s) => s,
        None => {
            eprintln!("No scan data found. Running scan first\u{2026}");
            // Run a full scan (with enrichment if configured)
            cmd_scan(config, false, None)?;
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

    // Load enrichment data from the database (no network calls)
    let enrichment = load_stored_enrichment(&storage, &scan.packages);

    let contributions = ContributionMap::new();

    // Build contribution summary from stored records
    let contribution_summary = {
        let records = storage
            .get_contributions(None, None)
            .context("Failed to load contributions")?;
        let s = ContributionSummary::from_records(&records);
        if s.is_empty() { None } else { Some(s) }
    };

    match format {
        ReportFormat::Terminal => {
            let mut packages = scan.packages;
            terminal::sort_packages(&mut packages);
            terminal::print_summary(
                &packages,
                limit,
                paginate,
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

/// Load enrichment data from the database cache without making network calls.
fn load_stored_enrichment(storage: &Storage, packages: &[InstalledPackage]) -> EnrichmentMap {
    let mut map = EnrichmentMap::new();
    for pkg in packages {
        let url = match &pkg.url {
            Some(u) if !u.is_empty() => u,
            _ => continue,
        };
        let normalized = terminal::normalize_url(url);
        if map.contains_key(&normalized) {
            continue;
        }
        if let Ok(Some(project)) = storage.get_enrichment(&normalized) {
            map.insert(normalized, project);
        }
    }
    map
}

fn cmd_contribute(config: &Config, limit: usize, types: Option<&str>) -> Result<()> {
    let filter: Vec<SuggestionKind> = match types {
        Some(input) => suggest::parse_types(input).map_err(|e| anyhow::anyhow!(e))?,
        None => SuggestionKind::ALL.to_vec(),
    };

    let storage = Storage::open().context("Failed to open database")?;
    let scan = storage
        .latest_scan()
        .context("Failed to read latest scan")?;

    if scan.is_none() {
        eprintln!("No scan data found. Run `syld scan` first.");
        return Ok(());
    }

    // Load enriched project data from the database.
    let projects = storage.all_projects().context("Failed to load projects")?;
    if projects.is_empty() {
        eprintln!(
            "No enriched project data found. Run `syld scan` with enrichment enabled to populate project data."
        );
        return Ok(());
    }

    // Load existing contributions to filter already-completed actions.
    let contributions = storage
        .get_contributions(None, None)
        .context("Failed to load contributions")?;

    // Generate suggestions from enrichment data.
    let suggestions = suggest::generate_suggestions(&projects, &contributions, &filter);

    if suggestions.is_empty() {
        eprintln!("No suggestions available for the selected types.");
        return Ok(());
    }

    let selected = suggest::pick_random(suggestions, limit);
    print!("{}", suggest::format_suggestions(&selected));

    let _ = config;
    Ok(())
}

/// Normalize a project identifier into a GitHub URL and owner/repo pair.
///
/// Accepts:
/// - `owner/repo` (e.g. `curl/curl`)
/// - `github.com/owner/repo` (without scheme)
/// - `https://github.com/owner/repo` (full URL)
/// - SSH and git:// URLs
///
/// Returns `(repo_url, owner_repo)` or an error if the input is not a valid
/// GitHub project identifier.
fn resolve_github_project(input: &str) -> Result<(String, String)> {
    // Try bare owner/repo first (no dots, no slashes beyond the one separator).
    if !input.contains("://") && !input.contains("github.com") && !input.starts_with("git@") {
        let parts: Vec<&str> = input.splitn(2, '/').collect();
        if parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty() {
            let owner_repo = format!("{}/{}", parts[0], parts[1]);
            let repo_url = format!("https://github.com/{owner_repo}");
            return Ok((repo_url, owner_repo));
        }
    }

    // Try with https:// prefix if no scheme present
    let url = if !input.contains("://") && !input.starts_with("git@") {
        format!("https://{input}")
    } else {
        input.to_string()
    };

    let owner_repo = extract_github_owner_repo(&url)
        .ok_or_else(|| anyhow::anyhow!("Not a valid GitHub project: {input}"))?;
    let repo_url = format!("https://github.com/{owner_repo}");
    Ok((repo_url, owner_repo))
}

fn cmd_contribute_star(project: Option<&str>) -> Result<()> {
    let (repo_url, owner_repo) = match project {
        Some(input) => resolve_github_project(input)?,
        None => {
            // Pick a random unstarred project from the database
            let storage = Storage::open().context("Failed to open database")?;
            let projects = storage.all_projects().context("Failed to load projects")?;
            let contributions = storage
                .get_contributions(None, None)
                .context("Failed to load contributions")?;

            let suggestions =
                suggest::generate_suggestions(&projects, &contributions, &[SuggestionKind::Star]);

            if suggestions.is_empty() {
                eprintln!(
                    "No unstarred GitHub projects found. Run `syld scan` to discover projects."
                );
                return Ok(());
            }

            let picked = suggest::pick_random(suggestions, 1);
            let suggestion = &picked[0];

            let owner_repo = extract_github_owner_repo(&suggestion.url)
                .ok_or_else(|| anyhow::anyhow!("Invalid GitHub URL in suggestion"))?;
            (suggestion.url.clone(), owner_repo)
        }
    };

    if is_gh_available() {
        // Star via gh API
        let output = Command::new("gh")
            .args([
                "api",
                &format!("/user/starred/{owner_repo}"),
                "-X",
                "PUT",
                "--silent",
            ])
            .output()
            .context("Failed to run gh api")?;

        if output.status.success() {
            eprintln!("\u{2b50} Starred {owner_repo} on GitHub");
            eprintln!("  {repo_url}");
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("422") || stderr.contains("already") {
                eprintln!("\u{2b50} {owner_repo} is already starred");
                eprintln!("  {repo_url}");
            } else {
                anyhow::bail!("Failed to star {owner_repo}: {stderr}");
            }
        }
    } else {
        // Fallback: print the URL
        eprintln!("\u{2b50} Star {owner_repo} on GitHub:");
        eprintln!("  {repo_url}");
        eprintln!(
            "\nTip: install and authenticate the `gh` CLI to star directly from the terminal."
        );
    }

    // Record the contribution in the database
    let storage = Storage::open().context("Failed to open database")?;
    if !storage
        .has_contribution(&repo_url, &ContributionRecordKind::Star)
        .context("Failed to check contribution status")?
    {
        storage.save_contribution(&NewContribution {
            project_url: &repo_url,
            kind: &ContributionRecordKind::Star,
            title: None,
            url: None,
            contributed_at: chrono::Utc::now(),
            source: Some("contribute_star"),
            amount: None,
            currency: None,
            via: None,
        })?;
    }

    Ok(())
}

fn cmd_contribute_issue(project: Option<&str>) -> Result<()> {
    let (repo_url, owner_repo) = match project {
        Some(input) => resolve_github_project(input)?,
        None => {
            // Pick a random project with good first issues from the database
            let storage = Storage::open().context("Failed to open database")?;
            let projects = storage.all_projects().context("Failed to load projects")?;
            let contributions = storage
                .get_contributions(None, None)
                .context("Failed to load contributions")?;

            let suggestions =
                suggest::generate_suggestions(&projects, &contributions, &[SuggestionKind::Issue]);

            if suggestions.is_empty() {
                eprintln!(
                    "No projects with good first issues found. Run `syld scan` to discover projects."
                );
                return Ok(());
            }

            let picked = suggest::pick_random(suggestions, 1);
            let suggestion = &picked[0];

            let owner_repo = extract_github_owner_repo(&suggestion.url)
                .ok_or_else(|| anyhow::anyhow!("Invalid GitHub URL in suggestion"))?;
            (suggestion.url.clone(), owner_repo)
        }
    };

    if is_gh_available() {
        // List good first issues via gh CLI
        let output = Command::new("gh")
            .args([
                "issue",
                "list",
                "--repo",
                &owner_repo,
                "--label",
                "good first issue",
                "--state",
                "open",
                "--limit",
                "10",
                "--json",
                "title,url,labels",
            ])
            .output()
            .context("Failed to run gh issue list")?;

        if output.status.success() {
            let stdout = String::from_utf8(output.stdout)
                .context("gh issue list output is not valid UTF-8")?;

            let issues: Vec<GhIssue> =
                serde_json::from_str(&stdout).context("Failed to parse gh issue list JSON")?;

            if issues.is_empty() {
                eprintln!("No good first issues found for {owner_repo}");
                eprintln!("  Browse all issues: https://github.com/{owner_repo}/issues");
            } else {
                eprintln!(
                    "Good first issues for {owner_repo} ({} found):\n",
                    issues.len()
                );
                for (i, issue) in issues.iter().enumerate() {
                    eprintln!("  {}. {}", i + 1, issue.title);
                    eprintln!("     {}", issue.url);
                }
            }
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("Could not resolve")
                || stderr.contains("not found")
                || stderr.contains("403")
            {
                eprintln!("Could not access issues for {owner_repo}");
                eprintln!(
                    "  Browse issues: https://github.com/{owner_repo}/issues?q=label:%22good+first+issue%22"
                );
            } else {
                anyhow::bail!("Failed to list issues for {owner_repo}: {stderr}");
            }
        }
    } else {
        // Fallback: print the URL
        eprintln!("Good first issues for {owner_repo}:");
        eprintln!("  https://github.com/{owner_repo}/issues?q=label:%22good+first+issue%22");
        eprintln!(
            "\nTip: install and authenticate the `gh` CLI to list issues directly from the terminal."
        );
    }

    // Record the contribution in the database
    let storage = Storage::open().context("Failed to open database")?;
    if !storage
        .has_contribution(&repo_url, &ContributionRecordKind::Issue)
        .context("Failed to check contribution status")?
    {
        storage.save_contribution(&NewContribution {
            project_url: &repo_url,
            kind: &ContributionRecordKind::Issue,
            title: None,
            url: None,
            contributed_at: chrono::Utc::now(),
            source: Some("contribute_issue"),
            amount: None,
            currency: None,
            via: None,
        })?;
    }

    Ok(())
}

fn cmd_contribute_donate(
    project: Option<&str>,
    amount: Option<f64>,
    currency: Option<&str>,
    via: Option<&str>,
) -> Result<()> {
    let storage = Storage::open().context("Failed to open database")?;

    let (project_url, funding) = match project {
        Some(input) => {
            // Resolve the project and look up its funding channels in the database
            let (repo_url, _owner_repo) = resolve_github_project(input)?;

            let projects = storage.all_projects().context("Failed to load projects")?;
            let found = projects.iter().find(|p| {
                p.repo_url.as_deref() == Some(&repo_url)
                    || p.homepage.as_deref() == Some(input)
                    || p.name.eq_ignore_ascii_case(input)
            });

            match found {
                Some(p) => (repo_url, p.funding.clone()),
                None => {
                    // Project not in database — try fetching funding info via gh CLI
                    let funding = fetch_github_funding(input);
                    (repo_url, funding)
                }
            }
        }
        None => {
            // Pick a random project with funding channels from the database
            let projects = storage.all_projects().context("Failed to load projects")?;
            let contributions = storage
                .get_contributions(None, None)
                .context("Failed to load contributions")?;

            let suggestions =
                suggest::generate_suggestions(&projects, &contributions, &[SuggestionKind::Donate]);

            if suggestions.is_empty() {
                eprintln!(
                    "No projects with funding channels found. Run `syld scan` to discover projects."
                );
                return Ok(());
            }

            let picked = suggest::pick_random(suggestions, 1);
            let suggestion = &picked[0];

            // Find the project in the database to get its funding channels
            let found = projects.iter().find(|p| {
                p.funding.iter().any(|f| f.url == suggestion.url)
                    || p.repo_url.as_deref() == Some(&suggestion.url)
            });

            let funding = match found {
                Some(p) => p.funding.clone(),
                None => vec![],
            };

            let project_url = found
                .and_then(|p| p.repo_url.clone().or(p.homepage.clone()))
                .unwrap_or_else(|| suggestion.url.clone());

            (project_url, funding)
        }
    };

    if funding.is_empty() {
        eprintln!("No funding channels found for this project.");
        eprintln!("The project may not have a FUNDING.yml or known donation platform.");
        return Ok(());
    }

    eprintln!("Funding channels:\n");
    for channel in &funding {
        eprintln!("  {} — {}", channel.platform, channel.url);
    }

    // Generate title string if amount and currency are provided
    let title = match (amount, currency) {
        (Some(amt), Some(curr)) => Some(match via {
            Some(channel) => format!("{} {} via {}", amt, curr, channel),
            None => format!("{} {}", amt, curr),
        }),
        _ => None,
    };

    // Record the contribution in the database
    if !storage
        .has_contribution(&project_url, &ContributionRecordKind::Donation)
        .context("Failed to check contribution status")?
    {
        storage.save_contribution(&NewContribution {
            project_url: &project_url,
            kind: &ContributionRecordKind::Donation,
            title: title.as_deref(),
            url: funding.first().map(|f| f.url.as_str()),
            contributed_at: chrono::Utc::now(),
            source: Some("contribute_donate"),
            amount,
            currency,
            via,
        })?;
    }

    Ok(())
}

fn cmd_contribute_docs(project: Option<&str>) -> Result<()> {
    let storage = Storage::open().context("Failed to open database")?;

    let (project_url, contributing_url) = match project {
        Some(input) => {
            let (repo_url, owner_repo) = resolve_github_project(input)?;

            // Look up the project in the database for a known contributing URL
            let projects = storage.all_projects().context("Failed to load projects")?;
            let found = projects.iter().find(|p| {
                p.repo_url.as_deref() == Some(&repo_url)
                    || p.homepage.as_deref() == Some(input)
                    || p.name.eq_ignore_ascii_case(input)
            });

            let url = found.and_then(|p| p.contributing_url.clone());

            let url = match url {
                Some(u) => u,
                None => {
                    // No known contributing URL — verify the file exists before
                    // sending the user to a potentially dead link.
                    if is_gh_available() && !contributing_file_exists(&owner_repo) {
                        eprintln!(
                            "{owner_repo} does not have a CONTRIBUTING.md yet — \
                             creating one would be a great first contribution!"
                        );
                        return Ok(());
                    }
                    format!("https://github.com/{owner_repo}/blob/HEAD/CONTRIBUTING.md")
                }
            };

            (repo_url, url)
        }
        None => {
            // Pick a random project with a contributing guide from the database
            let projects = storage.all_projects().context("Failed to load projects")?;
            let contributions = storage
                .get_contributions(None, None)
                .context("Failed to load contributions")?;

            let suggestions =
                suggest::generate_suggestions(&projects, &contributions, &[SuggestionKind::Docs]);

            if suggestions.is_empty() {
                eprintln!(
                    "No projects with contributing guides found. Run `syld scan` to discover projects."
                );
                return Ok(());
            }

            let picked = suggest::pick_random(suggestions, 1);
            let suggestion = &picked[0];

            let project_url = projects
                .iter()
                .find(|p| p.contributing_url.as_deref() == Some(&suggestion.url))
                .and_then(|p| p.repo_url.clone())
                .unwrap_or_else(|| suggestion.url.clone());

            (project_url, suggestion.url.clone())
        }
    };

    eprintln!("Contributing guide:");
    eprintln!("  {contributing_url}");

    // Record the contribution in the database
    if !storage
        .has_contribution(&project_url, &ContributionRecordKind::Docs)
        .context("Failed to check contribution status")?
    {
        storage.save_contribution(&NewContribution {
            project_url: &project_url,
            kind: &ContributionRecordKind::Docs,
            title: None,
            url: Some(&contributing_url),
            contributed_at: chrono::Utc::now(),
            source: Some("contribute_docs"),
            amount: None,
            currency: None,
            via: None,
        })?;
    }

    Ok(())
}

/// Try to fetch funding information from a GitHub repository's FUNDING.yml.
fn fetch_github_funding(input: &str) -> Vec<syld::project::FundingChannel> {
    let Ok((_repo_url, owner_repo)) = resolve_github_project(input) else {
        return vec![];
    };

    if !is_gh_available() {
        return vec![];
    }

    // Try to read .github/FUNDING.yml via gh api
    let output = Command::new("gh")
        .args([
            "api",
            &format!("repos/{owner_repo}/contents/.github/FUNDING.yml"),
            "--jq",
            ".content",
        ])
        .output();

    let Ok(output) = output else {
        return vec![];
    };

    if !output.status.success() {
        return vec![];
    }

    let encoded = String::from_utf8_lossy(&output.stdout);
    let encoded = encoded.trim();
    if encoded.is_empty() {
        return vec![];
    }

    // Decode base64 content
    let decoded_bytes: Vec<u8> = encoded
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .as_bytes()
        .chunks(4)
        .filter_map(|chunk| {
            let s = std::str::from_utf8(chunk).ok()?;
            base64_decode_chunk(s)
        })
        .flatten()
        .collect();

    let content = String::from_utf8_lossy(&decoded_bytes);
    parse_funding_yml(&content)
}

/// Parse a FUNDING.yml file into funding channels.
fn parse_funding_yml(content: &str) -> Vec<syld::project::FundingChannel> {
    let mut channels = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim().to_lowercase();
        let value = value.trim().trim_matches(|c| c == '\'' || c == '"');
        if value.is_empty() {
            continue;
        }

        let (platform, url) = match key.as_str() {
            "github" => (
                "GitHub Sponsors",
                format!("https://github.com/sponsors/{value}"),
            ),
            "open_collective" => (
                "Open Collective",
                format!("https://opencollective.com/{value}"),
            ),
            "ko_fi" => ("Ko-fi", format!("https://ko-fi.com/{value}")),
            "liberapay" => ("Liberapay", format!("https://liberapay.com/{value}")),
            "patreon" => ("Patreon", format!("https://patreon.com/{value}")),
            "custom" => {
                // custom can be a URL directly
                if value.starts_with("http") {
                    ("Custom", value.to_string())
                } else {
                    continue;
                }
            }
            _ => continue,
        };

        channels.push(syld::project::FundingChannel {
            platform: platform.to_string(),
            url,
        });
    }

    channels
}

fn base64_decode_chunk(chunk: &str) -> Option<Vec<u8>> {
    let bytes: Vec<u8> = chunk
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => 0xFF,
        })
        .collect();

    if bytes.len() < 2 {
        return None;
    }

    let mut result = Vec::new();
    if bytes.len() >= 2 && bytes[0] != 0xFF && bytes[1] != 0xFF {
        result.push((bytes[0] << 2) | (bytes[1] >> 4));
    }
    if bytes.len() >= 3 && bytes[2] != 0xFF && chunk.as_bytes().get(2) != Some(&b'=') {
        result.push((bytes[1] << 4) | (bytes[2] >> 2));
    }
    if bytes.len() >= 4 && bytes[3] != 0xFF && chunk.as_bytes().get(3) != Some(&b'=') {
        result.push((bytes[2] << 6) | bytes[3]);
    }

    Some(result)
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
        HookCommands::Run { name, db_path } => cmd_hook_run(config, name, db_path.as_deref()),
        HookCommands::List => cmd_hook_list(),
    }
}

fn cmd_hook_run(config: &Config, name: &str, db_path: Option<&std::path::Path>) -> Result<()> {
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

    let ctx = HookContext {
        config,
        targets,
        db_path: db_path.map(|p| p.to_path_buf()),
    };

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
        eprintln!("  {:<30} {} [{}]", h.name(), h.description(), status);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_bare_owner_repo() {
        let (url, owner_repo) = resolve_github_project("curl/curl").unwrap();
        assert_eq!(url, "https://github.com/curl/curl");
        assert_eq!(owner_repo, "curl/curl");
    }

    #[test]
    fn resolve_github_url_without_scheme() {
        let (url, owner_repo) = resolve_github_project("github.com/curl/curl").unwrap();
        assert_eq!(url, "https://github.com/curl/curl");
        assert_eq!(owner_repo, "curl/curl");
    }

    #[test]
    fn resolve_full_https_url() {
        let (url, owner_repo) = resolve_github_project("https://github.com/curl/curl").unwrap();
        assert_eq!(url, "https://github.com/curl/curl");
        assert_eq!(owner_repo, "curl/curl");
    }

    #[test]
    fn resolve_url_with_trailing_slash() {
        let (_, owner_repo) = resolve_github_project("https://github.com/curl/curl/").unwrap();
        assert_eq!(owner_repo, "curl/curl");
    }

    #[test]
    fn resolve_url_with_git_suffix() {
        let (_, owner_repo) = resolve_github_project("https://github.com/curl/curl.git").unwrap();
        assert_eq!(owner_repo, "curl/curl");
    }

    #[test]
    fn resolve_ssh_url() {
        let (url, owner_repo) = resolve_github_project("git@github.com:curl/curl.git").unwrap();
        assert_eq!(url, "https://github.com/curl/curl");
        assert_eq!(owner_repo, "curl/curl");
    }

    #[test]
    fn resolve_invalid_input() {
        assert!(resolve_github_project("not-a-project").is_err());
    }

    #[test]
    fn resolve_non_github_url() {
        assert!(resolve_github_project("https://gitlab.com/owner/repo").is_err());
    }

    #[test]
    fn parse_funding_yml_github_sponsors() {
        let content = "github: curl\n";
        let channels = parse_funding_yml(content);
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].platform, "GitHub Sponsors");
        assert_eq!(channels[0].url, "https://github.com/sponsors/curl");
    }

    #[test]
    fn parse_funding_yml_multiple() {
        let content = "\
github: user1
open_collective: myproject
ko_fi: creator
liberapay: dev
patreon: artist
";
        let channels = parse_funding_yml(content);
        assert_eq!(channels.len(), 5);
        assert_eq!(channels[0].platform, "GitHub Sponsors");
        assert_eq!(channels[1].platform, "Open Collective");
        assert_eq!(channels[1].url, "https://opencollective.com/myproject");
        assert_eq!(channels[2].platform, "Ko-fi");
        assert_eq!(channels[2].url, "https://ko-fi.com/creator");
        assert_eq!(channels[3].platform, "Liberapay");
        assert_eq!(channels[3].url, "https://liberapay.com/dev");
        assert_eq!(channels[4].platform, "Patreon");
        assert_eq!(channels[4].url, "https://patreon.com/artist");
    }

    #[test]
    fn parse_funding_yml_custom_url() {
        let content = "custom: https://example.com/donate\n";
        let channels = parse_funding_yml(content);
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].platform, "Custom");
        assert_eq!(channels[0].url, "https://example.com/donate");
    }

    #[test]
    fn parse_funding_yml_skips_comments_and_blanks() {
        let content = "\
# This is a comment
github: user1

# Another comment
";
        let channels = parse_funding_yml(content);
        assert_eq!(channels.len(), 1);
    }

    #[test]
    fn parse_funding_yml_empty() {
        let channels = parse_funding_yml("");
        assert!(channels.is_empty());
    }

    #[test]
    fn parse_funding_yml_skips_empty_values() {
        let content = "github:\nopen_collective: myproject\n";
        let channels = parse_funding_yml(content);
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].platform, "Open Collective");
    }

    #[test]
    fn base64_decode_hello() {
        // "Hello" in base64 is "SGVsbG8="
        let decoded: Vec<u8> = "SGVsbG8="
            .as_bytes()
            .chunks(4)
            .filter_map(|chunk| {
                let s = std::str::from_utf8(chunk).ok()?;
                base64_decode_chunk(s)
            })
            .flatten()
            .collect();
        assert_eq!(String::from_utf8(decoded).unwrap(), "Hello");
    }

    #[test]
    fn base64_decode_empty_input() {
        assert_eq!(base64_decode_chunk(""), None);
    }

    #[test]
    fn base64_decode_single_char() {
        assert_eq!(base64_decode_chunk("A"), None);
    }

    #[test]
    fn base64_decode_no_padding() {
        // "AQID" decodes to [1, 2, 3]
        assert_eq!(base64_decode_chunk("AQID"), Some(vec![1, 2, 3]));
    }

    #[test]
    fn base64_decode_single_padding() {
        // "AQI=" decodes to [1, 2]
        assert_eq!(base64_decode_chunk("AQI="), Some(vec![1, 2]));
    }

    #[test]
    fn base64_decode_double_padding() {
        // "AQ==" decodes to [1]
        assert_eq!(base64_decode_chunk("AQ=="), Some(vec![1]));
    }

    #[test]
    fn base64_decode_plus_and_slash() {
        // '+' maps to 62 (0b111110), '/' maps to 63 (0b111111)
        // "+/" as a 2-char chunk: byte0=62, byte1=63
        // result[0] = (62 << 2) | (63 >> 4) = 248 | 3 = 251
        assert_eq!(base64_decode_chunk("+/"), Some(vec![251]));
    }

    #[test]
    fn base64_decode_invalid_chars() {
        // '!' and '@' map to 0xFF and are skipped in output
        // Two valid chars needed for any output; "!@" has none valid
        // But length >= 2, so it enters the logic — bytes[0]=0xFF, bytes[1]=0xFF
        // First byte condition fails, so result is empty
        assert_eq!(base64_decode_chunk("!@"), Some(vec![]));
    }
}
