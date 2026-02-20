// SPDX-License-Identifier: GPL-3.0-or-later

//! Pacman post-transaction hook.
//!
//! Surfaces contribution opportunities after pacman installs or upgrades
//! packages. Uses the contribution suggestion engine to generate actionable
//! suggestions scoped to the packages in the transaction. Reads existing
//! scan and project data from the local SQLite cache — no network calls,
//! so it's fast enough for inline pacman output.

use std::collections::HashSet;
use std::path::Path;

use anyhow::Result;

use super::{Hook, HookContext};
use crate::contribute::suggest::{self, SuggestionKind};
use crate::report::terminal::normalize_url;
use crate::storage::Storage;

const PACMAN_DB_PATH: &str = "/var/lib/pacman/local";

/// Maximum number of suggestions to show in hook output.
const MAX_HOOK_SUGGESTIONS: usize = 3;

/// Post-transaction hook for pacman (Arch Linux).
///
/// After pacman installs or upgrades packages, this hook uses the
/// contribution suggestion engine to show a brief list of ways the user
/// can support the affected projects. It never makes network calls and
/// returns `Ok(())` silently when there is no data to show.
pub struct PacmanPostTransactionHook;

impl Hook for PacmanPostTransactionHook {
    fn name(&self) -> &str {
        "pacman-post-transaction"
    }

    fn description(&self) -> &str {
        "Show contribution suggestions after pacman transactions"
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
        let mut matched_urls: HashSet<String> = HashSet::new();
        for target in &ctx.targets {
            for pkg in &scan.packages {
                if pkg.name == *target {
                    if let Some(url) = &pkg.url {
                        matched_urls.insert(url.clone());
                        matched_urls.insert(normalize_url(url));
                    }
                    break;
                }
            }
        }

        if matched_urls.is_empty() {
            return Ok(());
        }

        // Load projects and filter to those matching the transaction packages
        let all_projects = storage.all_projects().unwrap_or_default();
        let scoped_projects: Vec<_> = all_projects
            .into_iter()
            .filter(|p| {
                let urls: Vec<&str> = [p.repo_url.as_deref(), p.homepage.as_deref()]
                    .into_iter()
                    .flatten()
                    .collect();
                urls.iter()
                    .any(|u| matched_urls.contains(*u) || matched_urls.contains(&normalize_url(u)))
            })
            .collect();

        if scoped_projects.is_empty() {
            return Ok(());
        }

        // Generate suggestions using the contribute engine, excluding
        // contributions the user has already completed
        let contributions = storage.get_contributions(None, None).unwrap_or_default();
        let suggestions =
            suggest::generate_suggestions(&scoped_projects, &contributions, SuggestionKind::ALL);

        if suggestions.is_empty() {
            return Ok(());
        }

        let selected = suggest::pick_random(suggestions, MAX_HOOK_SUGGESTIONS);
        eprint!("{}", suggest::format_hook_suggestions(&selected));

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
