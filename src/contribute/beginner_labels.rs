// SPDX-License-Identifier: GPL-3.0-or-later

//! Label detection and fallback logic for beginner-friendly issues.
//!
//! This module provides utilities for discovering and selecting beginner-friendly
//! issue labels in GitHub repositories. It supports:
//!
//! - Default label lists for common platforms
//! - Auto-detection of beginner-friendly patterns in repository labels
//! - Fallback logic to try multiple labels in priority order

use std::process::Command;

use anyhow::{Context, Result};
use serde::Deserialize;

/// A GitHub repository label.
#[derive(Debug, Deserialize, Clone)]
pub struct RepositoryLabel {
    pub name: String,
}

/// Default labels to try for discovering beginner-friendly issues.
/// These are ordered by priority (most commonly used first).
pub const DEFAULT_BEGINNER_LABELS: &[&str] = &[
    "good first issue",
    "help wanted",
    "good first bug",
    "beginner friendly",
    "beginner-friendly",
    "difficulty/low",
    "effort/low",
];

/// Auto-detect beginner-friendly labels in a repository.
///
/// Queries the GitHub API for all labels in the repository and returns
/// those matching beginner-friendly patterns. Patterns include:
/// - Contains "good" AND "first"
/// - Contains "beginner"
/// - Contains "help" AND "wanted"
/// - Ends with "/low" (for scoped labels like "difficulty/low")
///
/// # Errors
///
/// Returns an error if the `gh` CLI fails or returns invalid JSON.
pub fn detect_beginner_labels(owner_repo: &str) -> Result<Vec<String>> {
    let output = Command::new("gh")
        .args([
            "label", "list", "--repo", owner_repo, "--limit", "100", "--json", "name",
        ])
        .output()
        .context("Failed to run gh label list")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Some repos may have issues disabled or be inaccessible — not fatal.
        if stderr.contains("Could not resolve")
            || stderr.contains("not found")
            || stderr.contains("403")
        {
            return Ok(Vec::new());
        }
        anyhow::bail!("gh label list failed for {owner_repo}: {stderr}");
    }

    let stdout =
        String::from_utf8(output.stdout).context("gh label list output is not valid UTF-8")?;

    let labels: Vec<RepositoryLabel> =
        serde_json::from_str(&stdout).context("Failed to parse gh label list JSON")?;

    let mut matching = Vec::new();
    for label in labels {
        if is_beginner_friendly(&label.name) {
            matching.push(label.name);
        }
    }

    Ok(matching)
}

/// Check if a label name matches beginner-friendly patterns.
///
/// Uses case-insensitive substring matching for the following patterns:
/// - Contains both "good" and "first"
/// - Contains "beginner"
/// - Contains both "help" and "wanted"
/// - Ends with "/low" (for scoped labels)
fn is_beginner_friendly(label_name: &str) -> bool {
    let lower = label_name.to_lowercase();

    // Contains "good" AND "first"
    if lower.contains("good") && lower.contains("first") {
        return true;
    }

    // Contains "beginner"
    if lower.contains("beginner") {
        return true;
    }

    // Contains "help" AND "wanted"
    if lower.contains("help") && lower.contains("wanted") {
        return true;
    }

    // Ends with "/low" (for scoped labels)
    if lower.ends_with("/low") {
        return true;
    }

    false
}

/// Select a beginner-friendly label for querying issues.
///
/// Attempts to use labels in priority order, falling back through the list
/// if a label doesn't exist in the repository. If provided labels are not
/// available, falls back to defaults.
///
/// Returns the first label that exists in the repository, or a default
/// if none of the provided labels are found.
pub fn select_label(owner_repo: &str, preferred_labels: Option<&[String]>) -> Result<String> {
    // Determine which labels to try
    let to_try: Vec<&str> = if let Some(labels) = preferred_labels {
        labels.iter().map(|s| s.as_str()).collect()
    } else {
        DEFAULT_BEGINNER_LABELS.to_vec()
    };

    // Try each label in order
    for label in &to_try {
        if label_exists(owner_repo, label)? {
            return Ok(label.to_string());
        }
    }

    // If none of the preferred labels exist, try auto-detection
    let detected = detect_beginner_labels(owner_repo)?;
    if !detected.is_empty() {
        return Ok(detected[0].clone());
    }

    // Fall back to the first default label (even if it doesn't exist)
    Ok(DEFAULT_BEGINNER_LABELS[0].to_string())
}

