// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashMap;
use std::process::Command;

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use serde::Deserialize;

use super::{Discoverer, InstalledPackage, PackageSource};

/// Discovers tools installed via mise (dev tool version manager).
///
/// Runs `mise ls --json` to enumerate all installed tools and their versions.
/// mise manages tools like rust, node, python, go, and cargo/npm-installed CLIs.
pub struct MiseDiscoverer;

/// A single tool version entry from `mise ls --json`.
#[derive(Debug, Deserialize)]
struct MiseToolEntry {
    version: String,
    #[allow(dead_code)]
    install_path: String,
    source: Option<MiseSource>,
}

/// The source configuration that requested a tool version.
#[derive(Debug, Deserialize)]
struct MiseSource {
    #[serde(rename = "type")]
    source_type: Option<String>,
    path: Option<String>,
}

impl Discoverer for MiseDiscoverer {
    fn name(&self) -> &str {
        "mise"
    }

    fn is_available(&self) -> bool {
        which_mise().is_some()
    }

    fn discover(&self) -> Result<Vec<InstalledPackage>> {
        let output = Command::new("mise")
            .args(["ls", "--json"])
            .output()
            .context("Failed to run mise ls --json")?;

        if !output.status.success() {
            anyhow::bail!(
                "mise ls --json failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let stdout =
            String::from_utf8(output.stdout).context("mise ls --json output is not valid UTF-8")?;

        parse_mise_output(&stdout)
    }
}

/// Check common paths for the mise binary.
fn which_mise() -> Option<&'static str> {
    use std::path::Path;

    // mise may be installed via package manager or cargo
    let candidates = ["/usr/bin/mise", "/usr/local/bin/mise"];

    for path in &candidates {
        if Path::new(path).is_file() {
            return Some(path);
        }
    }

    // Also check ~/.local/bin/mise (common user install)
    if let Some(home) = std::env::var_os("HOME") {
        let user_path = std::path::PathBuf::from(home).join(".local/bin/mise");
        if user_path.is_file() {
            // Return a leaked string to keep the &'static lifetime.
            // This runs at most once at startup so the leak is negligible.
            return Some(Box::leak(
                user_path.to_string_lossy().into_owned().into_boxed_str(),
            ));
        }
    }

    None
}

/// Parse the JSON output of `mise ls --json`.
///
/// The format is a map of tool name to array of version entries:
/// ```json
/// {
///   "node": [{ "version": "20.0.0", "install_path": "...", "source": { ... } }],
///   "python": [...]
/// }
/// ```
fn parse_mise_output(output: &str) -> Result<Vec<InstalledPackage>> {
    if output.trim().is_empty() {
        return Ok(Vec::new());
    }

    let tools: HashMap<String, Vec<MiseToolEntry>> =
        serde_json::from_str(output).context("Failed to parse mise ls --json output")?;

    let total: usize = tools.values().map(|v| v.len()).sum();

    let pb = ProgressBar::new(total as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("  {bar:30} {pos}/{len} packages")
            .unwrap(),
    );

    let mut packages = Vec::new();

    for (tool_name, versions) in &tools {
        for entry in versions {
            let description = build_description(tool_name, &entry.source);

            packages.push(InstalledPackage {
                name: tool_name.clone(),
                version: entry.version.clone(),
                description,
                url: None,
                source: PackageSource::Mise,
                licenses: Vec::new(),
            });
            pb.inc(1);
        }
    }

    pb.finish_and_clear();

    // Sort for deterministic output
    packages.sort_by(|a, b| a.name.cmp(&b.name).then(a.version.cmp(&b.version)));

    Ok(packages)
}

/// Build a description string from the tool name and its source config.
fn build_description(tool_name: &str, source: &Option<MiseSource>) -> Option<String> {
    match source {
        Some(s) => {
            let source_info = match (&s.source_type, &s.path) {
                (Some(t), Some(p)) => format!("{tool_name} (from {t}: {p})"),
                (Some(t), None) => format!("{tool_name} (from {t})"),
                _ => format!("{tool_name} (installed via mise)"),
            };
            Some(source_info)
        }
        None => Some(format!("{tool_name} (installed via mise)")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_tool() {
        let output = r#"{
            "node": [
                {
                    "version": "20.0.0",
                    "install_path": "/home/user/.mise/installs/node/20.0.0",
                    "source": {
                        "type": "mise.toml",
                        "path": "/home/user/project/mise.toml"
                    }
                }
            ]
        }"#;

        let packages = parse_mise_output(output).unwrap();
        assert_eq!(packages.len(), 1);

        let pkg = &packages[0];
        assert_eq!(pkg.name, "node");
        assert_eq!(pkg.version, "20.0.0");
        assert!(pkg.url.is_none());
        assert_eq!(pkg.source, PackageSource::Mise);
        assert!(pkg.licenses.is_empty());
        assert!(pkg.description.is_some());
    }

    #[test]
    fn parse_multiple_tools() {
        let output = r#"{
            "node": [
                {
                    "version": "20.0.0",
                    "install_path": "/home/user/.mise/installs/node/20.0.0",
                    "source": {
                        "type": "mise.toml",
                        "path": "/home/user/mise.toml"
                    }
                }
            ],
            "python": [
                {
                    "version": "3.12.0",
                    "install_path": "/home/user/.mise/installs/python/3.12.0",
                    "source": {
                        "type": ".tool-versions",
                        "path": "/home/user/.tool-versions"
                    }
                }
            ],
            "rust": [
                {
                    "version": "1.77.0",
                    "install_path": "/home/user/.mise/installs/rust/1.77.0",
                    "source": null
                }
            ]
        }"#;

        let packages = parse_mise_output(output).unwrap();
        assert_eq!(packages.len(), 3);

        // Sorted by name then version
        assert_eq!(packages[0].name, "node");
        assert_eq!(packages[1].name, "python");
        assert_eq!(packages[2].name, "rust");

        assert!(packages[0].url.is_none());
        assert!(packages[1].url.is_none());
        assert!(packages[2].url.is_none());
    }

    #[test]
    fn parse_multiple_versions_same_tool() {
        let output = r#"{
            "node": [
                {
                    "version": "18.0.0",
                    "install_path": "/home/user/.mise/installs/node/18.0.0",
                    "source": null
                },
                {
                    "version": "20.0.0",
                    "install_path": "/home/user/.mise/installs/node/20.0.0",
                    "source": null
                }
            ]
        }"#;

        let packages = parse_mise_output(output).unwrap();
        assert_eq!(packages.len(), 2);
        assert_eq!(packages[0].version, "18.0.0");
        assert_eq!(packages[1].version, "20.0.0");
    }

