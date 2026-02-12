// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};

use super::{Discoverer, InstalledPackage, PackageSource};

/// Discovers applications installed via Flatpak.
///
/// Runs `flatpak list --app` to enumerate user-facing applications from both
/// system and user installations. Runtimes are excluded to focus on apps the
/// user has explicitly installed.
pub struct FlatpakDiscoverer;

impl Discoverer for FlatpakDiscoverer {
    fn name(&self) -> &str {
        "flatpak"
    }

    fn is_available(&self) -> bool {
        Path::new("/usr/bin/flatpak").is_file()
    }

    fn discover(&self) -> Result<Vec<InstalledPackage>> {
        let output = Command::new("flatpak")
            .args([
                "list",
                "--app",
                "--columns=application,version,description,origin",
            ])
            .output()
            .context("Failed to run flatpak list")?;

        if !output.status.success() {
            anyhow::bail!(
                "flatpak list failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let stdout =
            String::from_utf8(output.stdout).context("flatpak list output is not valid UTF-8")?;

        parse_flatpak_output(&stdout)
    }
}

/// Parse the tab-separated output of `flatpak list --columns=application,version,description,origin`.
fn parse_flatpak_output(output: &str) -> Result<Vec<InstalledPackage>> {
    let lines: Vec<&str> = output.lines().filter(|l| !l.is_empty()).collect();

    let pb = ProgressBar::new(lines.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("  {bar:30} {pos}/{len} packages")
            .unwrap(),
    );

    let packages: Vec<InstalledPackage> = lines
        .iter()
        .filter_map(|line| {
            let result = parse_flatpak_line(line);
            pb.inc(1);
            match result {
                Ok(pkg) => Some(pkg),
                Err(e) => {
                    pb.suspend(|| {
                        eprintln!("  Warning: failed to parse flatpak entry: {e}");
                    });
                    None
                }
            }
        })
        .collect();

    pb.finish_and_clear();

    Ok(packages)
}

/// Parse a single tab-separated line from flatpak list output.
///
/// Expected columns: application, version, description, origin.
fn parse_flatpak_line(line: &str) -> Result<InstalledPackage> {
    let fields: Vec<&str> = line.split('\t').collect();

    let name = fields
        .first()
        .filter(|s| !s.is_empty())
        .context("Missing application ID")?
        .to_string();

    let version = fields
        .get(1)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let description = fields
        .get(2)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    Ok(InstalledPackage {
        name,
        version,
        description,
        url: None,
        source: PackageSource::Flatpak,
        licenses: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_full_line() {
        let output = "org.mozilla.firefox\t128.0\tFast, Private & Safe Web Browser\tflathub\n";
        let packages = parse_flatpak_output(output).unwrap();
        assert_eq!(packages.len(), 1);
        let pkg = &packages[0];
        assert_eq!(pkg.name, "org.mozilla.firefox");
        assert_eq!(pkg.version, "128.0");
        assert_eq!(
            pkg.description.as_deref(),
            Some("Fast, Private & Safe Web Browser")
        );
        assert_eq!(pkg.source, PackageSource::Flatpak);
        assert!(pkg.url.is_none());
        assert!(pkg.licenses.is_empty());
    }

    #[test]
    fn parse_multiple_apps() {
        let output = "\
org.mozilla.firefox\t128.0\tFast, Private & Safe Web Browser\tflathub
org.gimp.GIMP\t2.10.38\tGNU Image Manipulation Program\tflathub
com.spotify.Client\t1.2.26\tOnline music streaming service\tflathub
";
        let packages = parse_flatpak_output(output).unwrap();
        assert_eq!(packages.len(), 3);
        assert_eq!(packages[0].name, "org.mozilla.firefox");
        assert_eq!(packages[1].name, "org.gimp.GIMP");
        assert_eq!(packages[2].name, "com.spotify.Client");
    }

    #[test]
    fn parse_missing_version() {
        let output = "org.example.App\t\tSome App\tflathub\n";
        let packages = parse_flatpak_output(output).unwrap();
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].version, "unknown");
    }

    #[test]
    fn parse_missing_description() {
        let output = "org.example.App\t1.0\t\tflathub\n";
        let packages = parse_flatpak_output(output).unwrap();
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].description, None);
    }

    #[test]
    fn parse_minimal_line() {
        let output = "org.example.App\t1.0\n";
        let packages = parse_flatpak_output(output).unwrap();
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].name, "org.example.App");
        assert_eq!(packages[0].version, "1.0");
        assert_eq!(packages[0].description, None);
    }

    #[test]
    fn parse_empty_output() {
        let packages = parse_flatpak_output("").unwrap();
        assert!(packages.is_empty());
    }

    #[test]
    fn parse_skips_blank_lines() {
        let output = "\norg.example.App\t1.0\tAn App\tflathub\n\n";
        let packages = parse_flatpak_output(output).unwrap();
        assert_eq!(packages.len(), 1);
    }

    #[test]
    fn parse_empty_application_id_skipped() {
        let output = "\t1.0\tSome App\tflathub\n";
        let packages = parse_flatpak_output(output).unwrap();
        assert!(packages.is_empty());
    }
}
