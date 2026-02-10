// SPDX-License-Identifier: GPL-3.0-or-later

use serde::{Deserialize, Serialize};

/// An upstream open source project, potentially backing multiple installed packages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamProject {
    /// Canonical project name
    pub name: String,

    /// Source code repository URL
    pub repo_url: Option<String>,

    /// Project homepage
    pub homepage: Option<String>,

    /// License identifier(s)
    pub licenses: Vec<String>,

    /// Known funding/donation channels (populated by enrichment)
    pub funding: Vec<FundingChannel>,

    /// Bug tracker URL (populated by enrichment)
    pub bug_tracker: Option<String>,

    /// Contributing guide URL (populated by enrichment)
    pub contributing_url: Option<String>,
}

/// A way to financially support a project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FundingChannel {
    /// Platform name (e.g., "GitHub Sponsors", "Open Collective", "Liberapay")
    pub platform: String,

    /// URL to the funding page
    pub url: String,
}
