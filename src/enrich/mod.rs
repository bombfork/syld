// SPDX-License-Identifier: GPL-3.0-or-later

//! Network-based enrichment of project metadata.
//!
//! Enrichment fills in missing fields on [`UpstreamProject`] — stars, homepage,
//! bug tracker, contributing URL, documentation URL, funding channels, and
//! license-based OSI classification.
//!
//! Controlled at runtime via `--enrich` CLI flag or `enrich = true` in config.
//!
//! Enrichment sources:
//! - GitHub API (via `gh` CLI) — stars, homepage, license, issues, FUNDING.yml
//! - License classification — OSI-approved status from SPDX identifiers
//! - Open Collective API — funding channel lookup
//! - Liberapay API — funding channel lookup

pub mod github;
pub mod liberapay;
pub mod license_classify;
pub mod open_collective;

use std::collections::HashMap;

use anyhow::Result;
use indicatif::{ProgressBar, ProgressStyle};

use crate::config::Config;
use crate::discover::InstalledPackage;
use crate::project::{FundingChannel, UpstreamProject};
use crate::report::terminal::normalize_url;
use crate::storage::Storage;

/// Enriched project metadata keyed by normalized package URL.
pub type EnrichmentMap = HashMap<String, UpstreamProject>;

/// Trait for enrichment backends.
///
/// Each implementation enriches an [`UpstreamProject`] with additional metadata
/// from a particular source. The enriched project is returned as a new value —
/// the caller merges it with the base using [`merge_enrichment`].
pub trait EnrichmentBackend {
    /// A stable, lowercase identifier for this backend.
    fn name(&self) -> &str;

    /// Returns `true` if this backend can operate in the current environment.
    fn is_available(&self) -> bool;

    /// Enrich a project with additional metadata.
    ///
    /// Returns a new `UpstreamProject` with fields filled in from this source.
    /// Fields that this backend cannot determine should be left as-is (cloned
    /// from the input).
    fn enrich(&self, project: &UpstreamProject) -> Result<UpstreamProject>;
}

/// Returns all enrichment backends that are available in the current environment.
pub fn active_backends(_config: &Config) -> Vec<Box<dyn EnrichmentBackend>> {
    let candidates: Vec<Box<dyn EnrichmentBackend>> = vec![
        Box::new(license_classify::LicenseClassifyBackend),
        Box::new(github::GitHubBackend),
        Box::new(open_collective::OpenCollectiveBackend),
        Box::new(liberapay::LiberapayBackend),
    ];

    candidates
        .into_iter()
        .filter(|b| b.is_available())
        .collect()
}

/// Merge enriched data onto a base project.
///
/// Non-empty fields from `enriched` overlay `base`. Funding channels are
/// deduplicated by URL.
pub fn merge_enrichment(base: &UpstreamProject, enriched: &UpstreamProject) -> UpstreamProject {
    let mut result = base.clone();

    if result.homepage.is_none() && enriched.homepage.is_some() {
        result.homepage = enriched.homepage.clone();
    }
    if result.bug_tracker.is_none() && enriched.bug_tracker.is_some() {
        result.bug_tracker = enriched.bug_tracker.clone();
    }
    if result.contributing_url.is_none() && enriched.contributing_url.is_some() {
        result.contributing_url = enriched.contributing_url.clone();
    }
    if result.documentation_url.is_none() && enriched.documentation_url.is_some() {
        result.documentation_url = enriched.documentation_url.clone();
    }
    if result.good_first_issues_url.is_none() && enriched.good_first_issues_url.is_some() {
        result.good_first_issues_url = enriched.good_first_issues_url.clone();
    }
    if result.is_open_source.is_none() && enriched.is_open_source.is_some() {
        result.is_open_source = enriched.is_open_source;
    }
    if result.stars.is_none() && enriched.stars.is_some() {
        result.stars = enriched.stars;
    }

    // Merge licenses (deduplicate)
    for license in &enriched.licenses {
        if !result.licenses.contains(license) {
            result.licenses.push(license.clone());
        }
    }

    // Merge funding channels (deduplicate by URL)
    for channel in &enriched.funding {
        if !result.funding.iter().any(|f| f.url == channel.url) {
            result.funding.push(channel.clone());
        }
    }

    result
}

