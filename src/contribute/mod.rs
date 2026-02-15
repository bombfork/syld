// SPDX-License-Identifier: GPL-3.0-or-later

//! Non-monetary contribution discovery system.
//!
//! This module provides a pluggable framework for surfacing ways users can
//! contribute to the upstream open source projects they depend on, beyond
//! financial donations. Each contribution type is represented by a *backend*
//! that implements the [`ContributionBackend`] trait. At runtime the
//! application calls [`active_backends()`] to obtain the subset of backends
//! that are available, and then queries each one for contribution
//! opportunities.
//!
//! # Adding a new backend
//!
//! Follow these steps to add a new contribution type. For a complete
//! reference implementation, see
//! [`github_good_first_issues::GitHubGoodFirstIssuesBackend`].
//!
//! ## 1. Create a module file
//!
//! Add a new file under `src/contribute/` (e.g. `github_stars.rs`) and
//! declare it in this module with `pub mod github_stars;`. Define a public
//! unit struct to represent the backend:
//!
//! ```rust,ignore
//! pub struct GitHubStarsBackend;
//! ```
//!
//! ## 2. Implement [`ContributionBackend`]
//!
//! The trait has three required methods:
//!
//! - **[`name()`](ContributionBackend::name)** — Return a stable, lowercase
//!   identifier (e.g. `"github_stars"`). This string appears in reports and
//!   logs and must not change between releases.
//!
//! - **[`is_available()`](ContributionBackend::is_available)** — Return
//!   `true` if the backend can operate in the current environment. This is
//!   called at startup to filter backends, so it must be **cheap and fast**.
//!   Typical checks include verifying that a CLI tool exists, an API token
//!   is set, or a well-known path is present. Avoid network round-trips or
//!   heavy computation here.
//!
//! - **[`find_opportunities()`](ContributionBackend::find_opportunities)** —
//!   Inspect the [`UpstreamProject`] metadata (repo URL, bug tracker,
//!   documentation URL, etc.) and return a `Vec<ContributionOpportunity>`.
//!   Returning an empty vector is fine when the project is not applicable
//!   (e.g. not hosted on the right platform). For unrecoverable failures
//!   (network timeouts, malformed responses), return an `Err` — the caller
//!   logs the error and continues with other backends.
//!
//! ## 3. Add a [`ContributionKind`] variant (if needed)
//!
//! If no existing [`ContributionKind`] variant fits the new action, add one
//! to the enum. Remember to update the [`Display`](std::fmt::Display) impl
//! and the ordering tests in this module.
//!
//! ## 4. Register the backend
//!
//! In [`active_backends()`], append a `Box::new(YourBackend)` entry to the
//! `candidates` vector. The new backend will be included automatically
//! whenever its [`is_available()`](ContributionBackend::is_available) check
//! passes.
//!
//! # Example
//!
//! A minimal backend that suggests starring GitHub repositories:
//!
//! ```rust,ignore
//! use anyhow::Result;
//! use crate::contribute::{ContributionBackend, ContributionKind, ContributionOpportunity};
//! use crate::project::UpstreamProject;
//!
//! pub struct GitHubStarsBackend;
//!
//! impl ContributionBackend for GitHubStarsBackend {
//!     fn name(&self) -> &str {
//!         "github_stars"
//!     }
//!
//!     fn is_available(&self) -> bool {
//!         // Check that the `gh` CLI is authenticated (cheap subprocess call).
//!         std::process::Command::new("gh")
//!             .args(["auth", "status"])
//!             .output()
//!             .map(|o| o.status.success())
//!             .unwrap_or(false)
//!     }
//!
//!     fn find_opportunities(
//!         &self,
//!         project: &UpstreamProject,
//!     ) -> Result<Vec<ContributionOpportunity>> {
//!         let repo_url = match &project.repo_url {
//!             Some(url) if url.contains("github.com") => url,
//!             _ => return Ok(Vec::new()),
//!         };
//!
//!         Ok(vec![ContributionOpportunity {
//!             kind: ContributionKind::Star,
//!             title: format!("Star {} on GitHub", project.name),
//!             description: None,
//!             url: repo_url.clone(),
//!         }])
//!     }
//! }
//! ```
//!
//! See the parent issue <https://github.com/bombfork/syld/issues/26> for
//! the full design context.

