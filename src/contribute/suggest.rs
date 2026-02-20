// SPDX-License-Identifier: GPL-3.0-or-later

//! Contribution suggestion engine for the `syld contribute` command.
//!
//! This module defines the types used to generate actionable suggestions
//! from scan and enrichment data.

use std::collections::HashSet;
use std::fmt;
use std::str::FromStr;

use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};

use super::{ContributionRecord, ContributionRecordKind};
use crate::project::UpstreamProject;

/// The category of a contribution suggestion.
///
/// These map to the `--type` flag values on the `contribute` command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SuggestionKind {
    /// Star the project on GitHub.
    Star,
    /// Check good first issues.
    Issue,
    /// Donate via a funding channel.
    Donate,
    /// Improve project documentation.
    Docs,
    /// Share a lesser-known project.
    Spread,
}

impl SuggestionKind {
    /// All known suggestion kinds.
    pub const ALL: &[SuggestionKind] = &[
        SuggestionKind::Star,
        SuggestionKind::Issue,
        SuggestionKind::Donate,
        SuggestionKind::Docs,
        SuggestionKind::Spread,
    ];

    /// The emoji used when displaying this suggestion kind.
    pub fn emoji(self) -> &'static str {
        match self {
            SuggestionKind::Star => "\u{2b50}",
            SuggestionKind::Issue => "\u{1f41b}",
            SuggestionKind::Donate => "\u{1f4b0}",
            SuggestionKind::Docs => "\u{1f4d6}",
            SuggestionKind::Spread => "\u{1f4e3}",
        }
    }
}

impl fmt::Display for SuggestionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SuggestionKind::Star => write!(f, "star"),
            SuggestionKind::Issue => write!(f, "issue"),
            SuggestionKind::Donate => write!(f, "donate"),
            SuggestionKind::Docs => write!(f, "docs"),
            SuggestionKind::Spread => write!(f, "spread"),
        }
    }
}

impl FromStr for SuggestionKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "star" => Ok(SuggestionKind::Star),
            "issue" => Ok(SuggestionKind::Issue),
            "donate" => Ok(SuggestionKind::Donate),
            "docs" => Ok(SuggestionKind::Docs),
            "spread" => Ok(SuggestionKind::Spread),
            _ => Err(format!(
                "unknown suggestion type '{s}'. Valid types: star, issue, donate, docs, spread"
            )),
        }
    }
}

/// A concrete, actionable contribution suggestion shown to the user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContributionSuggestion {
    /// What category of contribution this is.
    pub kind: SuggestionKind,

    /// Human-readable action description (e.g. "Star curl/curl on GitHub").
    pub title: String,

    /// URL the user can visit to act on this suggestion.
    pub url: String,
}

/// Parse a comma-separated list of suggestion types.
///
/// Returns an error message if any type is unrecognised.
pub fn parse_types(input: &str) -> Result<Vec<SuggestionKind>, String> {
    input.split(',').map(|s| s.trim().parse()).collect()
}

/// Star count threshold below which a project is considered "lesser-known"
/// for spread suggestions.
const SPREAD_STAR_THRESHOLD: u64 = 1000;

type Generator = fn(&UpstreamProject) -> Option<ContributionSuggestion>;

/// Generate all contribution suggestions from enriched project data, filtered
/// against already-completed contributions.
///
/// Each generator inspects the enrichment fields on [`UpstreamProject`] and
/// produces suggestions for a single [`SuggestionKind`]. Results from all
/// generators are collected into one flat list.
pub fn generate_suggestions(
    projects: &[UpstreamProject],
    contributions: &[ContributionRecord],
    filter: &[SuggestionKind],
) -> Vec<ContributionSuggestion> {
    let starred: HashSet<&str> = contributions
        .iter()
        .filter(|c| c.kind == ContributionRecordKind::Star)
        .map(|c| c.project_url.as_str())
        .collect();

    let donated: HashSet<&str> = contributions
        .iter()
        .filter(|c| c.kind == ContributionRecordKind::Donation)
        .map(|c| c.project_url.as_str())
        .collect();

    let docs_contributed: HashSet<&str> = contributions
        .iter()
        .filter(|c| c.kind == ContributionRecordKind::Docs)
        .map(|c| c.project_url.as_str())
        .collect();

    let filter_set: HashSet<SuggestionKind> = filter.iter().copied().collect();

    let generators: &[(SuggestionKind, Generator)] = &[
        (SuggestionKind::Star, generate_star),
        (SuggestionKind::Issue, generate_issue),
        (SuggestionKind::Donate, generate_donate),
        (SuggestionKind::Docs, generate_docs),
        (SuggestionKind::Spread, generate_spread),
    ];

    let mut suggestions = Vec::new();

    for project in projects {
        let project_url = match &project.repo_url {
            Some(url) => url.as_str(),
            None => continue,
        };

        for &(kind, generator) in generators {
            if !filter_set.contains(&kind) {
                continue;
            }

            // Skip if the user already made this kind of contribution
            let dominated = match kind {
                SuggestionKind::Star => starred.contains(project_url),
                SuggestionKind::Donate => donated.contains(project_url),
                SuggestionKind::Docs => docs_contributed.contains(project_url),
                // Issue and Spread are always fresh suggestions
                _ => false,
            };
            if dominated {
                continue;
            }

            if let Some(suggestion) = generator(project) {
                suggestions.push(suggestion);
            }
        }
    }

    suggestions
}