/// Enrich packages using all available backends.
///
/// Deduplicates packages by normalized URL, checks the enrichment cache first,
/// and runs each backend on cache misses. Results are saved back to cache.
///
/// Returns an `EnrichmentMap` keyed by normalized URL.
pub fn enrich_packages(
    packages: &[InstalledPackage],
    storage: &Storage,
    config: &Config,
) -> Result<EnrichmentMap> {
    let backends = active_backends(config);

    if backends.is_empty() {
        eprintln!("No enrichment backends available.");
        return Ok(EnrichmentMap::new());
    }

    eprintln!(
        "Enrichment backends: {}",
        backends
            .iter()
            .map(|b| b.name())
            .collect::<Vec<_>>()
            .join(", ")
    );

    // Collect unique URLs to enrich
    let mut url_to_project: HashMap<String, UpstreamProject> = HashMap::new();
    for pkg in packages {
        if let Some(url) = &pkg.url {
            let normalized = normalize_url(url);
            if !normalized.is_empty() {
                url_to_project
                    .entry(normalized)
                    .or_insert_with(|| UpstreamProject {
                        name: pkg.name.clone(),
                        repo_url: Some(url.clone()),
                        homepage: None,
                        licenses: pkg.licenses.clone(),
                        funding: vec![],
                        bug_tracker: None,
                        contributing_url: None,
                        is_open_source: None,
                        documentation_url: None,
                        good_first_issues_url: None,
                        stars: None,
                    });
            }
        }
    }

    let total = url_to_project.len();
    let pb = ProgressBar::new(total as u64);
    pb.set_style(
        ProgressStyle::with_template("Enriching [{bar:30}] {pos}/{len} {msg}")
            .unwrap()
            .progress_chars("=> "),
    );

    let mut enrichment_map = EnrichmentMap::new();

    for (normalized_url, base_project) in &url_to_project {
        pb.set_message(base_project.name.clone());

        // Check cache first (use the original URL from repo_url as cache key)
        let cache_key = base_project.repo_url.as_deref().unwrap_or(normalized_url);

        if let Ok(Some(cached)) = storage.get_enrichment(cache_key) {
            enrichment_map.insert(normalized_url.clone(), cached);
            pb.inc(1);
            continue;
        }

        // Run all backends
        let mut enriched = base_project.clone();
        for backend in &backends {
            match backend.enrich(&enriched) {
                Ok(result) => {
                    enriched = merge_enrichment(&enriched, &result);
                }
                Err(e) => {
                    eprintln!(
                        "Warning: {} enrichment failed for {}: {e}",
                        backend.name(),
                        base_project.name
                    );
                }
            }
        }

        // Save to cache
        if let Err(e) = storage.save_enrichment(cache_key, &enriched) {
            eprintln!(
                "Warning: failed to cache enrichment for {}: {e}",
                base_project.name
            );
        }

        enrichment_map.insert(normalized_url.clone(), enriched);
        pb.inc(1);
    }

    pb.finish_with_message("done");
    eprintln!("Enriched {} projects", enrichment_map.len());

    Ok(enrichment_map)
}

/// Build a `FundingChannel` — convenience constructor used across backends.
pub fn funding_channel(platform: &str, url: String) -> FundingChannel {
    FundingChannel {
        platform: platform.to_string(),
        url,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_project(name: &str) -> UpstreamProject {
        UpstreamProject {
            name: name.to_string(),
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
        }
    }

    #[test]
    fn merge_fills_empty_fields() {
        let base = empty_project("test");
        let enriched = UpstreamProject {
            homepage: Some("https://example.com".to_string()),
            stars: Some(42),
            bug_tracker: Some("https://example.com/issues".to_string()),
            is_open_source: Some(true),
            ..empty_project("test")
        };

        let result = merge_enrichment(&base, &enriched);
        assert_eq!(result.homepage.as_deref(), Some("https://example.com"));
        assert_eq!(result.stars, Some(42));
        assert_eq!(
            result.bug_tracker.as_deref(),
            Some("https://example.com/issues")
        );
        assert_eq!(result.is_open_source, Some(true));
    }

    #[test]
    fn merge_does_not_overwrite_existing() {
        let base = UpstreamProject {
            homepage: Some("https://original.com".to_string()),
            stars: Some(100),
            ..empty_project("test")
        };
        let enriched = UpstreamProject {
            homepage: Some("https://new.com".to_string()),
            stars: Some(200),
            ..empty_project("test")
        };

        let result = merge_enrichment(&base, &enriched);
        assert_eq!(result.homepage.as_deref(), Some("https://original.com"));
        assert_eq!(result.stars, Some(100));
    }

    #[test]
    fn merge_deduplicates_funding_by_url() {
        let base = UpstreamProject {
            funding: vec![FundingChannel {
                platform: "GitHub Sponsors".to_string(),
                url: "https://github.com/sponsors/test".to_string(),
            }],
            ..empty_project("test")
        };
        let enriched = UpstreamProject {
            funding: vec![
                FundingChannel {
                    platform: "GitHub Sponsors".to_string(),
                    url: "https://github.com/sponsors/test".to_string(), // duplicate
                },
                FundingChannel {
                    platform: "Open Collective".to_string(),
                    url: "https://opencollective.com/test".to_string(), // new
                },
            ],
            ..empty_project("test")
        };

        let result = merge_enrichment(&base, &enriched);
        assert_eq!(result.funding.len(), 2);
        assert_eq!(result.funding[0].platform, "GitHub Sponsors");
        assert_eq!(result.funding[1].platform, "Open Collective");
    }

    #[test]
    fn merge_deduplicates_licenses() {
        let base = UpstreamProject {
            licenses: vec!["MIT".to_string()],
            ..empty_project("test")
        };
        let enriched = UpstreamProject {
            licenses: vec!["MIT".to_string(), "Apache-2.0".to_string()],
            ..empty_project("test")
        };

        let result = merge_enrichment(&base, &enriched);
        assert_eq!(result.licenses, vec!["MIT", "Apache-2.0"]);
    }

    #[test]
    fn merge_empty_enriched_is_noop() {
        let base = UpstreamProject {
            homepage: Some("https://example.com".to_string()),
            stars: Some(42),
            ..empty_project("test")
        };
        let enriched = empty_project("test");

        let result = merge_enrichment(&base, &enriched);
        assert_eq!(result.homepage.as_deref(), Some("https://example.com"));
        assert_eq!(result.stars, Some(42));
    }

    #[test]
    fn active_backends_does_not_panic() {
        let config = Config::default();
        let backends = active_backends(&config);
        // License classify is always available
        assert!(backends.iter().any(|b| b.name() == "license_classify"));
    }
}
