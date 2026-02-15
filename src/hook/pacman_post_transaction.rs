// SPDX-License-Identifier: GPL-3.0-or-later

//! Pacman post-transaction hook.
//!
//! Surfaces contribution opportunities after pacman installs or upgrades
//! packages. Reads existing scan and enrichment data from the local SQLite
//! cache — no network calls, so it's fast enough for inline pacman output.

use std::path::Path;

use anyhow::Result;

use super::{Hook, HookContext};
use crate::report::terminal::normalize_url;
use crate::storage::Storage;

const PACMAN_DB_PATH: &str = "/var/lib/pacman/local";

/// Post-transaction hook for pacman (Arch Linux).
///
/// After pacman installs or upgrades packages, this hook checks the local
/// enrichment cache for funding links and prints a brief summary to stderr.
/// It never makes network calls and returns `Ok(())` silently when there is
/// no data to show.
pub struct PacmanPostTransactionHook;

impl Hook for PacmanPostTransactionHook {
    fn name(&self) -> &str {
        "pacman-post-transaction"
    }

    fn description(&self) -> &str {
        "Show funding opportunities after pacman transactions"
    }

    fn is_available(&self) -> bool {
        Path::new(PACMAN_DB_PATH).is_dir()
    }

    fn run(&self, ctx: &HookContext) -> Result<()> {
        if ctx.targets.is_empty() {
            return Ok(());
        }

        let storage = match Storage::open() {
            Ok(s) => s,
            Err(_) => return Ok(()), // No database yet — nothing to report
        };

        let scan = match storage.latest_scan()? {
            Some(s) => s,
            None => return Ok(()), // No scan data
        };

        // Match targets against scanned packages to find their URLs
        let mut matched_urls: Vec<(String, String)> = Vec::new();
        for target in &ctx.targets {
            for pkg in &scan.packages {
                if pkg.name == *target {
                    if let Some(url) = &pkg.url {
                        matched_urls.push((pkg.name.clone(), url.clone()));
                    }
                    break;
                }
            }
        }

        if matched_urls.is_empty() {
            return Ok(());
        }

        // Look up enrichment cache for funding links
        let mut fundable: Vec<(String, Vec<String>)> = Vec::new();
        for (name, url) in &matched_urls {
            // Try both the raw URL and the normalized form as cache keys
            let cached = storage.get_enrichment(url).ok().flatten().or_else(|| {
                let normalized = normalize_url(url);
                storage.get_enrichment(&normalized).ok().flatten()
            });

            if let Some(project) = cached
                && !project.funding.is_empty()
            {
                let links: Vec<String> = project.funding.iter().map(|f| f.url.clone()).collect();
                fundable.push((name.clone(), links));
            }
        }

        if fundable.is_empty() {
            return Ok(());
        }

        // Print a brief summary (max 3 projects) to stderr
        let show_count = fundable.len().min(3);
        eprintln!();
        eprintln!("  Some of these packages accept donations:");
        for (name, links) in fundable.iter().take(show_count) {
            let link = &links[0];
            eprintln!("    {name}: {link}");
        }
        if fundable.len() > 3 {
            eprintln!(
                "    ...and {} more (run `syld report --enrich` to see all)",
                fundable.len() - 3
            );
        }
        eprintln!();

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_and_description() {
        let hook = PacmanPostTransactionHook;
        assert_eq!(hook.name(), "pacman-post-transaction");
        assert!(!hook.description().is_empty());
    }

    #[test]
    fn run_with_empty_targets_succeeds() {
        let hook = PacmanPostTransactionHook;
        let config = crate::config::Config::default();
        let ctx = HookContext {
            config: &config,
            targets: vec![],
        };
        // Should return Ok silently
        assert!(hook.run(&ctx).is_ok());
    }
}
