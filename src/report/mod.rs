// SPDX-License-Identifier: GPL-3.0-or-later

//! Report generation in multiple output formats.

use std::collections::HashMap;

use serde::Serialize;

use crate::contribute::{ContributionOpportunity, ContributionRecord, ContributionRecordKind};
use crate::enrich::EnrichmentMap;
use crate::project::UpstreamProject;

pub mod html;
pub mod json;
pub mod terminal;

/// Contribution opportunities keyed by normalized project URL.
///
/// Report functions accept this as an optional parameter so they can display
/// a "Ways to Help" section alongside the existing package/project tables.
pub type ContributionMap = HashMap<String, Vec<ContributionOpportunity>>;

/// Aggregated summary of the user's contribution history.
#[derive(Debug, Default, Serialize)]
pub struct ContributionSummary {
    /// Number of projects starred.
    pub stars: usize,
    /// Number of issues filed.
    pub issues: usize,
    /// Number of pull requests submitted.
    pub pull_requests: usize,
    /// Number of projects donated to.
    pub donations: usize,
    /// Total donation amount (if all donations share a currency).
    pub donation_total: Option<f64>,
    /// Currency of the donation total.
    pub donation_currency: Option<String>,
    /// Number of documentation contributions.
    pub docs: usize,
    /// Number of other contributions.
    pub other: usize,
}

impl ContributionSummary {
    /// Build a summary from a list of contribution records.
    pub fn from_records(records: &[ContributionRecord]) -> Self {
        let mut summary = Self::default();
        let mut donation_amounts: HashMap<String, f64> = HashMap::new();

        for record in records {
            match record.kind {
                ContributionRecordKind::Star => summary.stars += 1,
                ContributionRecordKind::Issue => summary.issues += 1,
                ContributionRecordKind::PullRequest => summary.pull_requests += 1,
                ContributionRecordKind::Donation => {
                    summary.donations += 1;
                    // Parse amount from title like "10 USD via GitHub Sponsors" or "25 EUR"
                    if let Some(title) = &record.title {
                        let parts: Vec<&str> = title.split_whitespace().collect();
                        if parts.len() >= 2
                            && let Ok(amount) = parts[0].parse::<f64>()
                        {
                            let currency = parts[1].to_string();
                            *donation_amounts.entry(currency).or_default() += amount;
                        }
                    }
                }
                ContributionRecordKind::Docs => summary.docs += 1,
                ContributionRecordKind::Other => summary.other += 1,
            }
        }

        // Set total only if all donations use the same currency
        if donation_amounts.len() == 1 {
            let (currency, total) = donation_amounts.into_iter().next().unwrap();
            summary.donation_total = Some(total);
            summary.donation_currency = Some(currency);
        }

        summary
    }

    /// Whether the summary has any contributions at all.
    pub fn is_empty(&self) -> bool {
        self.stars == 0
            && self.issues == 0
            && self.pull_requests == 0
            && self.donations == 0
            && self.docs == 0
            && self.other == 0
    }
}

/// Look up contributions for a project group, checking both the group URL and
/// any individual project URLs within an ancestor group.
pub fn lookup_contributions(
    group_url: &str,
    project_urls: &[String],
    contributions: &ContributionMap,
) -> Vec<ContributionOpportunity> {
    let mut result = Vec::new();

    if let Some(opps) = contributions.get(group_url) {
        result.extend(opps.iter().cloned());
    }

    for url in project_urls {
        if let Some(opps) = contributions.get(url.as_str()) {
            result.extend(opps.iter().cloned());
        }
    }

    result
}

