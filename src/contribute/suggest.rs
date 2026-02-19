// SPDX-License-Identifier: GPL-3.0-or-later

//! Contribution suggestion engine for the `syld contribute` command.
//!
//! This module defines the types used to generate actionable suggestions
//! from scan and enrichment data.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// The category of a contribution suggestion.
///
/// These map to the `--type` flag values on the `contribute` command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SuggestionKind {
    /// Star the project on GitHub.
    Star,
    /// Check good first issues.
    Issue,
    /// Donate via a funding channel.
    Donate,
    /// Improve project documentation.
    Docs,
    /// Share a lesser-known project.
    Spread,
}

impl SuggestionKind {
    /// All known suggestion kinds.
    pub const ALL: &[SuggestionKind] = &[
        SuggestionKind::Star,
        SuggestionKind::Issue,
        SuggestionKind::Donate,
        SuggestionKind::Docs,
        SuggestionKind::Spread,
    ];

    /// The emoji used when displaying this suggestion kind.
    pub fn emoji(self) -> &'static str {
        match self {
            SuggestionKind::Star => "\u{2b50}",
            SuggestionKind::Issue => "\u{1f41b}",
            SuggestionKind::Donate => "\u{1f4b0}",
            SuggestionKind::Docs => "\u{1f4d6}",
            SuggestionKind::Spread => "\u{1f4e3}",
        }
    }
}

impl fmt::Display for SuggestionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SuggestionKind::Star => write!(f, "star"),
            SuggestionKind::Issue => write!(f, "issue"),
            SuggestionKind::Donate => write!(f, "donate"),
            SuggestionKind::Docs => write!(f, "docs"),
            SuggestionKind::Spread => write!(f, "spread"),
        }
    }
}

impl FromStr for SuggestionKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "star" => Ok(SuggestionKind::Star),
            "issue" => Ok(SuggestionKind::Issue),
            "donate" => Ok(SuggestionKind::Donate),
            "docs" => Ok(SuggestionKind::Docs),
            "spread" => Ok(SuggestionKind::Spread),
            _ => Err(format!(
                "unknown suggestion type '{s}'. Valid types: star, issue, donate, docs, spread"
            )),
        }
    }
}

/// A concrete, actionable contribution suggestion shown to the user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContributionSuggestion {
    /// What category of contribution this is.
    pub kind: SuggestionKind,

    /// Human-readable action description (e.g. "Star curl/curl on GitHub").
    pub title: String,

    /// URL the user can visit to act on this suggestion.
    pub url: String,
}

/// Parse a comma-separated list of suggestion types.
///
/// Returns an error message if any type is unrecognised.
pub fn parse_types(input: &str) -> Result<Vec<SuggestionKind>, String> {
    input.split(',').map(|s| s.trim().parse()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggestion_kind_display_roundtrip() {
        for kind in SuggestionKind::ALL {
            let s = kind.to_string();
            let parsed: SuggestionKind = s.parse().unwrap();
            assert_eq!(*kind, parsed);
        }
    }

    #[test]
    fn suggestion_kind_from_str_unknown() {
        assert!("unknown".parse::<SuggestionKind>().is_err());
    }

    #[test]
    fn parse_types_single() {
        let types = parse_types("star").unwrap();
        assert_eq!(types, vec![SuggestionKind::Star]);
    }

    #[test]
    fn parse_types_multiple() {
        let types = parse_types("star,issue,donate").unwrap();
        assert_eq!(
            types,
            vec![
                SuggestionKind::Star,
                SuggestionKind::Issue,
                SuggestionKind::Donate,
            ]
        );
    }

    #[test]
    fn parse_types_with_whitespace() {
        let types = parse_types("star , docs").unwrap();
        assert_eq!(types, vec![SuggestionKind::Star, SuggestionKind::Docs]);
    }

    #[test]
    fn parse_types_invalid() {
        assert!(parse_types("star,bogus").is_err());
    }

    #[test]
    fn suggestion_kind_emoji() {
        // Just verify each kind has a non-empty emoji.
        for kind in SuggestionKind::ALL {
            assert!(!kind.emoji().is_empty());
        }
    }

    #[test]
    fn suggestion_serde_roundtrip() {
        let suggestion = ContributionSuggestion {
            kind: SuggestionKind::Star,
            title: "Star curl/curl on GitHub".to_string(),
            url: "https://github.com/curl/curl".to_string(),
        };
        let json = serde_json::to_string(&suggestion).unwrap();
        let parsed: ContributionSuggestion = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.kind, SuggestionKind::Star);
        assert_eq!(parsed.title, "Star curl/curl on GitHub");
        assert_eq!(parsed.url, "https://github.com/curl/curl");
    }
}
