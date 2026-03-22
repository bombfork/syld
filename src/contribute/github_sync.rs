// SPDX-License-Identifier: GPL-3.0-or-later

//! GitHub contribution sync backend.
//!
//! Fetches the user's GitHub activity (stars, issues, PRs) for
//! discovered projects and stores them in the contributions database.

use std::collections::HashSet;
use std::process::Command;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::Deserialize;

use super::{ContributionRecordKind, NewContribution};
use crate::contribute::github_beginner_issues::extract_github_owner_repo;
use crate::project::UpstreamProject;
use crate::storage::Storage;

/// Summary of what was synced.
pub struct SyncResult {
    /// Number of new star contributions recorded.
    pub stars: usize,
    /// Number of new issue contributions recorded.
    pub issues: usize,
    /// Number of new pull request contributions recorded.
    pub pull_requests: usize,
}

/// A GitHub search result item (issue or PR).
#[derive(Debug, Deserialize)]
struct SearchItem {
    html_url: String,
    title: String,
    created_at: String,
}

/// Response from the GitHub search/issues API.
#[derive(Debug, Deserialize)]
struct SearchResponse {
    items: Vec<SearchItem>,
}

/// A resolved GitHub project with its owner/repo string and canonical URL.
struct GitHubProject<'a> {
    /// The original project reference.
    project: &'a UpstreamProject,
    /// `owner/repo` string extracted from the repo URL.
    owner_repo: String,
    /// The canonical repo URL (from the project).
    repo_url: String,
}

/// Check whether the `gh` CLI is installed and authenticated.
pub fn is_gh_available() -> bool {
    Command::new("gh")
        .args(["auth", "status"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Extract GitHub projects from a slice of upstream projects.
///
/// Returns only projects that have a `repo_url` pointing to GitHub and
/// from which an `owner/repo` can be parsed.
fn extract_github_repos<'a>(projects: &'a [UpstreamProject]) -> Vec<GitHubProject<'a>> {
    projects
        .iter()
        .filter_map(|p| {
            let url = p.repo_url.as_deref()?;
            let owner_repo = extract_github_owner_repo(url)?;
            Some(GitHubProject {
                project: p,
                owner_repo,
                repo_url: url.to_string(),
            })
        })
        .collect()
}

/// Fetch all repos the authenticated user has starred.
///
/// Returns a set of `owner/repo` strings (lowercased for comparison).
fn fetch_starred_repos() -> Result<HashSet<String>> {
    eprintln!("  fetching starred repos...");
    let output = Command::new("gh")
        .args([
            "api",
            "user/starred",
            "--paginate",
            "--jq",
            ".[].full_name",
            "--cache",
            "1h",
        ])
        .output()
        .context("Failed to run gh api user/starred")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh api user/starred failed: {stderr}");
    }

    let stdout = String::from_utf8(output.stdout)
        .context("gh api user/starred output is not valid UTF-8")?;

    Ok(parse_starred_repos(&stdout))
}

/// Parse the newline-delimited `full_name` output from the starred repos API.
///
/// Returns a set of lowercased `owner/repo` strings.
fn parse_starred_repos(output: &str) -> HashSet<String> {
    output
        .lines()
        .map(|line| line.trim().to_lowercase())
        .filter(|line| !line.is_empty())
        .collect()
}

/// Fetch issues authored by the current user for a given repo.
fn fetch_user_issues(owner_repo: &str) -> Result<Vec<SearchItem>> {
    let query = format!("search/issues?q=author:@me+repo:{owner_repo}+type:issue");
    let output = Command::new("gh")
        .args(["api", &query, "--cache", "1h"])
        .output()
        .context("Failed to run gh api search/issues")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Rate limits or auth issues — warn and return empty.
        if stderr.contains("rate limit")
            || stderr.contains("403")
            || stderr.contains("401")
            || stderr.contains("422")
        {
            eprintln!("warning: skipping issues for {owner_repo}: {stderr}");
            return Ok(Vec::new());
        }
        anyhow::bail!("gh api search/issues failed for {owner_repo}: {stderr}");
    }

    let stdout =
        String::from_utf8(output.stdout).context("gh api search/issues output is not UTF-8")?;

    parse_search_response(&stdout)
}

