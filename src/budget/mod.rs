// SPDX-License-Identifier: GPL-3.0-or-later

//! Budget management and donation plan generation.
//!
//! Given a user's monthly/yearly budget and a list of discovered projects,
//! this module generates a donation plan that distributes the budget across
//! projects according to the chosen allocation strategy.

use serde::{Deserialize, Serialize};

use crate::project::UpstreamProject;

/// A complete donation plan for a budget period.
#[derive(Debug, Serialize, Deserialize)]
pub struct DonationPlan {
    pub allocations: Vec<Allocation>,
}

/// A single allocation in a donation plan.
#[derive(Debug, Serialize, Deserialize)]
pub struct Allocation {
    /// The project to donate to
    pub project: UpstreamProject,

    /// Amount per donation
    pub amount: f64,

    /// Donate every N months
    pub every_n_months: u32,

    /// Suggested funding channel
    pub via: Option<String>,

    /// Reason for including this project (e.g. "top dependency", "most used")
    pub reason: Option<String>,
}

/// A record of a completed donation.
#[derive(Debug, Serialize, Deserialize)]
pub struct DonationRecord {
    /// Database row ID
    pub id: i64,

    /// URL of the project that received the donation
    pub project_url: String,

    /// Amount donated
    pub amount: f64,

    /// Currency code (e.g. "USD", "EUR")
    pub currency: String,

    /// When the donation was made
    pub donated_at: chrono::DateTime<chrono::Utc>,

    /// Funding channel used
    pub via: Option<String>,

    /// Free-form notes
    pub notes: Option<String>,
}
