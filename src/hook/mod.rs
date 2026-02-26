// SPDX-License-Identifier: GPL-3.0-or-later

//! Package manager hook system.
//!
//! This module provides a pluggable framework for running actions in response
//! to package manager events (installs, upgrades, removals). Each hook is
//! represented by a *backend* that implements the [`Hook`] trait. At runtime
//! the application calls [`all_hooks()`] to obtain the full list of known
//! hooks, or [`active_hooks()`] for only those available on the current system.
//!
//! # Adding a new hook
//!
//! Follow these steps to add a new hook backend. For a complete reference
//! implementation, see
//! [`pacman_post_transaction::PacmanPostTransactionHook`].
//!
//! ## 1. Create a module file
//!
//! Add a new file under `src/hook/` (e.g. `apt_post_invoke.rs`) and declare
//! it in this module with `pub mod apt_post_invoke;`. Define a public unit
//! struct to represent the hook:
//!
//! ```rust,ignore
//! pub struct AptPostInvokeHook;
//! ```
//!
//! ## 2. Implement [`Hook`]
//!
//! The trait has four required methods:
//!
//! - **[`name()`](Hook::name)** — Return a stable, kebab-case identifier
//!   (e.g. `"apt-post-invoke"`). This string is used as the CLI argument to
//!   `syld hook run <name>` and must not change between releases.
//!
//! - **[`description()`](Hook::description)** — Return a short, human-readable
//!   description of what this hook does (displayed in `syld hook list`).
//!
//! - **[`is_available()`](Hook::is_available)** — Return `true` if the hook
//!   can operate in the current environment. This is called to filter hooks,
//!   so it must be **cheap and fast**. Typical checks include verifying that a
//!   well-known path exists (e.g. `/var/lib/pacman`).
//!
//! - **[`run()`](Hook::run)** — Execute the hook logic. The [`HookContext`]
//!   provides the configuration and a list of target package names read from
//!   stdin. Hooks should print to stderr (not stdout) and must never fail
//!   noisily in package manager output — return `Ok(())` when there is nothing
//!   to report.
//!
//! ## 3. Register the hook
//!
//! In [`all_hooks()`], append a `Box::new(YourHook)` entry to the `candidates`
//! vector. The new hook will be included automatically whenever its
//! [`is_available()`](Hook::is_available) check passes.

pub mod apt_post_invoke;
pub mod pacman_post_transaction;

use std::path::PathBuf;

use anyhow::Result;

use crate::config::Config;

/// Context passed to a hook at execution time.
pub struct HookContext<'a> {
    /// Application configuration.
    pub config: &'a Config,

    /// Package names provided via stdin (one per line).
    pub targets: Vec<String>,

    /// Optional override for the database path.
    ///
    /// When set, hooks should use this path instead of the default
    /// `Storage::open()` resolution. This is needed when hooks run as root
    /// (e.g. pacman ALPM hooks), where `$HOME` resolves to `/root` instead
    /// of the installing user's home directory.
    pub db_path: Option<PathBuf>,
}

/// Trait for package manager hook backends.
///
/// Each implementation represents a hook that runs in response to a package
/// manager event (e.g. pacman post-transaction). The lifecycle is:
///
/// 1. The hook is instantiated unconditionally.
/// 2. [`Hook::is_available()`] is called to check whether the hook can operate.
/// 3. If available, [`Hook::run()`] is called with a [`HookContext`] containing
///    the target package names.
pub trait Hook {
    /// A stable, kebab-case identifier for this hook.
    ///
    /// Used as the CLI argument to `syld hook run <name>`. Must not change
    /// between releases.
    fn name(&self) -> &str;

    /// A short, human-readable description of this hook.
    fn description(&self) -> &str;

    /// Returns `true` if this hook can operate in the current environment.
    ///
    /// This method is called to filter the set of active hooks. It should be
    /// **cheap and fast** — e.g. checking whether a well-known path exists.
    fn is_available(&self) -> bool;

    /// Execute the hook.
    ///
    /// Hooks should print to stderr and return `Ok(())` silently when there is
    /// nothing to report. Hooks must never fail noisily in package manager
    /// output.
    fn run(&self, ctx: &HookContext) -> Result<()>;
}

/// Returns all known hooks, regardless of availability.
///
/// Used by `syld hook list` to show all hooks with their status, and by
/// `find_hook()` to look up a hook by name.
pub fn all_hooks() -> Vec<Box<dyn Hook>> {
    vec![
        Box::new(apt_post_invoke::AptPostInvokeHook),
        Box::new(pacman_post_transaction::PacmanPostTransactionHook),
    ]
}

/// Returns all hooks that are available in the current environment.
///
/// Every known hook is instantiated and then filtered through
/// [`Hook::is_available()`]. Only hooks whose package manager is actually
/// present are returned.
pub fn active_hooks(_config: &Config) -> Vec<Box<dyn Hook>> {
    all_hooks()
        .into_iter()
        .filter(|h| h.is_available())
        .collect()
}

/// Look up a hook by name, without filtering by availability.
///
/// Returns `None` if no hook with the given name is registered. This is used
/// by the CLI to provide clear error messages (distinguishing "unknown hook"
/// from "hook not available on this system").
pub fn find_hook(name: &str) -> Option<Box<dyn Hook>> {
    all_hooks().into_iter().find(|h| h.name() == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockHook {
        available: bool,
    }

    impl Hook for MockHook {
        fn name(&self) -> &str {
            "mock"
        }

        fn description(&self) -> &str {
            "A mock hook for testing"
        }

        fn is_available(&self) -> bool {
            self.available
        }

        fn run(&self, _ctx: &HookContext) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn mock_hook_trait_object() {
        let hook: Box<dyn Hook> = Box::new(MockHook { available: true });
        assert_eq!(hook.name(), "mock");
        assert_eq!(hook.description(), "A mock hook for testing");
        assert!(hook.is_available());
    }

    #[test]
    fn unavailable_hook_filtered() {
        let hooks: Vec<Box<dyn Hook>> = vec![
            Box::new(MockHook { available: true }),
            Box::new(MockHook { available: false }),
            Box::new(MockHook { available: true }),
        ];

        let active: Vec<_> = hooks.into_iter().filter(|h| h.is_available()).collect();
        assert_eq!(active.len(), 2);
    }

    #[test]
    fn all_hooks_contains_pacman() {
        let hooks = all_hooks();
        assert!(hooks.iter().any(|h| h.name() == "pacman-post-transaction"));
    }

    #[test]
    fn all_hooks_contains_apt() {
        let hooks = all_hooks();
        assert!(hooks.iter().any(|h| h.name() == "apt-post-invoke"));
    }

    #[test]
    fn find_hook_returns_known() {
        let hook = find_hook("pacman-post-transaction");
        assert!(hook.is_some());
        assert_eq!(hook.unwrap().name(), "pacman-post-transaction");
    }

    #[test]
    fn find_hook_returns_none_for_unknown() {
        assert!(find_hook("nonexistent-hook").is_none());
    }

    #[test]
    fn active_hooks_does_not_panic() {
        let config = Config::default();
        let _ = active_hooks(&config);
    }
}