/// Fetch pull requests authored by the current user for a given repo.
fn fetch_user_pull_requests(owner_repo: &str) -> Result<Vec<SearchItem>> {
    let query = format!("search/issues?q=author:@me+repo:{owner_repo}+type:pr");
    let output = Command::new("gh")
        .args(["api", &query, "--cache", "1h"])
        .output()
        .context("Failed to run gh api search/issues (PRs)")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("rate limit")
            || stderr.contains("403")
            || stderr.contains("401")
            || stderr.contains("422")
        {
            eprintln!("warning: skipping PRs for {owner_repo}: {stderr}");
            return Ok(Vec::new());
        }
        anyhow::bail!("gh api search/issues (PRs) failed for {owner_repo}: {stderr}");
    }

    let stdout =
        String::from_utf8(output.stdout).context("gh api search/issues (PRs) output not UTF-8")?;

    parse_search_response(&stdout)
}

/// Parse a GitHub search/issues JSON response into a list of items.
fn parse_search_response(json: &str) -> Result<Vec<SearchItem>> {
    let resp: SearchResponse =
        serde_json::from_str(json).context("Failed to parse search/issues JSON")?;
    Ok(resp.items)
}

/// Parse a `created_at` timestamp string from the GitHub API.
///
/// Falls back to `Utc::now()` if the string cannot be parsed.
fn parse_datetime(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

/// Sync GitHub contributions (stars, issues, PRs) for the given projects.
///
/// Fetches the authenticated user's activity from GitHub and records any
/// new contributions in the database. Already-synced contributions are
/// skipped via [`Storage::has_contribution`].
///
/// # Errors
///
/// Returns an error if the `gh` CLI is not available or if fetching starred
/// repos fails. Per-repo errors for issues/PRs are logged as warnings and
/// do not abort the sync.
pub fn sync_github_contributions(
    storage: &Storage,
    projects: &[UpstreamProject],
) -> Result<SyncResult> {
    let mut result = SyncResult {
        stars: 0,
        issues: 0,
        pull_requests: 0,
    };

    if !is_gh_available() {
        anyhow::bail!("gh CLI is not available or not authenticated");
    }

    let github_projects = extract_github_repos(projects);
    if github_projects.is_empty() {
        eprintln!("  no GitHub projects found among discovered projects");
        return Ok(result);
    }

    eprintln!(
        "  syncing contributions for {} GitHub projects...",
        github_projects.len()
    );

    // --- Stars ---
    match fetch_starred_repos() {
        Ok(starred) => {
            for gp in &github_projects {
                let key = gp.owner_repo.to_lowercase();
                if starred.contains(&key) {
                    let already_recorded = match storage
                        .has_contribution(&gp.repo_url, &ContributionRecordKind::Star)
                    {
                        Ok(v) => v,
                        Err(e) => {
                            eprintln!(
                                "warning: failed to check star contribution for {}: {e}",
                                gp.project.name
                            );
                            false
                        }
                    };
                    if already_recorded {
                        continue;
                    }
                    if let Err(e) = storage.save_contribution(&NewContribution {
                        project_url: &gp.repo_url,
                        kind: &ContributionRecordKind::Star,
                        title: None,
                        url: None,
                        contributed_at: Utc::now(),
                        source: Some("github_sync"),
                        amount: None,
                        currency: None,
                        via: None,
                    }) {
                        eprintln!("warning: failed to save star for {}: {e}", gp.project.name);
                        continue;
                    }
                    result.stars += 1;
                }
            }
        }
        Err(e) => {
            eprintln!("warning: failed to fetch starred repos: {e}");
        }
    }

    // --- Issues and PRs ---
    for gp in &github_projects {
        // Issues
        match fetch_user_issues(&gp.owner_repo) {
            Ok(issues) => {
                for item in &issues {
                    // Dedup by URL since a project can have multiple issues
                    let already_recorded = match storage.has_contribution_url(&item.html_url) {
                        Ok(v) => v,
                        Err(e) => {
                            eprintln!(
                                "warning: failed to check contribution URL {}: {e}",
                                item.html_url
                            );
                            false
                        }
                    };
                    if already_recorded {
                        continue;
                    }
                    let dt = parse_datetime(&item.created_at);
                    if let Err(e) = storage.save_contribution(&NewContribution {
                        project_url: &gp.repo_url,
                        kind: &ContributionRecordKind::Issue,
                        title: Some(&item.title),
                        url: Some(&item.html_url),
                        contributed_at: dt,
                        source: Some("github_sync"),
                        amount: None,
                        currency: None,
                        via: None,
                    }) {
                        eprintln!("warning: failed to save issue for {}: {e}", gp.project.name);
                    } else {
                        result.issues += 1;
                    }
                }
            }
            Err(e) => {
                eprintln!("warning: failed to fetch issues for {}: {e}", gp.owner_repo);
            }
        }

        // Pull requests
        match fetch_user_pull_requests(&gp.owner_repo) {
            Ok(prs) => {
                for item in &prs {
                    // Dedup by URL since a project can have multiple PRs
                    let already_recorded = match storage.has_contribution_url(&item.html_url) {
                        Ok(v) => v,
                        Err(e) => {
                            eprintln!(
                                "warning: failed to check contribution URL {}: {e}",
                                item.html_url
                            );
                            false
                        }
                    };
                    if already_recorded {
                        continue;
                    }
                    let dt = parse_datetime(&item.created_at);
                    if let Err(e) = storage.save_contribution(&NewContribution {
                        project_url: &gp.repo_url,
                        kind: &ContributionRecordKind::PullRequest,
                        title: Some(&item.title),
                        url: Some(&item.html_url),
                        contributed_at: dt,
                        source: Some("github_sync"),
                        amount: None,
                        currency: None,
                        via: None,
                    }) {
                        eprintln!("warning: failed to save PR for {}: {e}", gp.project.name);
                    } else {
                        result.pull_requests += 1;
                    }
                }
            }
            Err(e) => {
                eprintln!("warning: failed to fetch PRs for {}: {e}", gp.owner_repo);
            }
        }
    }

    eprintln!(
        "  sync complete: {} stars, {} issues, {} PRs",
        result.stars, result.issues, result.pull_requests
    );

    Ok(result)
}

#[cfg(test)]
mod tests {
    use chrono::Datelike;

    use super::*;

    #[test]
    fn parse_starred_repos_basic() {
        let output = "torvalds/linux\nrust-lang/rust\nbombfork/syld\n";
        let repos = parse_starred_repos(output);
        assert_eq!(repos.len(), 3);
        assert!(repos.contains("torvalds/linux"));
        assert!(repos.contains("rust-lang/rust"));
        assert!(repos.contains("bombfork/syld"));
    }

    #[test]
    fn parse_starred_repos_empty() {
        let repos = parse_starred_repos("");
        assert!(repos.is_empty());
    }

    #[test]
    fn parse_starred_repos_lowercases() {
        let output = "TorVaLds/Linux\n";
        let repos = parse_starred_repos(output);
        assert!(repos.contains("torvalds/linux"));
        assert!(!repos.contains("TorVaLds/Linux"));
    }

    #[test]
    fn parse_starred_repos_skips_blank_lines() {
        let output = "owner/repo\n\n  \nanother/repo\n";
        let repos = parse_starred_repos(output);
        assert_eq!(repos.len(), 2);
    }

    #[test]
    fn parse_search_response_with_items() {
        let json = r#"{
            "total_count": 2,
            "incomplete_results": false,
            "items": [
                {
                    "html_url": "https://github.com/owner/repo/issues/1",
                    "title": "Fix a bug",
                    "created_at": "2025-01-15T10:30:00Z"
                },
                {
                    "html_url": "https://github.com/owner/repo/issues/2",
                    "title": "Add feature",
                    "created_at": "2025-02-20T14:00:00Z"
                }
            ]
        }"#;

        let items = parse_search_response(json).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].title, "Fix a bug");
        assert_eq!(items[0].html_url, "https://github.com/owner/repo/issues/1");
        assert_eq!(items[0].created_at, "2025-01-15T10:30:00Z");
        assert_eq!(items[1].title, "Add feature");
    }

    #[test]
    fn parse_search_response_empty() {
        let json = r#"{"total_count": 0, "incomplete_results": false, "items": []}"#;
        let items = parse_search_response(json).unwrap();
        assert!(items.is_empty());
    }

    #[test]
    fn parse_search_response_invalid_json() {
        let result = parse_search_response("not json");
        assert!(result.is_err());
    }

    #[test]
    fn extract_github_repos_filters_correctly() {
        let projects = vec![
            UpstreamProject {
                name: "linux".to_string(),
                repo_url: Some("https://github.com/torvalds/linux".to_string()),
                homepage: None,
                licenses: vec![],
                funding: vec![],
                bug_tracker: None,
                contributing_url: None,
                is_open_source: None,
                documentation_url: None,
                good_first_issues_url: None,
                stars: None,
                description: None,
            },
            UpstreamProject {
                name: "gitlab-project".to_string(),
                repo_url: Some("https://gitlab.com/owner/repo".to_string()),
                homepage: None,
                licenses: vec![],
                funding: vec![],
                bug_tracker: None,
                contributing_url: None,
                is_open_source: None,
                documentation_url: None,
                good_first_issues_url: None,
                stars: None,
                description: None,
            },
            UpstreamProject {
                name: "no-repo".to_string(),
                repo_url: None,
                homepage: None,
                licenses: vec![],
                funding: vec![],
                bug_tracker: None,
                contributing_url: None,
                is_open_source: None,
                documentation_url: None,
                good_first_issues_url: None,
                stars: None,
                description: None,
            },
            UpstreamProject {
                name: "rust".to_string(),
                repo_url: Some("https://github.com/rust-lang/rust".to_string()),
                homepage: None,
                licenses: vec![],
                funding: vec![],
                bug_tracker: None,
                contributing_url: None,
                is_open_source: None,
                documentation_url: None,
                good_first_issues_url: None,
                stars: None,
                description: None,
            },
        ];

        let github = extract_github_repos(&projects);
        assert_eq!(github.len(), 2);
        assert_eq!(github[0].owner_repo, "torvalds/linux");
        assert_eq!(github[1].owner_repo, "rust-lang/rust");
    }

    #[test]
    fn extract_github_repos_empty() {
        let projects: Vec<UpstreamProject> = vec![];
        let github = extract_github_repos(&projects);
        assert!(github.is_empty());
    }

    #[test]
    fn sync_result_construction() {
        let result = SyncResult {
            stars: 5,
            issues: 3,
            pull_requests: 2,
        };
        assert_eq!(result.stars, 5);
        assert_eq!(result.issues, 3);
        assert_eq!(result.pull_requests, 2);
    }

    #[test]
    fn parse_datetime_valid_rfc3339() {
        let dt = parse_datetime("2025-06-15T12:30:00Z");
        assert_eq!(dt.year(), 2025);
        assert_eq!(dt.month(), 6);
        assert_eq!(dt.day(), 15);
    }

    #[test]
    fn parse_datetime_invalid_falls_back() {
        // Should not panic, just returns now().
        let dt = parse_datetime("not-a-date");
        // Just verify it's a reasonable recent date.
        assert!(dt.year() >= 2025);
    }
}
