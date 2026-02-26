// SPDX-License-Identifier: GPL-3.0-or-later

//! APT post-invoke hook.
//!
//! Surfaces contribution opportunities after APT installs or upgrades
//! packages. Uses the contribution suggestion engine to generate actionable
//! suggestions from all enriched project data. No network calls, so it's
//! fast enough for inline APT output.

use std::path::Path;

use anyhow::Result;

use super::{Hook, HookContext};
use crate::contribute::suggest::{self, SuggestionKind};
use crate::storage::Storage;

const DPKG_STATUS_PATH: &str = "/var/lib/dpkg/status";

/// Maximum number of suggestions to show in hook output.
const MAX_HOOK_SUGGESTIONS: usize = 3;

/// Post-invoke hook for APT (Debian/Ubuntu).
///
/// After APT installs or upgrades packages, this hook uses the
/// contribution suggestion engine to show a brief list of ways the user
/// can support the affected projects. It never makes network calls and
/// returns `Ok(())` silently when there is no data to show.
pub struct AptPostInvokeHook;

impl Hook for AptPostInvokeHook {
    fn name(&self) -> &str {
        "apt-post-invoke"
    }

    fn description(&self) -> &str {
        "Show contribution suggestions after APT transactions"
    }

    fn is_available(&self) -> bool {
        Path::new(DPKG_STATUS_PATH).is_file()
    }

    fn run(&self, ctx: &HookContext) -> Result<()> {
        if ctx.targets.is_empty() {
            return Ok(());
        }

        let storage = match ctx.db_path {
            Some(ref path) => match Storage::open_path(path) {
                Ok(s) => s,
                Err(_) => return Ok(()),
            },
            None => match Storage::open() {
                Ok(s) => s,
                Err(_) => return Ok(()), // No database yet — nothing to report
            },
        };

        // Load all enriched projects from the database.
        let projects = storage.all_projects().unwrap_or_default();
        if projects.is_empty() {
            return Ok(());
        }

        // Generate suggestions using the contribute engine, excluding
        // contributions the user has already completed.
        let contributions = storage.get_contributions(None, None).unwrap_or_default();
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
        let hook = AptPostInvokeHook;
        assert_eq!(hook.name(), "apt-post-invoke");
        assert!(!hook.description().is_empty());
    }

    #[test]
    fn run_with_empty_targets_succeeds() {
        let hook = AptPostInvokeHook;
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
        let hook = AptPostInvokeHook;
        let config = crate::config::Config::default();
        let ctx = HookContext {
            config: &config,
            targets: vec!["curl".to_string()],
            db_path: None,
        };
        assert!(hook.run(&ctx).is_ok());
    }
}