/// Suggest starring a GitHub project.
fn generate_star(project: &UpstreamProject) -> Option<ContributionSuggestion> {
    let repo_url = project.repo_url.as_ref()?;
    if !repo_url.contains("github.com") {
        return None;
    }
    Some(ContributionSuggestion {
        kind: SuggestionKind::Star,
        title: format!("Star {} on GitHub", project.name),
        url: repo_url.clone(),
    })
}

/// Suggest checking good first issues on a project.
fn generate_issue(project: &UpstreamProject) -> Option<ContributionSuggestion> {
    let url = project.good_first_issues_url.as_ref()?;
    Some(ContributionSuggestion {
        kind: SuggestionKind::Issue,
        title: format!("Check good first issues on {}", project.name),
        url: url.clone(),
    })
}

/// Suggest donating to a project via its first funding channel.
fn generate_donate(project: &UpstreamProject) -> Option<ContributionSuggestion> {
    let channel = project.funding.first()?;
    Some(ContributionSuggestion {
        kind: SuggestionKind::Donate,
        title: format!("Donate to {} via {}", project.name, channel.platform),
        url: channel.url.clone(),
    })
}

/// Suggest improving documentation for a project with a contributing guide.
fn generate_docs(project: &UpstreamProject) -> Option<ContributionSuggestion> {
    let url = project.contributing_url.as_ref()?;
    Some(ContributionSuggestion {
        kind: SuggestionKind::Docs,
        title: format!("Improve documentation for {}", project.name),
        url: url.clone(),
    })
}

/// Suggest sharing a lesser-known project (below the star threshold).
fn generate_spread(project: &UpstreamProject) -> Option<ContributionSuggestion> {
    let repo_url = project.repo_url.as_ref()?;
    if !repo_url.contains("github.com") {
        return None;
    }
    let stars = project.stars?;
    if stars >= SPREAD_STAR_THRESHOLD {
        return None;
    }
    Some(ContributionSuggestion {
        kind: SuggestionKind::Spread,
        title: format!(
            "Share {} — a lesser-known project with {} stars",
            project.name, stars
        ),
        url: repo_url.clone(),
    })
}

/// Randomly shuffle and pick up to `n` suggestions.
pub fn pick_random(
    mut suggestions: Vec<ContributionSuggestion>,
    n: usize,
) -> Vec<ContributionSuggestion> {
    let mut rng = rand::rng();
    suggestions.shuffle(&mut rng);
    suggestions.truncate(n);
    suggestions
}

/// Format suggestions for terminal display.
///
/// Produces the numbered list described in the `contribute` command spec:
///
/// ```text
/// Here are 3 ways you can support open source today:
///
///   1. ⭐ Star curl/curl on GitHub
///      https://github.com/curl/curl
/// ```
pub fn format_suggestions(suggestions: &[ContributionSuggestion]) -> String {
    let count = suggestions.len();
    if count == 0 {
        return String::new();
    }

    let noun = if count == 1 { "way" } else { "ways" };
    let mut out = format!("Here are {count} {noun} you can support open source today:\n");

    for (i, s) in suggestions.iter().enumerate() {
        let num = i + 1;
        let emoji = s.kind.emoji();
        out.push_str(&format!("\n  {num}. {emoji} {}\n     {}\n", s.title, s.url));
    }

    out
}

