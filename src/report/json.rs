// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::contribute::ContributionOpportunity;
use crate::discover::InstalledPackage;
use crate::report::terminal::group_by_project;
use crate::report::{ContributionMap, lookup_contributions};

/// A grouped upstream project for the JSON report.
#[derive(Serialize)]
pub struct JsonProject {
    /// The grouping URL — either an exact project URL or a common ancestor prefix.
    pub url: String,
    /// Individual project URLs within an ancestor group.
    /// Empty array for single-project groups and the no-URL bucket.
    pub project_urls: Vec<String>,
    /// Names of all packages that belong to this group.
    pub package_names: Vec<String>,
    /// Contribution opportunities for this project.
    /// Empty when no contribution data is available.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub contributions: Vec<ContributionOpportunity>,
}

/// A JSON-serializable report of a scan.
#[derive(Serialize)]
pub struct JsonReport {
    pub scan_timestamp: DateTime<Utc>,
    pub total_packages: usize,
    pub total_projects: usize,
    pub packages_without_url: usize,
    pub projects_with_contributions: usize,
    pub total_contribution_opportunities: usize,
    pub projects: Vec<JsonProject>,
    pub packages: Vec<InstalledPackage>,
}

/// Generate a JSON report and print it to stdout.
pub fn print_json(
    packages: &[InstalledPackage],
    timestamp: DateTime<Utc>,
    contributions: &ContributionMap,
) -> Result<()> {
    let groups = group_by_project(packages);
    let total_projects = groups.iter().filter(|g| !g.url.is_empty()).count();
    let packages_without_url = packages.iter().filter(|p| p.url.is_none()).count();

    let projects: Vec<JsonProject> = groups
        .iter()
        .filter(|g| !g.url.is_empty())
        .map(|g| {
            let mut package_names: Vec<String> =
                g.packages.iter().map(|p| p.name.clone()).collect();
            package_names.sort();
            let project_contributions =
                lookup_contributions(&g.url, &g.project_urls, contributions);
            JsonProject {
                url: g.url.clone(),
                project_urls: g.project_urls.clone(),
                package_names,
                contributions: project_contributions,
            }
        })
        .collect();

    let projects_with_contributions = projects
        .iter()
        .filter(|p| !p.contributions.is_empty())
        .count();
    let total_contribution_opportunities: usize =
        projects.iter().map(|p| p.contributions.len()).sum();

    let report = JsonReport {
        scan_timestamp: timestamp,
        total_packages: packages.len(),
        total_projects,
        packages_without_url,
        projects_with_contributions,
        total_contribution_opportunities,
        projects,
        packages: packages.to_vec(),
    };

    let json = serde_json::to_string_pretty(&report)?;
    println!("{json}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discover::PackageSource;

    fn sample_packages() -> Vec<InstalledPackage> {
        vec![
            InstalledPackage {
                name: "firefox".to_string(),
                version: "128.0".to_string(),
                description: Some("Web browser".to_string()),
                url: Some("https://www.mozilla.org/firefox/".to_string()),
                source: PackageSource::Pacman,
                licenses: vec!["MPL-2.0".to_string()],
            },
            InstalledPackage {
                name: "linux".to_string(),
                version: "6.9.7".to_string(),
                description: None,
                url: Some("https://kernel.org".to_string()),
                source: PackageSource::Pacman,
                licenses: vec!["GPL-2.0".to_string()],
            },
        ]
    }

    #[test]
    fn json_report_structure() {
        let packages = sample_packages();
        let timestamp = "2025-01-15T10:30:00Z".parse::<DateTime<Utc>>().unwrap();

        let report = JsonReport {
            scan_timestamp: timestamp,
            total_packages: packages.len(),
            total_projects: 2,
            packages_without_url: 0,
            projects_with_contributions: 0,
            total_contribution_opportunities: 0,
            projects: vec![
                JsonProject {
                    url: "kernel.org".to_string(),
                    project_urls: vec![],
                    package_names: vec!["linux".to_string()],
                    contributions: vec![],
                },
                JsonProject {
                    url: "mozilla.org/firefox".to_string(),
                    project_urls: vec![],
                    package_names: vec!["firefox".to_string()],
                    contributions: vec![],
                },
            ],
            packages: packages.clone(),
        };

        let json = serde_json::to_string_pretty(&report).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["total_packages"], 2);
        assert_eq!(parsed["total_projects"], 2);
        assert_eq!(parsed["packages_without_url"], 0);
        assert_eq!(parsed["packages"][0]["name"], "firefox");
        assert_eq!(parsed["packages"][0]["version"], "128.0");
        assert_eq!(parsed["packages"][0]["source"], "Pacman");
        assert_eq!(parsed["packages"][0]["licenses"][0], "MPL-2.0");
        assert_eq!(parsed["packages"][1]["name"], "linux");
        assert!(parsed["scan_timestamp"].as_str().unwrap().contains("2025"));
        assert_eq!(parsed["projects"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn json_report_empty_packages() {
        let timestamp = "2025-01-15T10:30:00Z".parse::<DateTime<Utc>>().unwrap();

        let report = JsonReport {
            scan_timestamp: timestamp,
            total_packages: 0,
            total_projects: 0,
            packages_without_url: 0,
            projects_with_contributions: 0,
            total_contribution_opportunities: 0,
            projects: vec![],
            packages: vec![],
        };

        let json = serde_json::to_string_pretty(&report).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["total_packages"], 0);
        assert_eq!(parsed["total_projects"], 0);
        assert_eq!(parsed["packages_without_url"], 0);
        assert!(parsed["packages"].as_array().unwrap().is_empty());
        assert!(parsed["projects"].as_array().unwrap().is_empty());
    }

    #[test]
    fn json_report_optional_fields() {
        let packages = vec![InstalledPackage {
            name: "orphan".to_string(),
            version: "1.0".to_string(),
            description: None,
            url: None,
            source: PackageSource::Pacman,
            licenses: vec![],
        }];
        let timestamp = "2025-01-15T10:30:00Z".parse::<DateTime<Utc>>().unwrap();

        let report = JsonReport {
            scan_timestamp: timestamp,
            total_packages: 1,
            total_projects: 0,
            packages_without_url: 1,
            projects_with_contributions: 0,
            total_contribution_opportunities: 0,
            projects: vec![],
            packages,
        };

        let json = serde_json::to_string_pretty(&report).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert!(parsed["packages"][0]["description"].is_null());
        assert!(parsed["packages"][0]["url"].is_null());
        assert!(
            parsed["packages"][0]["licenses"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }

    fn load_schema() -> serde_json::Value {
        let raw = include_str!("../../schemas/report.v1.json");
        serde_json::from_str(raw).expect("schema is not valid JSON")
    }

    #[test]
    fn json_report_validates_against_schema() {
        let packages = sample_packages();
        let timestamp = "2025-01-15T10:30:00Z".parse::<DateTime<Utc>>().unwrap();

        let report = JsonReport {
            scan_timestamp: timestamp,
            total_packages: packages.len(),
            total_projects: 2,
            packages_without_url: 0,
            projects_with_contributions: 0,
            total_contribution_opportunities: 0,
            projects: vec![
                JsonProject {
                    url: "kernel.org".to_string(),
                    project_urls: vec![],
                    package_names: vec!["linux".to_string()],
                    contributions: vec![],
                },
                JsonProject {
                    url: "mozilla.org/firefox".to_string(),
                    project_urls: vec![],
                    package_names: vec!["firefox".to_string()],
                    contributions: vec![],
                },
            ],
            packages,
        };

        let json = serde_json::to_string_pretty(&report).unwrap();
        let instance: serde_json::Value = serde_json::from_str(&json).unwrap();
        let schema = load_schema();

        jsonschema::validate(&schema, &instance)
            .expect("JSON report should validate against the schema");
    }

    #[test]
    fn json_report_empty_validates_against_schema() {
        let timestamp = "2025-01-15T10:30:00Z".parse::<DateTime<Utc>>().unwrap();

        let report = JsonReport {
            scan_timestamp: timestamp,
            total_packages: 0,
            total_projects: 0,
            packages_without_url: 0,
            projects_with_contributions: 0,
            total_contribution_opportunities: 0,
            projects: vec![],
            packages: vec![],
        };

        let json = serde_json::to_string_pretty(&report).unwrap();
        let instance: serde_json::Value = serde_json::from_str(&json).unwrap();
        let schema = load_schema();

        jsonschema::validate(&schema, &instance)
            .expect("Empty JSON report should validate against the schema");
    }

    #[test]
    fn json_report_optional_fields_null_validates() {
        let packages = vec![InstalledPackage {
            name: "orphan".to_string(),
            version: "1.0".to_string(),
            description: None,
            url: None,
            source: PackageSource::Pacman,
            licenses: vec![],
        }];
        let timestamp = "2025-01-15T10:30:00Z".parse::<DateTime<Utc>>().unwrap();

        let report = JsonReport {
            scan_timestamp: timestamp,
            total_packages: packages.len(),
            total_projects: 0,
            packages_without_url: 1,
            projects_with_contributions: 0,
            total_contribution_opportunities: 0,
            projects: vec![],
            packages,
        };

        let json = serde_json::to_string_pretty(&report).unwrap();
        let instance: serde_json::Value = serde_json::from_str(&json).unwrap();
        let schema = load_schema();

        jsonschema::validate(&schema, &instance)
            .expect("Report with null optional fields should validate against the schema");
    }

    #[test]
    fn json_report_with_contributions() {
        use crate::contribute::{ContributionKind, ContributionOpportunity};

        let packages = sample_packages();
        let timestamp = "2025-01-15T10:30:00Z".parse::<DateTime<Utc>>().unwrap();

        let report = JsonReport {
            scan_timestamp: timestamp,
            total_packages: packages.len(),
            total_projects: 2,
            packages_without_url: 0,
            projects_with_contributions: 1,
            total_contribution_opportunities: 2,
            projects: vec![
                JsonProject {
                    url: "kernel.org".to_string(),
                    project_urls: vec![],
                    package_names: vec!["linux".to_string()],
                    contributions: vec![
                        ContributionOpportunity {
                            kind: ContributionKind::GoodFirstIssue,
                            title: "Fix typo in README".to_string(),
                            description: Some("Simple fix".to_string()),
                            url: "https://github.com/torvalds/linux/issues/1".to_string(),
                        },
                        ContributionOpportunity {
                            kind: ContributionKind::Documentation,
                            title: "Improve docs".to_string(),
                            description: None,
                            url: "https://github.com/torvalds/linux/issues/2".to_string(),
                        },
                    ],
                },
                JsonProject {
                    url: "mozilla.org/firefox".to_string(),
                    project_urls: vec![],
                    package_names: vec!["firefox".to_string()],
                    contributions: vec![],
                },
            ],
            packages: packages.clone(),
        };

        let json = serde_json::to_string_pretty(&report).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["projects_with_contributions"], 1);
        assert_eq!(parsed["total_contribution_opportunities"], 2);

        // Project with contributions includes them
        let kernel = &parsed["projects"][0];
        assert_eq!(kernel["contributions"].as_array().unwrap().len(), 2);
        assert_eq!(kernel["contributions"][0]["kind"], "GoodFirstIssue");
        assert_eq!(kernel["contributions"][0]["title"], "Fix typo in README");
        assert_eq!(kernel["contributions"][0]["description"], "Simple fix");

        // Project without contributions omits the field (skip_serializing_if)
        let firefox = &parsed["projects"][1];
        assert!(firefox.get("contributions").is_none());
    }

    #[test]
    fn json_report_with_contributions_validates_against_schema() {
        use crate::contribute::{ContributionKind, ContributionOpportunity};

        let packages = sample_packages();
        let timestamp = "2025-01-15T10:30:00Z".parse::<DateTime<Utc>>().unwrap();

        let report = JsonReport {
            scan_timestamp: timestamp,
            total_packages: packages.len(),
            total_projects: 2,
            packages_without_url: 0,
            projects_with_contributions: 1,
            total_contribution_opportunities: 1,
            projects: vec![JsonProject {
                url: "kernel.org".to_string(),
                project_urls: vec![],
                package_names: vec!["linux".to_string()],
                contributions: vec![ContributionOpportunity {
                    kind: ContributionKind::GoodFirstIssue,
                    title: "Fix bug".to_string(),
                    description: None,
                    url: "https://github.com/torvalds/linux/issues/1".to_string(),
                }],
            }],
            packages,
        };

        let json = serde_json::to_string_pretty(&report).unwrap();
        let instance: serde_json::Value = serde_json::from_str(&json).unwrap();
        let schema = load_schema();

        jsonschema::validate(&schema, &instance)
            .expect("Report with contributions should validate against the schema");
    }

    #[test]
    fn print_json_with_contributions() {
        use crate::contribute::{ContributionKind, ContributionOpportunity};

        let packages = sample_packages();
        let timestamp = "2025-01-15T10:30:00Z".parse::<DateTime<Utc>>().unwrap();

        let mut contributions = ContributionMap::new();
        contributions.insert(
            "kernel.org".to_string(),
            vec![ContributionOpportunity {
                kind: ContributionKind::GoodFirstIssue,
                title: "Fix bug".to_string(),
                description: None,
                url: "https://github.com/torvalds/linux/issues/1".to_string(),
            }],
        );

        // Just verify it doesn't panic — output goes to stdout
        let result = print_json(&packages, timestamp, &contributions);
        assert!(result.is_ok());
    }

    #[test]
    fn print_json_empty_contributions() {
        let packages = sample_packages();
        let timestamp = "2025-01-15T10:30:00Z".parse::<DateTime<Utc>>().unwrap();
        let contributions = ContributionMap::new();

        let result = print_json(&packages, timestamp, &contributions);
        assert!(result.is_ok());
    }
}