pub mod github_good_first_issues;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::project::UpstreamProject;

/// The kind of non-monetary contribution a user can make.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ContributionKind {
    /// Star or favourite the project on its hosting platform.
    Star,
    /// Work on a beginner-friendly issue.
    GoodFirstIssue,
    /// Report a bug through the project's issue tracker.
    BugReport,
    /// Help translate the project into other languages.
    Translation,
    /// Improve project documentation.
    Documentation,
    /// Share the project on social media or a blog.
    SpreadTheWord,
}

impl std::fmt::Display for ContributionKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContributionKind::Star => write!(f, "star"),
            ContributionKind::GoodFirstIssue => write!(f, "good first issue"),
            ContributionKind::BugReport => write!(f, "bug report"),
            ContributionKind::Translation => write!(f, "translation"),
            ContributionKind::Documentation => write!(f, "documentation"),
            ContributionKind::SpreadTheWord => write!(f, "spread the word"),
        }
    }
}

/// A concrete opportunity to contribute to an upstream project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContributionOpportunity {
    /// What kind of contribution this is.
    pub kind: ContributionKind,

    /// Human-readable title (e.g. an issue title, or "Star on GitHub").
    pub title: String,

    /// Optional longer description or context.
    pub description: Option<String>,

    /// URL the user can visit to act on this opportunity.
    pub url: String,
}

/// Trait for non-monetary contribution backends.
///
/// Each implementation surfaces a particular type of contribution opportunity
/// (e.g. GitHub stars, good first issues) for upstream projects. The lifecycle
/// mirrors [`crate::discover::Discoverer`]:
///
/// 1. The backend is instantiated unconditionally.
/// 2. [`ContributionBackend::is_available()`] is called to check whether the
///    backend can operate (e.g. required API tokens are present).
/// 3. If available, [`ContributionBackend::find_opportunities()`] is called
///    for each upstream project to discover actionable contributions.
pub trait ContributionBackend {
    /// A stable, lowercase identifier for this backend.
    ///
    /// Used in reports, storage, and log output. Must not change between
    /// releases.
    fn name(&self) -> &str;

    /// Returns `true` if this backend can operate in the current environment.
    ///
    /// This method is called at startup to filter the set of active backends.
    /// It should be **cheap and fast** — e.g. checking for the presence of an
    /// API token or a CLI tool.
    fn is_available(&self) -> bool;

    /// Discovers contribution opportunities for the given upstream project.
    ///
    /// Backends should inspect the project's metadata (repo URL, bug tracker,
    /// etc.) to determine what actions are possible. Returning an empty vector
    /// is fine when no opportunities apply.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend encounters an unrecoverable failure
    /// (e.g. network timeout, malformed API response). The caller will log
    /// the error and continue with other backends.
    fn find_opportunities(&self, project: &UpstreamProject)
    -> Result<Vec<ContributionOpportunity>>;
}

