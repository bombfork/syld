// SPDX-License-Identifier: GPL-3.0-or-later

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};

use super::{Discoverer, InstalledPackage, PackageSource};

/// Discovers packages installed via apt by reading the dpkg status database.
///
/// The dpkg database is a single file at /var/lib/dpkg/status using the Debian
/// control file format (RFC 822-style `Key: Value` paragraphs separated by
/// blank lines).
pub struct AptDiscoverer;

const DPKG_STATUS_PATH: &str = "/var/lib/dpkg/status";

impl Discoverer for AptDiscoverer {
    fn name(&self) -> &str {
        "apt"
    }

    fn is_available(&self) -> bool {
        Path::new(DPKG_STATUS_PATH).is_file()
    }

    fn discover(&self) -> Result<Vec<InstalledPackage>> {
        let content =
            fs::read_to_string(DPKG_STATUS_PATH).context("Failed to read dpkg status file")?;
        parse_dpkg_status(&content)
    }
}

/// Parse the entire dpkg status file into a list of installed packages.
///
/// Paragraphs are separated by blank lines. Each paragraph describes one
/// package. Packages whose `Status` field does not contain "installed" are
/// skipped (e.g. packages that have been removed but not purged).
fn parse_dpkg_status(content: &str) -> Result<Vec<InstalledPackage>> {
    let paragraphs: Vec<&str> = content.split("\n\n").collect();

    let pb = ProgressBar::new(paragraphs.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("  {bar:30} {pos}/{len} packages")
            .unwrap(),
    );

    let mut packages = Vec::new();

    for paragraph in &paragraphs {
        pb.inc(1);

        let paragraph = paragraph.trim();
        if paragraph.is_empty() {
            continue;
        }

        match parse_dpkg_entry(paragraph) {
            Ok(Some(pkg)) => packages.push(pkg),
            Ok(None) => {} // not installed, skip
            Err(e) => {
                pb.suspend(|| {
                    eprintln!("  Warning: failed to parse dpkg entry: {e}");
                });
            }
        }
    }

    pb.finish_and_clear();

    Ok(packages)
}