/// Check if a specific label exists in a repository.
///
/// Uses a lightweight `gh` query to check for the label's existence.
/// Returns `false` if the label is not found or if the repository
/// is inaccessible.
fn label_exists(owner_repo: &str, label: &str) -> Result<bool> {
    let output = Command::new("gh")
        .args([
            "label", "list", "--repo", owner_repo, "--search", label, "--limit", "1", "--json",
            "name",
        ])
        .output()
        .context("Failed to run gh label list")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Repository not accessible — treat as false
        if stderr.contains("Could not resolve")
            || stderr.contains("not found")
            || stderr.contains("403")
        {
            return Ok(false);
        }
        // Other errors are still fatal
        anyhow::bail!("gh label list failed: {stderr}");
    }

    let stdout =
        String::from_utf8(output.stdout).context("gh label list output is not valid UTF-8")?;

    // If the JSON array is non-empty, the label exists
    Ok(stdout.trim() != "[]")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_beginner_friendly_good_first_issue() {
        assert!(is_beginner_friendly("good first issue"));
    }

    #[test]
    fn is_beginner_friendly_good_first_bug() {
        assert!(is_beginner_friendly("good first bug"));
    }

    #[test]
    fn is_beginner_friendly_beginner_friendly() {
        assert!(is_beginner_friendly("beginner friendly"));
    }

    #[test]
    fn is_beginner_friendly_beginner_hyphen() {
        assert!(is_beginner_friendly("beginner-friendly"));
    }

    #[test]
    fn is_beginner_friendly_help_wanted() {
        assert!(is_beginner_friendly("help wanted"));
    }

    #[test]
    fn is_beginner_friendly_difficulty_low() {
        assert!(is_beginner_friendly("difficulty/low"));
    }

    #[test]
    fn is_beginner_friendly_effort_low() {
        assert!(is_beginner_friendly("effort/low"));
    }

    #[test]
    fn is_beginner_friendly_case_insensitive() {
        assert!(is_beginner_friendly("Good First Issue"));
        assert!(is_beginner_friendly("BEGINNER FRIENDLY"));
        assert!(is_beginner_friendly("Help Wanted"));
    }

    #[test]
    fn is_beginner_friendly_false_for_unrelated() {
        assert!(!is_beginner_friendly("bug"));
        assert!(!is_beginner_friendly("feature request"));
        assert!(!is_beginner_friendly("documentation"));
    }

    #[test]
    fn is_beginner_friendly_false_partial_match() {
        assert!(!is_beginner_friendly("good")); // needs "first" too
        assert!(!is_beginner_friendly("first")); // needs "good" too
        assert!(!is_beginner_friendly("help")); // needs "wanted" too
    }

    #[test]
    fn default_labels_not_empty() {
        assert!(!DEFAULT_BEGINNER_LABELS.is_empty());
    }

    #[test]
    fn default_labels_in_priority_order() {
        // Verify the most common labels appear first
        assert_eq!(DEFAULT_BEGINNER_LABELS[0], "good first issue");
    }

    #[test]
    fn repo_label_deserialize() {
        let json = r#"{"name":"good first issue"}"#;
        let label: RepositoryLabel = serde_json::from_str(json).unwrap();
        assert_eq!(label.name, "good first issue");
    }

    #[test]
    fn repo_labels_list_deserialize() {
        let json = r#"[
            {"name":"good first issue"},
            {"name":"help wanted"},
            {"name":"bug"}
        ]"#;
        let labels: Vec<RepositoryLabel> = serde_json::from_str(json).unwrap();
        assert_eq!(labels.len(), 3);
        assert_eq!(labels[0].name, "good first issue");
    }
}