/// Returns all contribution backends that are available in the current
/// environment.
///
/// Every known backend is instantiated and then filtered through
/// [`ContributionBackend::is_available()`]. Only backends that can operate
/// are returned.
///
/// # Registering a new backend
///
/// To add support for another contribution type, append a
/// `Box::new(YourBackend)` entry to the `candidates` vector below. The new
/// backend will automatically be included whenever its
/// [`is_available()`](ContributionBackend::is_available) check passes.
pub fn active_backends(_config: &Config) -> Vec<Box<dyn ContributionBackend>> {
    let candidates: Vec<Box<dyn ContributionBackend>> = vec![Box::new(
        github_good_first_issues::GitHubGoodFirstIssuesBackend,
    )];

    candidates
        .into_iter()
        .filter(|b| b.is_available())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contribution_kind_display() {
        assert_eq!(ContributionKind::Star.to_string(), "star");
        assert_eq!(
            ContributionKind::GoodFirstIssue.to_string(),
            "good first issue"
        );
        assert_eq!(ContributionKind::BugReport.to_string(), "bug report");
        assert_eq!(ContributionKind::Translation.to_string(), "translation");
        assert_eq!(ContributionKind::Documentation.to_string(), "documentation");
        assert_eq!(
            ContributionKind::SpreadTheWord.to_string(),
            "spread the word"
        );
    }

    #[test]
    fn contribution_kind_ordering() {
        // Enum variants should have a stable ordering for consistent display.
        assert!(ContributionKind::Star < ContributionKind::GoodFirstIssue);
        assert!(ContributionKind::GoodFirstIssue < ContributionKind::BugReport);
        assert!(ContributionKind::Documentation < ContributionKind::SpreadTheWord);
    }

    #[test]
    fn opportunity_serde_roundtrip() {
        let opportunity = ContributionOpportunity {
            kind: ContributionKind::GoodFirstIssue,
            title: "Fix typo in README".to_string(),
            description: Some("Simple fix for a documentation typo".to_string()),
            url: "https://github.com/example/repo/issues/42".to_string(),
        };

        let json = serde_json::to_string(&opportunity).unwrap();
        let deserialized: ContributionOpportunity = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.kind, ContributionKind::GoodFirstIssue);
        assert_eq!(deserialized.title, "Fix typo in README");
        assert_eq!(
            deserialized.description.as_deref(),
            Some("Simple fix for a documentation typo")
        );
        assert_eq!(
            deserialized.url,
            "https://github.com/example/repo/issues/42"
        );
    }

    #[test]
    fn opportunity_serde_without_description() {
        let opportunity = ContributionOpportunity {
            kind: ContributionKind::Star,
            title: "Star on GitHub".to_string(),
            description: None,
            url: "https://github.com/example/repo".to_string(),
        };

        let json = serde_json::to_string(&opportunity).unwrap();
        let deserialized: ContributionOpportunity = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.kind, ContributionKind::Star);
        assert!(deserialized.description.is_none());
    }

    /// Minimal mock backend for testing the registration pattern.
    struct MockBackend {
        available: bool,
    }

    impl ContributionBackend for MockBackend {
        fn name(&self) -> &str {
            "mock"
        }

        fn is_available(&self) -> bool {
            self.available
        }

        fn find_opportunities(
            &self,
            _project: &UpstreamProject,
        ) -> Result<Vec<ContributionOpportunity>> {
            Ok(vec![ContributionOpportunity {
                kind: ContributionKind::Star,
                title: "Star this project".to_string(),
                description: None,
                url: "https://example.com".to_string(),
            }])
        }
    }

    #[test]
    fn mock_backend_trait_object() {
        // Verify the trait can be used as a boxed trait object.
        let backend: Box<dyn ContributionBackend> = Box::new(MockBackend { available: true });
        assert_eq!(backend.name(), "mock");
        assert!(backend.is_available());

        let project = UpstreamProject {
            name: "test-project".to_string(),
            repo_url: Some("https://github.com/example/repo".to_string()),
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

        let opportunities = backend.find_opportunities(&project).unwrap();
        assert_eq!(opportunities.len(), 1);
        assert_eq!(opportunities[0].kind, ContributionKind::Star);
    }

    #[test]
    fn unavailable_backend_filtered() {
        let backends: Vec<Box<dyn ContributionBackend>> = vec![
            Box::new(MockBackend { available: true }),
            Box::new(MockBackend { available: false }),
            Box::new(MockBackend { available: true }),
        ];

        let active: Vec<_> = backends.into_iter().filter(|b| b.is_available()).collect();

        assert_eq!(active.len(), 2);
    }

    #[test]
    fn active_backends_returns_registered_backends() {
        let config = Config::default();
        let backends = active_backends(&config);
        // At least one backend is registered (GitHub good first issues).
        // Whether it appears depends on whether `gh` is authenticated,
        // so we just verify the call doesn't panic.
        let _ = backends;
    }
}
