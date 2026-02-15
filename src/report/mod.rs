// SPDX-License-Identifier: GPL-3.0-or-later

//! Report generation in multiple output formats.

use std::collections::HashMap;

use crate::contribute::ContributionOpportunity;

pub mod html;
pub mod json;
pub mod terminal;

/// Contribution opportunities keyed by normalized project URL.
///
/// Report functions accept this as an optional parameter so they can display
/// a "Ways to Help" section alongside the existing package/project tables.
pub type ContributionMap = HashMap<String, Vec<ContributionOpportunity>>;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contribute::ContributionKind;

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
}