    #[test]
    fn parse_empty_output() {
        let packages = parse_mise_output("").unwrap();
        assert!(packages.is_empty());
    }

    #[test]
    fn parse_empty_json_object() {
        let packages = parse_mise_output("{}").unwrap();
        assert!(packages.is_empty());
    }

    #[test]
    fn parse_tool_with_source_info() {
        let output = r#"{
            "go": [
                {
                    "version": "1.22.0",
                    "install_path": "/home/user/.mise/installs/go/1.22.0",
                    "source": {
                        "type": "mise.toml",
                        "path": "/home/user/project/mise.toml"
                    }
                }
            ]
        }"#;

        let packages = parse_mise_output(output).unwrap();
        assert_eq!(packages.len(), 1);
        assert_eq!(
            packages[0].description.as_deref(),
            Some("go (from mise.toml: /home/user/project/mise.toml)")
        );
    }

    #[test]
    fn parse_tool_with_null_source() {
        let output = r#"{
            "ruby": [
                {
                    "version": "3.3.0",
                    "install_path": "/home/user/.mise/installs/ruby/3.3.0",
                    "source": null
                }
            ]
        }"#;

        let packages = parse_mise_output(output).unwrap();
        assert_eq!(packages.len(), 1);
        assert_eq!(
            packages[0].description.as_deref(),
            Some("ruby (installed via mise)")
        );
    }

    #[test]
    fn parse_empty_version_array() {
        let output = r#"{
            "node": []
        }"#;

        let packages = parse_mise_output(output).unwrap();
        assert!(packages.is_empty());
    }
}
