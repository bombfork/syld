// SPDX-License-Identifier: GPL-3.0-or-later

//! Liberapay enrichment backend.
//!
//! Checks if a project has a Liberapay account by querying their public API.
//! Adds a funding channel if found.

use anyhow::Result;

use super::EnrichmentBackend;
use crate::project::{FundingChannel, UpstreamProject};

pub struct LiberapayBackend;

impl EnrichmentBackend for LiberapayBackend {
    fn name(&self) -> &str {
        "liberapay"
    }

    fn is_available(&self) -> bool {
        true
    }

    fn enrich(&self, project: &UpstreamProject) -> Result<UpstreamProject> {
        let name = &project.name;

        // Skip if we already have a Liberapay funding channel
        if project.funding.iter().any(|f| f.platform == "Liberapay") {
            return Ok(project.clone());
        }

        let url = format!("https://liberapay.com/{name}/public.json");

        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()?;

        let response = client.get(&url).send();

        match response {
            Ok(resp) if resp.status().is_success() => {
                let mut enriched = project.clone();
                enriched.funding.push(FundingChannel {
                    platform: "Liberapay".to_string(),
                    url: format!("https://liberapay.com/{name}"),
                });
                Ok(enriched)
            }
            _ => Ok(project.clone()),
        }
    }
}
