// SPDX-License-Identifier: GPL-3.0-or-later

//! Donation tracking.
//!
//! Records of completed donations, stored in the local database.

use serde::{Deserialize, Serialize};

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