/// Format suggestions for hook output (stderr, brief, non-intrusive).
///
/// Produces a compact list suitable for display after package manager
/// transactions:
///
/// ```text
/// syld: 3 ways to support the packages you just installed:
///
///   1. ⭐ Star curl/curl on GitHub
///      https://github.com/curl/curl
///
/// Run `syld contribute` for more suggestions.
/// ```
pub fn format_hook_suggestions(suggestions: &[ContributionSuggestion]) -> String {
    let count = suggestions.len();
    if count == 0 {
        return String::new();
    }

    let noun = if count == 1 { "way" } else { "ways" };
    let mut out = format!("\nsyld: {count} {noun} to support the packages you just installed:\n");

    for (i, s) in suggestions.iter().enumerate() {
        let num = i + 1;
        let emoji = s.kind.emoji();
        out.push_str(&format!("\n  {num}. {emoji} {}\n     {}\n", s.title, s.url));
    }

    out.push_str("\nRun `syld contribute` for more suggestions.\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::FundingChannel;
    use chrono::Utc;

    fn sample_project() -> UpstreamProject {
        UpstreamProject {
            name: "test-project".to_string(),
            repo_url: Some("https://github.com/example/test-project".to_string()),
            homepage: None,
            licenses: vec![],
            funding: vec![],
            bug_tracker: None,
            contributing_url: None,
            is_open_source: None,
            documentation_url: None,
            good_first_issues_url: None,
            stars: None,
        }
    }

    fn star_record(project_url: &str) -> ContributionRecord {
        ContributionRecord {
            id: 1,
            project_url: project_url.to_string(),
            kind: ContributionRecordKind::Star,
            title: None,
            url: None,
            contributed_at: Utc::now(),
            source: None,
        }
    }

    #[test]
    fn generate_star_github_project() {
        let project = sample_project();
        let s = generate_star(&project).unwrap();
        assert_eq!(s.kind, SuggestionKind::Star);
        assert!(s.title.contains("test-project"));
        assert!(s.url.contains("github.com"));
    }

    #[test]
    fn generate_star_skips_non_github() {
        let mut project = sample_project();
        project.repo_url = Some("https://gitlab.com/example/repo".to_string());
        assert!(generate_star(&project).is_none());
    }

    #[test]
    fn generate_star_skips_no_repo() {
        let mut project = sample_project();
        project.repo_url = None;
        assert!(generate_star(&project).is_none());
    }

    #[test]
    fn generate_issue_with_good_first_issues() {
        let mut project = sample_project();
        project.good_first_issues_url =
            Some("https://github.com/example/repo/issues?q=label:good+first+issue".to_string());
        let s = generate_issue(&project).unwrap();
        assert_eq!(s.kind, SuggestionKind::Issue);
        assert!(s.title.contains("good first issues"));
    }

    #[test]
    fn generate_issue_skips_no_url() {
        let project = sample_project();
        assert!(generate_issue(&project).is_none());
    }

    #[test]
    fn generate_donate_with_funding() {
        let mut project = sample_project();
        project.funding = vec![FundingChannel {
            platform: "Open Collective".to_string(),
            url: "https://opencollective.com/test".to_string(),
        }];
        let s = generate_donate(&project).unwrap();
        assert_eq!(s.kind, SuggestionKind::Donate);
        assert!(s.title.contains("Open Collective"));
        assert_eq!(s.url, "https://opencollective.com/test");
    }

    #[test]
    fn generate_donate_skips_no_funding() {
        let project = sample_project();
        assert!(generate_donate(&project).is_none());
    }

    #[test]
    fn generate_docs_with_contributing_url() {
        let mut project = sample_project();
        project.contributing_url =
            Some("https://github.com/example/repo/blob/main/CONTRIBUTING.md".to_string());
        let s = generate_docs(&project).unwrap();
        assert_eq!(s.kind, SuggestionKind::Docs);
        assert!(s.title.contains("documentation"));
    }

    #[test]
    fn generate_docs_skips_no_url() {
        let project = sample_project();
        assert!(generate_docs(&project).is_none());
    }

    #[test]
    fn generate_spread_low_stars() {
        let mut project = sample_project();
        project.stars = Some(50);
        let s = generate_spread(&project).unwrap();
        assert_eq!(s.kind, SuggestionKind::Spread);
        assert!(s.title.contains("50 stars"));
    }

    #[test]
    fn generate_spread_skips_popular() {
        let mut project = sample_project();
        project.stars = Some(5000);
        assert!(generate_spread(&project).is_none());
    }

    #[test]
    fn generate_spread_skips_no_stars() {
        let project = sample_project();
        assert!(generate_spread(&project).is_none());
    }

    #[test]
    fn generate_spread_skips_non_github() {
        let mut project = sample_project();
        project.repo_url = Some("https://gitlab.com/example/repo".to_string());
        project.stars = Some(50);
        assert!(generate_spread(&project).is_none());
    }

    #[test]
    fn generate_suggestions_filters_already_starred() {
        let project = sample_project();
        let contributions = vec![star_record("https://github.com/example/test-project")];
        let suggestions = generate_suggestions(&[project], &contributions, &[SuggestionKind::Star]);
        assert!(suggestions.is_empty());
    }

    #[test]
    fn generate_suggestions_includes_unstarred() {
        let project = sample_project();
        let suggestions = generate_suggestions(&[project], &[], &[SuggestionKind::Star]);
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].kind, SuggestionKind::Star);
    }

    #[test]
    fn generate_suggestions_respects_type_filter() {
        let mut project = sample_project();
        project.good_first_issues_url = Some("https://example.com/issues".to_string());
        // Only ask for Star, not Issue
        let suggestions = generate_suggestions(&[project], &[], &[SuggestionKind::Star]);
        assert!(suggestions.iter().all(|s| s.kind == SuggestionKind::Star));
    }

    #[test]
    fn generate_suggestions_multiple_types() {
        let mut project = sample_project();
        project.good_first_issues_url = Some("https://example.com/issues".to_string());
        project.funding = vec![FundingChannel {
            platform: "GitHub Sponsors".to_string(),
            url: "https://github.com/sponsors/test".to_string(),
        }];
        let suggestions = generate_suggestions(&[project], &[], SuggestionKind::ALL);
        let kinds: HashSet<SuggestionKind> = suggestions.iter().map(|s| s.kind).collect();
        assert!(kinds.contains(&SuggestionKind::Star));
        assert!(kinds.contains(&SuggestionKind::Issue));
        assert!(kinds.contains(&SuggestionKind::Donate));
    }

    #[test]
    fn generate_suggestions_empty_projects() {
        let suggestions = generate_suggestions(&[], &[], SuggestionKind::ALL);
        assert!(suggestions.is_empty());
    }

    #[test]
    fn suggestion_kind_display_roundtrip() {
        for kind in SuggestionKind::ALL {
            let s = kind.to_string();
            let parsed: SuggestionKind = s.parse().unwrap();
            assert_eq!(*kind, parsed);
        }
    }

    #[test]
    fn suggestion_kind_from_str_unknown() {
        assert!("unknown".parse::<SuggestionKind>().is_err());
    }

    #[test]
    fn parse_types_single() {
        let types = parse_types("star").unwrap();
        assert_eq!(types, vec![SuggestionKind::Star]);
    }

    #[test]
    fn parse_types_multiple() {
        let types = parse_types("star,issue,donate").unwrap();
        assert_eq!(
            types,
            vec![
                SuggestionKind::Star,
                SuggestionKind::Issue,
                SuggestionKind::Donate,
            ]
        );
    }

    #[test]
    fn parse_types_with_whitespace() {
        let types = parse_types("star , docs").unwrap();
        assert_eq!(types, vec![SuggestionKind::Star, SuggestionKind::Docs]);
    }

    #[test]
    fn parse_types_invalid() {
        assert!(parse_types("star,bogus").is_err());
    }

    #[test]
    fn suggestion_kind_emoji() {
        // Just verify each kind has a non-empty emoji.
        for kind in SuggestionKind::ALL {
            assert!(!kind.emoji().is_empty());
        }
    }

    #[test]
    fn suggestion_serde_roundtrip() {
        let suggestion = ContributionSuggestion {
            kind: SuggestionKind::Star,
            title: "Star curl/curl on GitHub".to_string(),
            url: "https://github.com/curl/curl".to_string(),
        };
        let json = serde_json::to_string(&suggestion).unwrap();
        let parsed: ContributionSuggestion = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.kind, SuggestionKind::Star);
        assert_eq!(parsed.title, "Star curl/curl on GitHub");
        assert_eq!(parsed.url, "https://github.com/curl/curl");
    }

    // -- pick_random tests --

    fn make_suggestion(kind: SuggestionKind, name: &str) -> ContributionSuggestion {
        ContributionSuggestion {
            kind,
            title: format!("Action for {name}"),
            url: format!("https://example.com/{name}"),
        }
    }

    #[test]
    fn pick_random_limits_to_n() {
        let suggestions = vec![
            make_suggestion(SuggestionKind::Star, "a"),
            make_suggestion(SuggestionKind::Issue, "b"),
            make_suggestion(SuggestionKind::Donate, "c"),
            make_suggestion(SuggestionKind::Docs, "d"),
        ];
        let picked = pick_random(suggestions, 2);
        assert_eq!(picked.len(), 2);
    }

    #[test]
    fn pick_random_returns_all_when_fewer_than_n() {
        let suggestions = vec![make_suggestion(SuggestionKind::Star, "a")];
        let picked = pick_random(suggestions, 5);
        assert_eq!(picked.len(), 1);
    }

    #[test]
    fn pick_random_empty_input() {
        let picked = pick_random(vec![], 3);
        assert!(picked.is_empty());
    }

    // -- format_suggestions tests --

    #[test]
    fn format_suggestions_empty() {
        assert_eq!(format_suggestions(&[]), String::new());
    }

    #[test]
    fn format_suggestions_single() {
        let suggestions = vec![ContributionSuggestion {
            kind: SuggestionKind::Star,
            title: "Star curl/curl on GitHub".to_string(),
            url: "https://github.com/curl/curl".to_string(),
        }];
        let output = format_suggestions(&suggestions);
        assert!(output.starts_with("Here are 1 way you can support open source today:"));
        assert!(output.contains("1. ⭐ Star curl/curl on GitHub"));
        assert!(output.contains("https://github.com/curl/curl"));
    }

    #[test]
    fn format_suggestions_multiple() {
        let suggestions = vec![
            ContributionSuggestion {
                kind: SuggestionKind::Star,
                title: "Star curl/curl on GitHub".to_string(),
                url: "https://github.com/curl/curl".to_string(),
            },
            ContributionSuggestion {
                kind: SuggestionKind::Donate,
                title: "Donate to curl via Open Collective".to_string(),
                url: "https://opencollective.com/curl".to_string(),
            },
            ContributionSuggestion {
                kind: SuggestionKind::Issue,
                title: "Check good first issues on systemd/systemd".to_string(),
                url: "https://github.com/systemd/systemd/issues?q=label:\"good first issue\""
                    .to_string(),
            },
        ];
        let output = format_suggestions(&suggestions);
        assert!(output.starts_with("Here are 3 ways you can support open source today:"));
        assert!(output.contains("1. ⭐"));
        assert!(output.contains("2. 💰"));
        assert!(output.contains("3. 🐛"));
    }

    #[test]
    fn format_suggestions_numbering_and_indentation() {
        let suggestions = vec![
            make_suggestion(SuggestionKind::Star, "a"),
            make_suggestion(SuggestionKind::Issue, "b"),
        ];
        let output = format_suggestions(&suggestions);
        // Check indentation: numbers at 2 spaces, URLs at 5 spaces
        assert!(output.contains("  1."));
        assert!(output.contains("  2."));
        assert!(output.contains("     https://"));
    }

    // -- format_hook_suggestions tests --

    #[test]
    fn format_hook_suggestions_empty() {
        assert_eq!(format_hook_suggestions(&[]), String::new());
    }

    #[test]
    fn format_hook_suggestions_has_hook_header_and_footer() {
        let suggestions = vec![ContributionSuggestion {
            kind: SuggestionKind::Star,
            title: "Star curl/curl on GitHub".to_string(),
            url: "https://github.com/curl/curl".to_string(),
        }];
        let output = format_hook_suggestions(&suggestions);
        assert!(output.contains("syld: 1 way to support the packages you just installed:"));
        assert!(output.contains("Run `syld contribute` for more suggestions."));
    }

    #[test]
    fn format_hook_suggestions_multiple() {
        let suggestions = vec![
            make_suggestion(SuggestionKind::Star, "a"),
            make_suggestion(SuggestionKind::Donate, "b"),
            make_suggestion(SuggestionKind::Issue, "c"),
        ];
        let output = format_hook_suggestions(&suggestions);
        assert!(output.contains("3 ways to support"));
        assert!(output.contains("1. ⭐"));
        assert!(output.contains("2. 💰"));
        assert!(output.contains("3. 🐛"));
    }
}
