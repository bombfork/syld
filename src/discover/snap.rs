// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};

use super::{Discoverer, InstalledPackage, PackageSource};

/// Discovers applications installed via Snap.
///
/// Runs `snap list` to enumerate installed snap packages. The tabular output
/// contains columns for name, version, revision, tracking channel, publisher,
/// and notes. Additional metadata (description) is read from each snap's
/// `meta/snap.yaml` file when available.
pub struct SnapDiscoverer;

impl Discoverer for SnapDiscoverer {
    fn name(&self) -> &str {
        "snap"
    }

    fn is_available(&self) -> bool {
        Path::new("/usr/bin/snap").is_file() || Path::new("/snap").is_dir()
    }

    fn discover(&self) -> Result<Vec<InstalledPackage>> {
        let output = Command::new("snap")
            .args(["list"])
            .output()
            .context("Failed to run snap list")?;

        if !output.status.success() {
            anyhow::bail!(
                "snap list failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let stdout =
            String::from_utf8(output.stdout).context("snap list output is not valid UTF-8")?;

        parse_snap_output(&stdout)
    }
}

/// Parse the columnar output of `snap list`.
///
/// The first line is a header row. Subsequent lines contain whitespace-separated
/// fields: Name, Version, Rev, Tracking, Publisher, Notes.
fn parse_snap_output(output: &str) -> Result<Vec<InstalledPackage>> {
    let lines: Vec<&str> = output
        .lines()
        .filter(|l| !l.is_empty())
        .skip(1) // skip header row
        .collect();

    let pb = ProgressBar::new(lines.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("  {bar:30} {pos}/{len} packages")
            .unwrap(),
    );

    let packages: Vec<InstalledPackage> = lines
        .iter()
        .filter_map(|line| {
            let result = parse_snap_line(line);
            pb.inc(1);
            match result {
                Ok(pkg) => Some(pkg),
                Err(e) => {
                    pb.suspend(|| {
                        eprintln!("  Warning: failed to parse snap entry: {e}");
                    });
                    None
                }
            }
        })
        .collect();

    pb.finish_and_clear();

    // Enrich packages with descriptions from snap.yaml when available.
    let packages = enrich_with_descriptions(packages);

    Ok(packages)
}

/// Parse a single line from `snap list` output.
///
/// Expected columns (whitespace-separated):
/// Name  Version  Rev  Tracking  Publisher  Notes
fn parse_snap_line(line: &str) -> Result<InstalledPackage> {
    let fields: Vec<&str> = line.split_whitespace().collect();

    let name = fields
        .first()
        .filter(|s| !s.is_empty())
        .context("Missing snap name")?
        .to_string();

    let version = fields
        .get(1)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    Ok(InstalledPackage {
        name,
        version,
        description: None,
        url: None,
        source: PackageSource::Snap,
        licenses: Vec::new(),
    })
}

/// Attempt to read the description from `/snap/<name>/current/meta/snap.yaml`.
fn enrich_with_descriptions(mut packages: Vec<InstalledPackage>) -> Vec<InstalledPackage> {
    for pkg in &mut packages {
        let yaml_path = format!("/snap/{}/current/meta/snap.yaml", pkg.name);
        if let Ok(contents) = std::fs::read_to_string(&yaml_path)
            && let Some(desc) = extract_description(&contents)
        {
            pkg.description = Some(desc);
        }
    }
    packages
}

/// Extract the `description` field from a snap.yaml file.
///
/// Performs simple line-based parsing to avoid pulling in a YAML dependency.
/// Looks for a line starting with `description:` and extracts its value.
fn extract_description(yaml: &str) -> Option<String> {
    for line in yaml.lines() {
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix("description:") {
            let value = value.trim().trim_matches(|c| c == '\'' || c == '"');
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = "Name    Version   Rev    Tracking       Publisher   Notes";

    #[test]
    fn parse_full_output() {
        let output = format!("{HEADER}\nfirefox  128.0.3   4793   latest/stable  mozilla**   -\n");
        let packages = parse_snap_output(&output).unwrap();
        assert_eq!(packages.len(), 1);
        let pkg = &packages[0];
        assert_eq!(pkg.name, "firefox");
        assert_eq!(pkg.version, "128.0.3");
        assert_eq!(pkg.source, PackageSource::Snap);
        assert!(pkg.licenses.is_empty());
    }

    #[test]
    fn parse_multiple_snaps() {
        let output = format!(
            "{HEADER}
core20      20240227  2318   latest/stable  canonical**  base
firefox     128.0.3   4793   latest/stable  mozilla**    -
snapd       2.63      21759  latest/stable  canonical**  snapd
"
        );
        let packages = parse_snap_output(&output).unwrap();
        assert_eq!(packages.len(), 3);
        assert_eq!(packages[0].name, "core20");
        assert_eq!(packages[1].name, "firefox");
        assert_eq!(packages[2].name, "snapd");
    }

    #[test]
    fn parse_empty_output() {
        let packages = parse_snap_output("").unwrap();
        assert!(packages.is_empty());
    }

    #[test]
    fn parse_header_only() {
        let output = format!("{HEADER}\n");
        let packages = parse_snap_output(&output).unwrap();
        assert!(packages.is_empty());
    }

    #[test]
    fn parse_skips_blank_lines() {
        let output = format!("{HEADER}\n\nfirefox  128.0  4793  latest/stable  mozilla**  -\n\n");
        let packages = parse_snap_output(&output).unwrap();
        assert_eq!(packages.len(), 1);
    }

    #[test]
    fn parse_minimal_line() {
        let output = format!("{HEADER}\nsomepkg  1.0\n");
        let packages = parse_snap_output(&output).unwrap();
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].name, "somepkg");
        assert_eq!(packages[0].version, "1.0");
    }

    #[test]
    fn extract_description_simple() {
        let yaml = "name: firefox\ndescription: Fast web browser\nversion: 128.0\n";
        assert_eq!(
            extract_description(yaml),
            Some("Fast web browser".to_string())
        );
    }

    #[test]
    fn extract_description_quoted() {
        let yaml = "description: 'A quoted description'\n";
        assert_eq!(
            extract_description(yaml),
            Some("A quoted description".to_string())
        );
    }

    #[test]
    fn extract_description_missing() {
        let yaml = "name: firefox\nversion: 128.0\n";
        assert_eq!(extract_description(yaml), None);
    }

    #[test]
    fn extract_description_empty() {
        let yaml = "description:\n";
        assert_eq!(extract_description(yaml), None);
    }
}
