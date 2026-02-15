// SPDX-License-Identifier: GPL-3.0-or-later

//! GitHub good-first-issues contribution backend.
//!
//! Discovers beginner-friendly issues from GitHub repositories that the user
//! depends on. Uses the `gh` CLI to query the GitHub API, which handles
//! authentication transparently.

use std::process::Command;

use anyhow::{Context, Result};
use serde::Deserialize;

use super::{ContributionBackend, ContributionKind, ContributionOpportunity};
use crate::project::UpstreamProject;

/// Backend that discovers "good first issue" labeled issues from GitHub repos.
pub struct GitHubGoodFirstIssuesBackend;

/// A single issue from the `gh` CLI JSON output.
#[derive(Debug, Deserialize)]
struct GhIssue {
    title: String,
    url: String,
    #[serde(default)]
    labels: Vec<GhLabel>,
}

#[derive(Debug, Deserialize)]
struct GhLabel {
    name: String,
}

impl ContributionBackend for GitHubGoodFirstIssuesBackend {
    fn name(&self) -> &str {
        "github_good_first_issues"
    }

    fn is_available(&self) -> bool {
        // Check that gh CLI is installed and authenticated.
        Command::new("gh")
            .args(["auth", "status"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn find_opportunities(
        &self,
        project: &UpstreamProject,
    ) -> Result<Vec<ContributionOpportunity>> {
        let repo_url = match &project.repo_url {
            Some(url) => url,
            None => return Ok(Vec::new()),
        };

        let owner_repo = match extract_github_owner_repo(repo_url) {
            Some(or) => or,
            None => return Ok(Vec::new()),
        };

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

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Some repos may have issues disabled or be inaccessible — not fatal.
            if stderr.contains("Could not resolve")
                || stderr.contains("not found")
                || stderr.contains("403")
            {
                return Ok(Vec::new());
            }
            anyhow::bail!("gh issue list failed for {owner_repo}: {stderr}");
        }

        let stdout =
            String::from_utf8(output.stdout).context("gh issue list output is not valid UTF-8")?;

        let issues: Vec<GhIssue> =
            serde_json::from_str(&stdout).context("Failed to parse gh issue list JSON")?;

        let opportunities = issues
            .into_iter()
            .map(|issue| ContributionOpportunity {
                kind: ContributionKind::GoodFirstIssue,
                title: issue.title,
                description: if issue.labels.is_empty() {
                    None
                } else {
                    Some(
                        issue
                            .labels
                            .iter()
                            .map(|l| l.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", "),
                    )
                },
                url: issue.url,
            })
            .collect();

        Ok(opportunities)
    }
}

/// Extract `owner/repo` from a GitHub URL.
///
/// Accepts HTTPS, SSH, and `git://` URL formats:
/// - `https://github.com/owner/repo`
/// - `https://github.com/owner/repo.git`
/// - `git@github.com:owner/repo.git`
/// - `git://github.com/owner/repo`
///
/// Returns `None` if the URL is not a recognized GitHub URL.
pub(crate) fn extract_github_owner_repo(url: &str) -> Option<String> {
    // SSH format: git@github.com:owner/repo.git
    if let Some(rest) = url.strip_prefix("git@github.com:") {
        let rest = rest.strip_suffix(".git").unwrap_or(rest);
        let parts: Vec<&str> = rest.splitn(2, '/').collect();
        if parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty() {
            return Some(format!("{}/{}", parts[0], parts[1]));
        }
        return None;
    }

    // HTTPS / git:// format
    let url = url.strip_prefix("https://").or_else(|| {
        url.strip_prefix("http://")
            .or_else(|| url.strip_prefix("git://"))
    })?;

    let url = url.strip_prefix("www.").unwrap_or(url);

    if !url.starts_with("github.com/") {
        return None;
    }

    let path = url.strip_prefix("github.com/")?;
    let path = path.strip_suffix(".git").unwrap_or(path);
    let path = path.trim_end_matches('/');

    let parts: Vec<&str> = path.splitn(3, '/').collect();
    if parts.len() >= 2 && !parts[0].is_empty() && !parts[1].is_empty() {
        Some(format!("{}/{}", parts[0], parts[1]))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_https_url() {
        assert_eq!(
            extract_github_owner_repo("https://github.com/torvalds/linux"),
            Some("torvalds/linux".to_string())
        );
    }

    #[test]
    fn extract_https_url_with_git_suffix() {
        assert_eq!(
            extract_github_owner_repo("https://github.com/torvalds/linux.git"),
            Some("torvalds/linux".to_string())
        );
    }

    #[test]
    fn extract_https_url_with_trailing_slash() {
        assert_eq!(
            extract_github_owner_repo("https://github.com/torvalds/linux/"),
            Some("torvalds/linux".to_string())
        );
    }

    #[test]
    fn extract_https_url_with_subpath() {
        // Should still extract owner/repo, ignoring extra path segments
        assert_eq!(
            extract_github_owner_repo("https://github.com/torvalds/linux/tree/master"),
            Some("torvalds/linux".to_string())
        );
    }

    #[test]
    fn extract_ssh_url() {
        assert_eq!(
            extract_github_owner_repo("git@github.com:torvalds/linux.git"),
            Some("torvalds/linux".to_string())
        );
    }

    #[test]
    fn extract_ssh_url_no_suffix() {
        assert_eq!(
            extract_github_owner_repo("git@github.com:torvalds/linux"),
            Some("torvalds/linux".to_string())
        );
    }

    #[test]
    fn extract_git_protocol() {
        assert_eq!(
            extract_github_owner_repo("git://github.com/torvalds/linux"),
            Some("torvalds/linux".to_string())
        );
    }

    #[test]
    fn extract_http_url() {
        assert_eq!(
            extract_github_owner_repo("http://github.com/torvalds/linux"),
            Some("torvalds/linux".to_string())
        );
    }

    #[test]
    fn extract_www_url() {
        assert_eq!(
            extract_github_owner_repo("https://www.github.com/torvalds/linux"),
            Some("torvalds/linux".to_string())
        );
    }

    #[test]
    fn non_github_url_returns_none() {
        assert_eq!(
            extract_github_owner_repo("https://gitlab.com/owner/repo"),
            None
        );
    }

    #[test]
    fn incomplete_github_url_returns_none() {
        assert_eq!(
            extract_github_owner_repo("https://github.com/torvalds"),
            None
        );
    }

    #[test]
    fn empty_url_returns_none() {
        assert_eq!(extract_github_owner_repo(""), None);
    }

    #[test]
    fn non_url_returns_none() {
        assert_eq!(extract_github_owner_repo("not a url"), None);
    }

    #[test]
    fn find_opportunities_skips_non_github_projects() {
        let backend = GitHubGoodFirstIssuesBackend;
        let project = UpstreamProject {
            name: "test".to_string(),
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
        };

        let result = backend.find_opportunities(&project).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn find_opportunities_skips_projects_without_repo_url() {
        let backend = GitHubGoodFirstIssuesBackend;
        let project = UpstreamProject {
            name: "test".to_string(),
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
        };

        let result = backend.find_opportunities(&project).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn parse_gh_issue_json() {
        let json = r#"[
            {
                "title": "Fix typo in README",
                "url": "https://github.com/example/repo/issues/1",
                "labels": [
                    {"name": "good first issue"},
                    {"name": "documentation"}
                ]
            },
            {
                "title": "Add missing test",
                "url": "https://github.com/example/repo/issues/2",
                "labels": [
                    {"name": "good first issue"}
                ]
            }
        ]"#;

        let issues: Vec<GhIssue> = serde_json::from_str(json).unwrap();
        assert_eq!(issues.len(), 2);
        assert_eq!(issues[0].title, "Fix typo in README");
        assert_eq!(issues[0].labels.len(), 2);
        assert_eq!(issues[1].labels.len(), 1);
    }

    #[test]
    fn parse_gh_issue_json_empty() {
        let json = "[]";
        let issues: Vec<GhIssue> = serde_json::from_str(json).unwrap();
        assert!(issues.is_empty());
    }

    #[test]
    fn parse_gh_issue_json_no_labels() {
        let json = r#"[{
            "title": "Test issue",
            "url": "https://github.com/example/repo/issues/3"
        }]"#;

        let issues: Vec<GhIssue> = serde_json::from_str(json).unwrap();
        assert_eq!(issues.len(), 1);
        assert!(issues[0].labels.is_empty());
    }
}
