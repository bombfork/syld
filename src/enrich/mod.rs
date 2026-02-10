// SPDX-License-Identifier: GPL-3.0-or-later

//! Optional network-based enrichment of project metadata.
//!
//! This module is gated behind the `enrich` feature flag and requires
//! explicit opt-in from the user (via `--enrich` flag or config).
//!
//! Enrichment sources:
//! - GitHub FUNDING.yml
//! - GitLab funding metadata
//! - Open Collective API
//! - Liberapay API