/// Look up enrichment data for a project group, checking both the group URL
/// and any individual project URLs within an ancestor group.
///
/// Returns the first match found, since enrichment is per-project.
pub fn lookup_enrichment<'a>(
    group_url: &str,
    project_urls: &[String],
    enrichment: &'a EnrichmentMap,
) -> Option<&'a UpstreamProject> {
    if let Some(proj) = enrichment.get(group_url) {
        return Some(proj);
    }

    for url in project_urls {
        if let Some(proj) = enrichment.get(url.as_str()) {
            return Some(proj);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contribute::ContributionKind;
    use chrono::Utc;

    fn make_opp(kind: ContributionKind, title: &str) -> ContributionOpportunity {
        ContributionOpportunity {
            kind,
            title: title.to_string(),
            description: None,
            url: "https://example.com".to_string(),
        }
    }

    #[test]
    fn lookup_by_group_url() {
        let mut map = ContributionMap::new();
        map.insert(
            "github.com/foo".to_string(),
            vec![make_opp(ContributionKind::Star, "Star it")],
        );

        let result = lookup_contributions("github.com/foo", &[], &map);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "Star it");
    }

    #[test]
    fn lookup_by_project_urls() {
        let mut map = ContributionMap::new();
        map.insert(
            "github.com/org/repo-a".to_string(),
            vec![make_opp(ContributionKind::GoodFirstIssue, "Fix bug")],
        );

        let project_urls = vec!["github.com/org/repo-a".to_string()];
        let result = lookup_contributions("github.com/org", &project_urls, &map);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "Fix bug");
    }

    #[test]
    fn lookup_merges_group_and_project_urls() {
        let mut map = ContributionMap::new();
        map.insert(
            "github.com/org".to_string(),
            vec![make_opp(ContributionKind::Star, "Star org")],
        );
        map.insert(
            "github.com/org/repo-a".to_string(),
            vec![make_opp(ContributionKind::GoodFirstIssue, "Fix bug")],
        );

        let project_urls = vec!["github.com/org/repo-a".to_string()];
        let result = lookup_contributions("github.com/org", &project_urls, &map);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn lookup_empty_map_returns_empty() {
        let map = ContributionMap::new();
        let result = lookup_contributions("github.com/foo", &[], &map);
        assert!(result.is_empty());
    }

    #[test]
    fn lookup_no_match_returns_empty() {
        let mut map = ContributionMap::new();
        map.insert(
            "github.com/other".to_string(),
            vec![make_opp(ContributionKind::Star, "Star it")],
        );

        let result = lookup_contributions("github.com/foo", &[], &map);
        assert!(result.is_empty());
    }

    fn make_record(kind: ContributionRecordKind, title: Option<&str>) -> ContributionRecord {
        ContributionRecord {
            id: 0,
            project_url: "https://github.com/example".to_string(),
            kind,
            title: title.map(|s| s.to_string()),
            url: None,
            contributed_at: Utc::now(),
            source: None,
        }
    }

    #[test]
    fn contribution_summary_from_records() {
        let records = vec![
            make_record(ContributionRecordKind::Star, None),
            make_record(ContributionRecordKind::Star, None),
            make_record(ContributionRecordKind::Issue, Some("Fix bug")),
            make_record(ContributionRecordKind::PullRequest, Some("Add feature")),
            make_record(
                ContributionRecordKind::Donation,
                Some("10 USD via GitHub Sponsors"),
            ),
            make_record(ContributionRecordKind::Donation, Some("25 USD")),
            make_record(ContributionRecordKind::Docs, Some("Improve README")),
        ];

        let summary = ContributionSummary::from_records(&records);
        assert_eq!(summary.stars, 2);
        assert_eq!(summary.issues, 1);
        assert_eq!(summary.pull_requests, 1);
        assert_eq!(summary.donations, 2);
        assert_eq!(summary.donation_total, Some(35.0));
        assert_eq!(summary.donation_currency, Some("USD".to_string()));
        assert_eq!(summary.docs, 1);
        assert_eq!(summary.other, 0);
        assert!(!summary.is_empty());
    }

    #[test]
    fn contribution_summary_empty() {
        let summary = ContributionSummary::from_records(&[]);
        assert!(summary.is_empty());
        assert_eq!(summary.stars, 0);
        assert!(summary.donation_total.is_none());
    }

    #[test]
    fn contribution_summary_mixed_currencies() {
        let records = vec![
            make_record(ContributionRecordKind::Donation, Some("10 USD")),
            make_record(ContributionRecordKind::Donation, Some("25 EUR")),
        ];

        let summary = ContributionSummary::from_records(&records);
        assert_eq!(summary.donations, 2);
        // Mixed currencies — no total
        assert!(summary.donation_total.is_none());
        assert!(summary.donation_currency.is_none());
    }
}
