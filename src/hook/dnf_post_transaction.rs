// SPDX-License-Identifier: GPL-3.0-or-later

//! DNF post-transaction hook.
//!
//! Surfaces contribution opportunities after DNF installs or upgrades
//! packages. Uses the contribution suggestion engine to generate actionable
//! suggestions from all enriched project data. No network calls, so it's
//! fast enough for inline DNF output.

use std::path::Path;

use anyhow::Result;

use super::{Hook, HookContext};
use crate::contribute::suggest::{self, SuggestionKind};
use crate::storage::Storage;

const DNF_DATA_PATH: &str = "/var/lib/dnf";

/// Maximum number of suggestions to show in hook output.
const MAX_HOOK_SUGGESTIONS: usize = 3;

/// Post-transaction hook for DNF (Fedora/RHEL).
///
/// After DNF installs or upgrades packages, this hook uses the
/// contribution suggestion engine to show a brief list of ways the user
/// can support the affected projects. It never makes network calls and
/// returns `Ok(())` silently when there is no data to show.
pub struct DnfPostTransactionHook;

impl Hook for DnfPostTransactionHook {
    fn name(&self) -> &str {
        "dnf-post-transaction"
    }

    fn description(&self) -> &str {
        "Show contribution suggestions after DNF transactions"
    }

    fn is_available(&self) -> bool {
        Path::new(DNF_DATA_PATH).is_dir()
    }

    fn run(&self, ctx: &HookContext) -> Result<()> {
        if ctx.targets.is_empty() {
            return Ok(());
        }

        let storage = match ctx.db_path {
            Some(ref path) => match Storage::open_path(path) {
                Ok(s) => s,
                Err(e) => {
                    if path.exists() {
                        eprintln!(
                            "warning: failed to open database at {}: {e}",
                            path.display()
                        );
                    }
                    return Ok(());
                }
            },
            None => match Storage::open() {
                Ok(s) => s,
                Err(_) => return Ok(()), // No database yet — nothing to report
            },
        };

        // Load all enriched projects from the database.
        let projects = match storage.all_projects() {
            Ok(p) => p,
            Err(e) => {
                eprintln!("warning: failed to load projects: {e}");
                return Ok(());
            }
        };
        if projects.is_empty() {
            return Ok(());
        }

        // Generate suggestions using the contribute engine, excluding
        // contributions the user has already completed.
        let contributions = match storage.get_contributions(None, None) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("warning: failed to load contributions: {e}");
                Vec::new()
            }
        };
        let suggestions =
            suggest::generate_suggestions(&projects, &contributions, SuggestionKind::ALL);

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
        let hook = DnfPostTransactionHook;
        assert_eq!(hook.name(), "dnf-post-transaction");
        assert!(!hook.description().is_empty());
    }

    #[test]
    fn run_with_empty_targets_succeeds() {
        let hook = DnfPostTransactionHook;
        let config = crate::config::Config::default();
        let ctx = HookContext {
            config: &config,
            targets: vec![],
            db_path: None,
        };
        // Should return Ok silently
        assert!(hook.run(&ctx).is_ok());
    }

    #[test]
    fn run_with_targets_no_db_succeeds() {
        // When there's no database, the hook should return Ok silently
        let hook = DnfPostTransactionHook;
        let config = crate::config::Config::default();
        let ctx = HookContext {
            config: &config,
            targets: vec!["curl".to_string()],
            db_path: None,
        };
        assert!(hook.run(&ctx).is_ok());
    }
}
