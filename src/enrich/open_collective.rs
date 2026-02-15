// SPDX-License-Identifier: GPL-3.0-or-later

//! Open Collective enrichment backend.
//!
//! Checks if a project has an Open Collective page by querying their public API.
//! Adds a funding channel if found.

use anyhow::Result;

use super::EnrichmentBackend;
use crate::project::{FundingChannel, UpstreamProject};

pub struct OpenCollectiveBackend;

impl EnrichmentBackend for OpenCollectiveBackend {
    fn name(&self) -> &str {
        "open_collective"
    }

    fn is_available(&self) -> bool {
        true
    }

    fn enrich(&self, project: &UpstreamProject) -> Result<UpstreamProject> {
        // Try to derive a slug from the project name
        let slug = project.name.to_lowercase().replace(' ', "-");

        // Skip if we already have an Open Collective funding channel
        if project
            .funding
            .iter()
            .any(|f| f.platform == "Open Collective")
        {
            return Ok(project.clone());
        }

        let url = format!("https://api.opencollective.com/v1/collectives/{slug}");

        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()?;

        let response = client.get(&url).send();

        match response {
            Ok(resp) if resp.status().is_success() => {
                let mut enriched = project.clone();
                enriched.funding.push(FundingChannel {
                    platform: "Open Collective".to_string(),
                    url: format!("https://opencollective.com/{slug}"),
                });
                Ok(enriched)
            }
            _ => Ok(project.clone()),
        }
    }
}
