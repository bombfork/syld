// SPDX-License-Identifier: GPL-3.0-or-later

//! GitHub enrichment backend.
//!
//! Uses the `gh` CLI to fetch repository metadata and FUNDING.yml from GitHub.

use std::process::Command;

use anyhow::{Context, Result};
use serde::Deserialize;

use super::EnrichmentBackend;
use crate::contribute::github_good_first_issues::extract_github_owner_repo;
use crate::project::{FundingChannel, UpstreamProject};

pub struct GitHubBackend;

#[derive(Debug, Deserialize)]
struct GhRepo {
    #[serde(rename = "stargazerCount")]
    stargazer_count: Option<u64>,
    #[serde(rename = "homepageUrl")]
    homepage_url: Option<String>,
    #[serde(rename = "licenseInfo")]
    license_info: Option<GhLicense>,
    #[serde(rename = "hasIssuesEnabled")]
    has_issues_enabled: Option<bool>,
    url: Option<String>,
    #[allow(dead_code)]
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GhLicense {
    #[serde(rename = "spdxId")]
    spdx_id: Option<String>,
}

impl EnrichmentBackend for GitHubBackend {
    fn name(&self) -> &str {
        "github"
    }

    fn is_available(&self) -> bool {
        Command::new("gh")
            .args(["auth", "status"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn enrich(&self, project: &UpstreamProject) -> Result<UpstreamProject> {
        let repo_url = match &project.repo_url {
            Some(url) => url,
            None => return Ok(project.clone()),
        };

        let owner_repo = match extract_github_owner_repo(repo_url) {
            Some(or) => or,
            None => return Ok(project.clone()),
        };

        let mut enriched = project.clone();

        // Fetch repo metadata
        if let Ok(repo) = fetch_repo_metadata(&owner_repo) {
            if enriched.stars.is_none() {
                enriched.stars = repo.stargazer_count;
            }
            if enriched.homepage.is_none()
                && let Some(hp) = &repo.homepage_url
                && !hp.is_empty()
            {
                enriched.homepage = Some(hp.clone());
            }
            if let Some(license) = &repo.license_info
                && let Some(spdx) = &license.spdx_id
                && spdx != "NOASSERTION"
                && !enriched.licenses.iter().any(|l| l == spdx)
            {
                enriched.licenses.push(spdx.clone());
            }
            if let Some(html_url) = &repo.url {
                if enriched.bug_tracker.is_none() && repo.has_issues_enabled.unwrap_or(false) {
                    enriched.bug_tracker = Some(format!("{html_url}/issues"));
                }
                if enriched.good_first_issues_url.is_none() {
                    enriched.good_first_issues_url = Some(format!(
                        "{html_url}/issues?q=is%3Aissue+is%3Aopen+label%3A%22good+first+issue%22"
                    ));
                }
                if enriched.contributing_url.is_none() {
                    enriched.contributing_url =
                        Some(format!("{html_url}/blob/HEAD/CONTRIBUTING.md"));
                }
            }
        }

        // Fetch FUNDING.yml
        if let Ok(channels) = fetch_funding_yml(&owner_repo) {
            for channel in channels {
                if !enriched.funding.iter().any(|f| f.url == channel.url) {
                    enriched.funding.push(channel);
                }
            }
        }

        Ok(enriched)
    }
}

fn fetch_repo_metadata(owner_repo: &str) -> Result<GhRepo> {
    let output = Command::new("gh")
        .args([
            "api",
            &format!("repos/{owner_repo}"),
            "--jq",
            ".",
            "--cache",
            "1h",
        ])
        .output()
        .context("Failed to run gh api")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh api failed for {owner_repo}: {stderr}");
    }

    // gh api returns REST JSON; map to our struct
    let raw: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("Failed to parse gh api JSON")?;

    let repo = GhRepo {
        stargazer_count: raw.get("stargazers_count").and_then(|v| v.as_u64()),
        homepage_url: raw
            .get("homepage")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        license_info: raw.get("license").and_then(|v| {
            v.get("spdx_id")
                .and_then(|s| s.as_str())
                .map(|spdx| GhLicense {
                    spdx_id: Some(spdx.to_string()),
                })
        }),
        has_issues_enabled: raw.get("has_issues").and_then(|v| v.as_bool()),
        url: raw
            .get("html_url")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        description: raw
            .get("description")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    };

    Ok(repo)
}

fn fetch_funding_yml(owner_repo: &str) -> Result<Vec<FundingChannel>> {
    let output = Command::new("gh")
        .args([
            "api",
            &format!("repos/{owner_repo}/contents/.github/FUNDING.yml"),
            "--jq",
            ".content",
            "--cache",
            "1h",
        ])
        .output()
        .context("Failed to run gh api for FUNDING.yml")?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8(output.stdout).unwrap_or_default();
    let content = decode_base64_content(&stdout);

    Ok(parse_funding_yml(&content))
}

/// Decode base64 content from GitHub API (may contain newlines within the encoding).
fn decode_base64_content(encoded: &str) -> String {
    // GitHub returns base64 with newlines embedded; strip them and decode
    let clean: String = encoded.chars().filter(|c| !c.is_whitespace()).collect();

    // Simple base64 decode without pulling in a dependency
    base64_decode(&clean).unwrap_or_default()
}

fn base64_decode(input: &str) -> Option<String> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut buf = Vec::new();
    let bytes: Vec<u8> = input
        .bytes()
        .filter(|&b| b != b'=' && b != b'\n' && b != b'\r')
        .collect();

    let lookup = |b: u8| -> Option<u8> { TABLE.iter().position(|&c| c == b).map(|p| p as u8) };

    let mut i = 0;
    while i < bytes.len() {
        let b0 = lookup(bytes[i])?;
        let b1 = if i + 1 < bytes.len() {
            lookup(bytes[i + 1])?
        } else {
            0
        };
        let b2 = if i + 2 < bytes.len() {
            lookup(bytes[i + 2])?
        } else {
            0
        };
        let b3 = if i + 3 < bytes.len() {
            lookup(bytes[i + 3])?
        } else {
            0
        };

        let triple = ((b0 as u32) << 18) | ((b1 as u32) << 12) | ((b2 as u32) << 6) | (b3 as u32);

        buf.push((triple >> 16) as u8);
        if i + 2 < bytes.len() {
            buf.push((triple >> 8 & 0xFF) as u8);
        }
        if i + 3 < bytes.len() {
            buf.push((triple & 0xFF) as u8);
        }

        i += 4;
    }

    String::from_utf8(buf).ok()
}

/// Parse a FUNDING.yml file line-by-line (simple key: value format, no full YAML).
///
/// Recognizes common funding platforms:
/// - `github: username` or `github: [user1, user2]`
/// - `open_collective: slug`
/// - `ko_fi: username`
/// - `patreon: username`
/// - `liberapay: username`
/// - `custom: [url1, url2]` or `custom: url`
fn parse_funding_yml(content: &str) -> Vec<FundingChannel> {
    let mut channels = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let Some((key, value)) = line.split_once(':') else {
            continue;
        };

        let key = key.trim().to_lowercase();
        let value = value.trim();

        if value.is_empty() {
            continue;
        }

        match key.as_str() {
            "github" => {
                for name in parse_yaml_value(value) {
                    if !name.is_empty() {
                        channels.push(FundingChannel {
                            platform: "GitHub Sponsors".to_string(),
                            url: format!("https://github.com/sponsors/{name}"),
                        });
                    }
                }
            }
            "open_collective" => {
                for slug in parse_yaml_value(value) {
                    if !slug.is_empty() {
                        channels.push(FundingChannel {
                            platform: "Open Collective".to_string(),
                            url: format!("https://opencollective.com/{slug}"),
                        });
                    }
                }
            }
            "ko_fi" => {
                for name in parse_yaml_value(value) {
                    if !name.is_empty() {
                        channels.push(FundingChannel {
                            platform: "Ko-fi".to_string(),
                            url: format!("https://ko-fi.com/{name}"),
                        });
                    }
                }
            }
            "patreon" => {
                for name in parse_yaml_value(value) {
                    if !name.is_empty() {
                        channels.push(FundingChannel {
                            platform: "Patreon".to_string(),
                            url: format!("https://www.patreon.com/{name}"),
                        });
                    }
                }
            }
            "liberapay" => {
                for name in parse_yaml_value(value) {
                    if !name.is_empty() {
                        channels.push(FundingChannel {
                            platform: "Liberapay".to_string(),
                            url: format!("https://liberapay.com/{name}"),
                        });
                    }
                }
            }
            "community_bridge" => {
                for name in parse_yaml_value(value) {
                    if !name.is_empty() {
                        channels.push(FundingChannel {
                            platform: "Community Bridge".to_string(),
                            url: format!("https://funding.communitybridge.org/projects/{name}"),
                        });
                    }
                }
            }
            "issuehunt" => {
                for name in parse_yaml_value(value) {
                    if !name.is_empty() {
                        channels.push(FundingChannel {
                            platform: "IssueHunt".to_string(),
                            url: format!("https://issuehunt.io/r/{name}"),
                        });
                    }
                }
            }
            "polar" => {
                for name in parse_yaml_value(value) {
                    if !name.is_empty() {
                        channels.push(FundingChannel {
                            platform: "Polar".to_string(),
                            url: format!("https://polar.sh/{name}"),
                        });
                    }
                }
            }
            "buy_me_a_coffee" => {
                for name in parse_yaml_value(value) {
                    if !name.is_empty() {
                        channels.push(FundingChannel {
                            platform: "Buy Me a Coffee".to_string(),
                            url: format!("https://buymeacoffee.com/{name}"),
                        });
                    }
                }
            }
            "thanks_dev" => {
                for name in parse_yaml_value(value) {
                    if !name.is_empty() {
                        channels.push(FundingChannel {
                            platform: "thanks.dev".to_string(),
                            url: format!("https://thanks.dev/d/gh/{name}"),
                        });
                    }
                }
            }
            "custom" => {
                for url in parse_yaml_value(value) {
                    if !url.is_empty() {
                        channels.push(FundingChannel {
                            platform: "Custom".to_string(),
                            url: url.trim_matches('"').trim_matches('\'').to_string(),
                        });
                    }
                }
            }
            _ => {}
        }
    }

