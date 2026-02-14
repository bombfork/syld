// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};

use super::{Discoverer, InstalledPackage, PackageSource};

/// Discovers packages installed via Nix (both NixOS system packages and user profiles).
///
/// Uses `nix profile list --json` to enumerate packages in the default profile,
/// and also reads NixOS system packages from `/run/current-system/sw/` when
/// available. Package name and version are extracted from Nix store paths which
/// follow the pattern `/nix/store/<hash>-<name>-<version>`.
pub struct NixDiscoverer;

impl Discoverer for NixDiscoverer {
    fn name(&self) -> &str {
        "nix"
    }

    fn is_available(&self) -> bool {
        Path::new("/nix/store").is_dir()
    }

    fn discover(&self) -> Result<Vec<InstalledPackage>> {
        let mut packages = Vec::new();

        // Try user profile packages via `nix profile list`
        if let Ok(profile_pkgs) = discover_profile_packages() {
            packages.extend(profile_pkgs);
        }

        // Try NixOS system packages from /run/current-system/sw/
        if let Ok(system_pkgs) = discover_system_packages() {
            packages.extend(system_pkgs);
        }

        // Deduplicate by name (prefer system packages which come second)
        dedup_packages(&mut packages);

        Ok(packages)
    }
}

/// Discover packages from the user's Nix profile using `nix profile list`.
///
/// Parses the human-readable output where each entry contains a store path
/// like `/nix/store/<hash>-<name>-<version>`.
fn discover_profile_packages() -> Result<Vec<InstalledPackage>> {
    let output = Command::new("nix")
        .args(["profile", "list"])
        .output()
        .context("Failed to run nix profile list")?;

    if !output.status.success() {
        anyhow::bail!(
            "nix profile list failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let stdout =
        String::from_utf8(output.stdout).context("nix profile list output is not valid UTF-8")?;

    parse_profile_output(&stdout)
}

/// Parse the output of `nix profile list`.
///
/// Each line contains fields separated by whitespace. The store path
/// (starting with `/nix/store/`) contains the package name and version.
/// Example line:
/// ```text
/// Name:               firefox
/// Flake reference:    flake:nixpkgs#firefox
/// Store paths:        /nix/store/abc123-firefox-128.0
/// ```
///
/// Or in the older format, each entry is a single line:
/// ```text
/// 0 flake:nixpkgs#firefox github:NixOS/nixpkgs/abc123#firefox /nix/store/abc123-firefox-128.0
/// ```
fn parse_profile_output(output: &str) -> Result<Vec<InstalledPackage>> {
    // Try the new multi-line format first (Nix 2.20+)
    let packages = if output.contains("Store paths:") || output.contains("Store path:") {
        parse_new_profile_format(output)?
    } else {
        // Fall back to the legacy single-line format
        parse_legacy_profile_format(output)?
    };

    let pb = ProgressBar::new(packages.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("  {bar:30} {pos}/{len} packages")
            .unwrap(),
    );
    for pkg in &packages {
        pb.inc(1);
        let _ = pkg;
    }
    pb.finish_and_clear();

    Ok(packages)
}

/// Parse the new multi-line `nix profile list` format (Nix 2.20+).
///
/// Example:
/// ```text
/// Name:               firefox
/// Flake reference:    flake:nixpkgs#firefox
/// Store paths:        /nix/store/abc123-firefox-128.0
///
/// Name:               git
/// Flake reference:    flake:nixpkgs#git
/// Store paths:        /nix/store/def456-git-2.45.0
/// ```
fn parse_new_profile_format(output: &str) -> Result<Vec<InstalledPackage>> {
    let mut packages = Vec::new();
    let mut current_store_path: Option<String> = None;

    for line in output.lines() {
        let trimmed = line.trim();

        if let Some(paths) = trimmed
            .strip_prefix("Store paths:")
            .or_else(|| trimmed.strip_prefix("Store path:"))
        {
            current_store_path = Some(paths.trim().to_string());
        }

        // When we hit a blank line or end, process the accumulated entry
        if (trimmed.is_empty() || trimmed.starts_with("Name:"))
            && let Some(ref store_path) = current_store_path
        {
            // May have multiple paths separated by spaces; take the first
            let first_path = store_path.split_whitespace().next().unwrap_or(store_path);
            if let Some(pkg) = parse_store_path(first_path) {
                packages.push(pkg);
            }
            current_store_path = None;
        }
    }

    // Handle last entry
    if let Some(ref store_path) = current_store_path {
        let first_path = store_path.split_whitespace().next().unwrap_or(store_path);
        if let Some(pkg) = parse_store_path(first_path) {
            packages.push(pkg);
        }
    }

    Ok(packages)
}

/// Parse the legacy single-line `nix profile list` format.
///
/// Each line: `<index> <flake-ref> <resolved-ref> <store-path>`
fn parse_legacy_profile_format(output: &str) -> Result<Vec<InstalledPackage>> {
    let mut packages = Vec::new();

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Find the store path (starts with /nix/store/)
        for field in trimmed.split_whitespace() {
            if field.starts_with("/nix/store/") {
                if let Some(pkg) = parse_store_path(field) {
                    packages.push(pkg);
                }
                break;
            }
        }
    }

    Ok(packages)
}

/// Discover NixOS system packages from `/run/current-system/sw/`.
///
/// On NixOS, the system profile links to store paths in its `manifest.nix`
/// or through the `bin/`, `share/` etc. symlinks. We read the store paths
/// from the `manifest.nix` if available, or walk the directory.
fn discover_system_packages() -> Result<Vec<InstalledPackage>> {
    let sw_path = Path::new("/run/current-system/sw");
    if !sw_path.is_dir() {
        return Ok(Vec::new());
    }

    // Use `nix-store --query --references` to list all packages in the system profile
    let output = Command::new("nix-store")
        .args(["--query", "--references", "/run/current-system/sw"])
        .output()
        .context("Failed to run nix-store --query --references")?;

    if !output.status.success() {
        anyhow::bail!(
            "nix-store query failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let stdout =
        String::from_utf8(output.stdout).context("nix-store query output is not valid UTF-8")?;

    let mut packages = Vec::new();

    let lines: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();

    let pb = ProgressBar::new(lines.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("  {bar:30} {pos}/{len} packages")
            .unwrap(),
    );

    for line in &lines {
        if let Some(pkg) = parse_store_path(line.trim()) {
            packages.push(pkg);
        }
        pb.inc(1);
    }

    pb.finish_and_clear();

    Ok(packages)
}

/// Parse a Nix store path to extract package name and version.
///
/// Store paths have the format: `/nix/store/<hash>-<name>-<version>`
/// The hash is always 32 characters followed by a dash.
///
/// Examples:
/// - `/nix/store/abc123...xyz-firefox-128.0` -> name="firefox", version="128.0"
/// - `/nix/store/abc123...xyz-coreutils-9.5` -> name="coreutils", version="9.5"
/// - `/nix/store/abc123...xyz-my-package-1.0` -> name="my-package", version="1.0"
/// - `/nix/store/abc123...xyz-some-lib` -> name="some-lib", version="unknown"
fn parse_store_path(path: &str) -> Option<InstalledPackage> {
    // Strip the /nix/store/<hash>- prefix
    let after_store = path.strip_prefix("/nix/store/")?;
    // The hash is 32 chars, followed by a dash
    if after_store.len() < 34 || after_store.as_bytes()[32] != b'-' {
        return None;
    }
    let name_version = &after_store[33..];

    if name_version.is_empty() {
        return None;
    }

    let (name, version) = split_name_version(name_version);

    Some(InstalledPackage {
        name,
        version,
        description: None,
        url: None,
        source: PackageSource::Nix,
        licenses: Vec::new(),
    })
}

/// Split a Nix derivation name-version string into (name, version).
///
/// Nix convention: the version starts at the last segment that begins with a
/// digit, scanning from the right. For example:
/// - `firefox-128.0` -> ("firefox", "128.0")
/// - `my-package-1.0.2` -> ("my-package", "1.0.2")
/// - `some-lib` -> ("some-lib", "unknown")
/// - `python3-3.12.0` -> ("python3", "3.12.0")
fn split_name_version(s: &str) -> (String, String) {
    // Find the last dash where the part after it starts with a digit
    let mut split_pos = None;
    for (i, _) in s.match_indices('-') {
        let rest = &s[i + 1..];
        if rest.starts_with(|c: char| c.is_ascii_digit()) {
            split_pos = Some(i);
        }
    }

    match split_pos {
        Some(pos) => (s[..pos].to_string(), s[pos + 1..].to_string()),
        None => (s.to_string(), "unknown".to_string()),
    }
}

/// Remove duplicate packages by name, keeping the last occurrence.
fn dedup_packages(packages: &mut Vec<InstalledPackage>) {
    let mut seen = std::collections::HashSet::new();
    // Reverse so we keep the last occurrence (system packages override profile ones)
    packages.reverse();
    packages.retain(|pkg| seen.insert(pkg.name.clone()));
    packages.reverse();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_store_path_full() {
        let path = "/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-firefox-128.0";
        let pkg = parse_store_path(path).unwrap();
        assert_eq!(pkg.name, "firefox");
        assert_eq!(pkg.version, "128.0");
        assert_eq!(pkg.source, PackageSource::Nix);
    }

    #[test]
    fn parse_store_path_no_version() {
        let path = "/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-coreutils";
        let pkg = parse_store_path(path).unwrap();
        assert_eq!(pkg.name, "coreutils");
        assert_eq!(pkg.version, "unknown");
    }

    #[test]
    fn parse_store_path_hyphenated_name() {
        let path = "/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-my-cool-package-2.1.3";
        let pkg = parse_store_path(path).unwrap();
        assert_eq!(pkg.name, "my-cool-package");
        assert_eq!(pkg.version, "2.1.3");
    }

    #[test]
    fn parse_store_path_name_with_digits() {
        let path = "/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-python3-3.12.0";
        let pkg = parse_store_path(path).unwrap();
        assert_eq!(pkg.name, "python3");
        assert_eq!(pkg.version, "3.12.0");
    }

    #[test]
    fn parse_store_path_invalid() {
        assert!(parse_store_path("/not/a/store/path").is_none());
        assert!(parse_store_path("").is_none());
        assert!(parse_store_path("/nix/store/short-pkg").is_none());
    }

    #[test]
    fn split_name_version_basic() {
        let (name, version) = split_name_version("firefox-128.0");
        assert_eq!(name, "firefox");
        assert_eq!(version, "128.0");
    }

    #[test]
    fn split_name_version_complex() {
        let (name, version) = split_name_version("my-great-lib-0.1.2");
        assert_eq!(name, "my-great-lib");
        assert_eq!(version, "0.1.2");
    }

    #[test]
    fn split_name_version_no_version() {
        let (name, version) = split_name_version("just-a-name");
        assert_eq!(name, "just-a-name");
        assert_eq!(version, "unknown");
    }

    #[test]
    fn split_name_version_numeric_suffix_in_name() {
        let (name, version) = split_name_version("lib2to3-3.12.0");
        assert_eq!(name, "lib2to3");
        assert_eq!(version, "3.12.0");
    }

    #[test]
    fn parse_legacy_profile() {
        let output = "\
0 flake:nixpkgs#firefox github:NixOS/nixpkgs/abc123#firefox /nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-firefox-128.0
1 flake:nixpkgs#git github:NixOS/nixpkgs/abc123#git /nix/store/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb-git-2.45.0
";
        let packages = parse_legacy_profile_format(output).unwrap();
        assert_eq!(packages.len(), 2);
        assert_eq!(packages[0].name, "firefox");
        assert_eq!(packages[0].version, "128.0");
        assert_eq!(packages[1].name, "git");
        assert_eq!(packages[1].version, "2.45.0");
    }

    #[test]
    fn parse_new_profile() {
        let output = "\
Name:               firefox
Flake reference:    flake:nixpkgs#firefox
Store paths:        /nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-firefox-128.0

Name:               git
Flake reference:    flake:nixpkgs#git
Store paths:        /nix/store/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb-git-2.45.0
";
        let packages = parse_new_profile_format(output).unwrap();
        assert_eq!(packages.len(), 2);
        assert_eq!(packages[0].name, "firefox");
        assert_eq!(packages[0].version, "128.0");
        assert_eq!(packages[1].name, "git");
        assert_eq!(packages[1].version, "2.45.0");
    }

    #[test]
    fn parse_profile_empty() {
        let packages = parse_profile_output("").unwrap();
        assert!(packages.is_empty());
    }

    #[test]
    fn dedup_keeps_last() {
        let mut packages = vec![
            InstalledPackage {
                name: "firefox".to_string(),
                version: "127.0".to_string(),
                description: None,
                url: None,
                source: PackageSource::Nix,
                licenses: Vec::new(),
            },
            InstalledPackage {
                name: "firefox".to_string(),
                version: "128.0".to_string(),
                description: None,
                url: None,
                source: PackageSource::Nix,
                licenses: Vec::new(),
            },
        ];
        dedup_packages(&mut packages);
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].version, "128.0");
    }
}