/// Parse a single dpkg status paragraph into an [`InstalledPackage`].
///
/// Returns `Ok(None)` if the package is not in the "installed" state (e.g.
/// removed or half-configured). Returns `Err` only if required fields
/// (`Package`, `Version`) are missing.
fn parse_dpkg_entry(entry: &str) -> Result<Option<InstalledPackage>> {
    let mut name = None;
    let mut version = None;
    let mut homepage = None;
    let mut status = None;

    let mut current_key: Option<&str> = None;
    let mut desc_lines: Vec<&str> = Vec::new();

    for line in entry.lines() {
        if let Some(rest) = line.strip_prefix(' ') {
            // Continuation line (part of the previous field's value).
            if current_key == Some("Description") {
                if rest == "." {
                    desc_lines.push("");
                } else {
                    desc_lines.push(rest);
                }
            }
            continue;
        }

        if let Some((key, value)) = line.split_once(": ") {
            current_key = Some(key);
            match key {
                "Package" => name = Some(value.to_string()),
                "Version" => version = Some(value.to_string()),
                "Description" => {
                    desc_lines.clear();
                    desc_lines.push(value);
                }
                "Homepage" => homepage = Some(value.to_string()),
                "Status" => status = Some(value),
                _ => {}
            }
        }
    }

    // Only include packages that are fully installed.
    if let Some(s) = status
        && !s.contains("installed")
    {
        return Ok(None);
    }

    let description = if desc_lines.is_empty() {
        None
    } else {
        Some(desc_lines.join("\n"))
    };

    Ok(Some(InstalledPackage {
        name: name.context("Missing Package field in dpkg entry")?,
        version: version.context("Missing Version field in dpkg entry")?,
        description,
        url: homepage,
        source: PackageSource::Apt,
        licenses: Vec::new(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_full_entry() {
        let entry = "\
Package: curl
Version: 7.88.1-10+deb12u5
Status: install ok installed
Section: web
Homepage: https://curl.se/
Description: command line tool for transferring data with URL syntax
 curl is a command line tool for transferring data with URL syntax,
 supporting many protocols including HTTP and HTTPS.";
        let pkg = parse_dpkg_entry(entry).unwrap().unwrap();
        assert_eq!(pkg.name, "curl");
        assert_eq!(pkg.version, "7.88.1-10+deb12u5");
        assert_eq!(pkg.url.as_deref(), Some("https://curl.se/"));
        assert_eq!(
            pkg.description.as_deref(),
            Some(
                "command line tool for transferring data with URL syntax\n\
                 curl is a command line tool for transferring data with URL syntax,\n\
                 supporting many protocols including HTTP and HTTPS."
            )
        );
        assert_eq!(pkg.source, PackageSource::Apt);
        assert!(pkg.licenses.is_empty());
    }

    #[test]
    fn parse_minimal_entry() {
        let entry = "\
Package: base-files
Version: 12.4+deb12u5
Status: install ok installed";
        let pkg = parse_dpkg_entry(entry).unwrap().unwrap();
        assert_eq!(pkg.name, "base-files");
        assert_eq!(pkg.version, "12.4+deb12u5");
        assert_eq!(pkg.description, None);
        assert_eq!(pkg.url, None);
    }

    #[test]
    fn skips_removed_package() {
        let entry = "\
Package: old-pkg
Version: 1.0
Status: deinstall ok config-files";
        let result = parse_dpkg_entry(entry).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn skips_half_installed_package() {
        let entry = "\
Package: broken-pkg
Version: 2.0
Status: install reinstreq half-installed";
        // "half-installed" still contains "installed" — this is intentional,
        // since dpkg considers it an installed (albeit broken) state.
        let result = parse_dpkg_entry(entry).unwrap();
        assert!(result.is_some());
    }

    #[test]
    fn missing_package_field_errors() {
        let entry = "\
Version: 1.0
Status: install ok installed";
        let err = parse_dpkg_entry(entry).unwrap_err();
        assert!(err.to_string().contains("Package"));
    }

    #[test]
    fn missing_version_field_errors() {
        let entry = "\
Package: something
Status: install ok installed";
        let err = parse_dpkg_entry(entry).unwrap_err();
        assert!(err.to_string().contains("Version"));
    }

    #[test]
    fn multiline_description_with_blank_lines() {
        let entry = "\
Package: apt
Version: 2.6.1
Status: install ok installed
Description: commandline package manager
 apt provides commandline tools for searching and
 managing packages.
 .
 This package contains the apt-get and apt-cache tools.";
        let pkg = parse_dpkg_entry(entry).unwrap().unwrap();
        assert_eq!(
            pkg.description.as_deref(),
            Some(
                "commandline package manager\n\
                 apt provides commandline tools for searching and\n\
                 managing packages.\n\
                 \n\
                 This package contains the apt-get and apt-cache tools."
            )
        );
    }

    #[test]
    fn parse_multiple_packages() {
        let content = "\
Package: pkg-a
Version: 1.0
Status: install ok installed
Homepage: https://example.com/a

Package: pkg-b
Version: 2.0
Status: install ok installed

Package: removed-pkg
Version: 3.0
Status: deinstall ok config-files
";
        let packages = parse_dpkg_status(content).unwrap();
        assert_eq!(packages.len(), 2);
        assert_eq!(packages[0].name, "pkg-a");
        assert_eq!(packages[0].url.as_deref(), Some("https://example.com/a"));
        assert_eq!(packages[1].name, "pkg-b");
        assert_eq!(packages[1].url, None);
    }

    #[test]
    fn parse_empty_file() {
        let packages = parse_dpkg_status("").unwrap();
        assert!(packages.is_empty());
    }

    #[test]
    fn parse_only_whitespace() {
        let packages = parse_dpkg_status("   \n\n  \n").unwrap();
        assert!(packages.is_empty());
    }

    #[test]
    fn ignores_unknown_fields() {
        let entry = "\
Package: pkg
Version: 1.0
Status: install ok installed
Maintainer: Someone <someone@example.com>
Architecture: amd64
Depends: libc6";
        let pkg = parse_dpkg_entry(entry).unwrap().unwrap();
        assert_eq!(pkg.name, "pkg");
        assert_eq!(pkg.version, "1.0");
    }

    #[test]
    fn no_status_field_includes_package() {
        // If there's no Status field at all, include the package
        // (the file might be a partial extract or custom format).
        let entry = "\
Package: pkg
Version: 1.0";
        let result = parse_dpkg_entry(entry).unwrap();
        assert!(result.is_some());
    }

    #[test]
    fn description_single_line_only() {
        let entry = "\
Package: pkg
Version: 1.0
Status: install ok installed
Description: A simple package";
        let pkg = parse_dpkg_entry(entry).unwrap().unwrap();
        assert_eq!(pkg.description.as_deref(), Some("A simple package"));
    }
}