    channels
}

/// Parse a YAML value that might be a scalar or an inline array `[a, b, c]`.
fn parse_yaml_value(value: &str) -> Vec<String> {
    let value = value.trim();

    if value.starts_with('[') && value.ends_with(']') {
        let inner = &value[1..value.len() - 1];
        inner
            .split(',')
            .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
            .filter(|s| !s.is_empty())
            .collect()
    } else {
        vec![value.trim_matches('"').trim_matches('\'').to_string()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_funding_yml_github_single() {
        let content = "github: octocat\n";
        let channels = parse_funding_yml(content);
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].platform, "GitHub Sponsors");
        assert_eq!(channels[0].url, "https://github.com/sponsors/octocat");
    }

    #[test]
    fn parse_funding_yml_github_array() {
        let content = "github: [octocat, surftocat]\n";
        let channels = parse_funding_yml(content);
        assert_eq!(channels.len(), 2);
        assert_eq!(channels[0].url, "https://github.com/sponsors/octocat");
        assert_eq!(channels[1].url, "https://github.com/sponsors/surftocat");
    }

    #[test]
    fn parse_funding_yml_multiple_platforms() {
        let content = "\
github: octocat
open_collective: my-project
ko_fi: myname
patreon: creator
liberapay: dev
custom: https://example.com/donate
";
        let channels = parse_funding_yml(content);
        assert_eq!(channels.len(), 6);
        assert_eq!(channels[0].platform, "GitHub Sponsors");
        assert_eq!(channels[1].platform, "Open Collective");
        assert_eq!(channels[1].url, "https://opencollective.com/my-project");
        assert_eq!(channels[2].platform, "Ko-fi");
        assert_eq!(channels[3].platform, "Patreon");
        assert_eq!(channels[4].platform, "Liberapay");
        assert_eq!(channels[5].platform, "Custom");
        assert_eq!(channels[5].url, "https://example.com/donate");
    }

    #[test]
    fn parse_funding_yml_comments_and_blanks() {
        let content = "\
# This is a comment
github: octocat

# Another comment
";
        let channels = parse_funding_yml(content);
        assert_eq!(channels.len(), 1);
    }

    #[test]
    fn parse_funding_yml_empty() {
        let channels = parse_funding_yml("");
        assert!(channels.is_empty());
    }

    #[test]
    fn parse_funding_yml_custom_array() {
        let content = "custom: [\"https://a.com\", \"https://b.com\"]\n";
        let channels = parse_funding_yml(content);
        assert_eq!(channels.len(), 2);
        assert_eq!(channels[0].url, "https://a.com");
        assert_eq!(channels[1].url, "https://b.com");
    }

    #[test]
    fn parse_yaml_value_scalar() {
        assert_eq!(parse_yaml_value("hello"), vec!["hello"]);
    }

    #[test]
    fn parse_yaml_value_array() {
        assert_eq!(parse_yaml_value("[a, b, c]"), vec!["a", "b", "c"]);
    }

    #[test]
    fn parse_yaml_value_quoted() {
        assert_eq!(parse_yaml_value("\"hello\""), vec!["hello"]);
    }

    #[test]
    fn base64_decode_simple() {
        // "hello" in base64 is "aGVsbG8="
        assert_eq!(base64_decode("aGVsbG8=").unwrap(), "hello");
    }

    #[test]
    fn base64_decode_with_newlines() {
        assert_eq!(base64_decode("aGVs\nbG8=").unwrap(), "hello");
    }

    #[test]
    fn base64_roundtrip_funding() {
        // "github: octocat\n" base64 encoded
        let encoded = "Z2l0aHViOiBvY3RvY2F0Cg==";
        let decoded = base64_decode(encoded).unwrap();
        assert_eq!(decoded, "github: octocat\n");

        let channels = parse_funding_yml(&decoded);
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].platform, "GitHub Sponsors");
    }
}
