// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use serde::Deserialize;

use super::{Discoverer, InstalledPackage, PackageSource};

/// Discovers packages installed via Homebrew or Linuxbrew.
///
/// Runs `brew info --json=v2 --installed` to enumerate installed formulae and
/// casks. The JSON output contains separate arrays for formulae and casks, each
/// with name, version, description, license, and homepage metadata.
pub struct BrewDiscoverer;

impl Discoverer for BrewDiscoverer {
    fn name(&self) -> &str {
        "brew"
    }

    fn is_available(&self) -> bool {
        // Check common Linuxbrew paths
        Path::new("/home/linuxbrew/.linuxbrew/bin/brew").is_file()
            || std::env::var_os("HOME")
                .map(|h| Path::new(&h).join(".linuxbrew/bin/brew").is_file())
                .unwrap_or(false)
            // macOS Homebrew path
            || Path::new("/opt/homebrew/bin/brew").is_file()
            // Check standard PATH locations
            || Path::new("/usr/local/bin/brew").is_file()
    }

    fn discover(&self) -> Result<Vec<InstalledPackage>> {
        let output = Command::new("brew")
            .args(["info", "--json=v2", "--installed"])
            .output()
            .context("Failed to run brew info")?;

        if !output.status.success() {
            anyhow::bail!(
                "brew info failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let stdout =
            String::from_utf8(output.stdout).context("brew info output is not valid UTF-8")?;

        parse_brew_info(&stdout)
    }
}

#[derive(Deserialize)]
struct BrewInfoOutput {
    formulae: Vec<BrewFormula>,
    #[serde(default)]
    casks: Vec<BrewCask>,
}

#[derive(Deserialize)]
struct BrewFormula {
    name: String,
    desc: Option<String>,
    license: Option<String>,
    homepage: Option<String>,
    installed: Vec<BrewInstalledVersion>,
}

#[derive(Deserialize)]
struct BrewInstalledVersion {
    version: String,
}

#[derive(Deserialize)]
struct BrewCask {
    token: String,
    desc: Option<String>,
    homepage: Option<String>,
    version: Option<String>,
}

/// Parse the JSON output of `brew info --json=v2 --installed`.
///
/// Returns a combined list of installed formulae and casks as
/// [`InstalledPackage`] entries attributed to [`PackageSource::Brew`].
fn parse_brew_info(json: &str) -> Result<Vec<InstalledPackage>> {
    let info: BrewInfoOutput =
        serde_json::from_str(json).context("Failed to parse brew info JSON")?;

    let total = info.formulae.len() + info.casks.len();
    let pb = ProgressBar::new(total as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("  {bar:30} {pos}/{len} packages")
            .unwrap(),
    );

    let mut packages = Vec::with_capacity(total);

    for formula in &info.formulae {
        let version = formula
            .installed
            .first()
            .map(|v| v.version.clone())
            .unwrap_or_else(|| "unknown".to_string());

        let licenses = formula
            .license
            .as_ref()
            .map(|l| vec![l.clone()])
            .unwrap_or_default();

        packages.push(InstalledPackage {
            name: formula.name.clone(),
            version,
            description: formula.desc.clone(),
            url: formula.homepage.clone(),
            source: PackageSource::Brew,
            licenses,
        });
        pb.inc(1);
    }

    for cask in &info.casks {
        let version = cask
            .version
            .clone()
            .unwrap_or_else(|| "unknown".to_string());

        packages.push(InstalledPackage {
            name: cask.token.clone(),
            version,
            description: cask.desc.clone(),
            url: cask.homepage.clone(),
            source: PackageSource::Brew,
            licenses: Vec::new(),
        });
        pb.inc(1);
    }

    pb.finish_and_clear();

    Ok(packages)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_formulae_basic() {
        let json = r#"{
            "formulae": [
                {
                    "name": "wget",
                    "desc": "Internet file retriever",
                    "license": "GPL-3.0-or-later",
                    "homepage": "https://www.gnu.org/software/wget/",
                    "installed": [{"version": "1.24.5"}]
                }
            ],
            "casks": []
        }"#;
        let packages = parse_brew_info(json).unwrap();
        assert_eq!(packages.len(), 1);
        let pkg = &packages[0];
        assert_eq!(pkg.name, "wget");
        assert_eq!(pkg.version, "1.24.5");
        assert_eq!(pkg.description.as_deref(), Some("Internet file retriever"));
        assert_eq!(
            pkg.url.as_deref(),
            Some("https://www.gnu.org/software/wget/")
        );
        assert_eq!(pkg.source, PackageSource::Brew);
        assert_eq!(pkg.licenses, vec!["GPL-3.0-or-later"]);
    }

    #[test]
    fn parse_casks_basic() {
        let json = r#"{
            "formulae": [],
            "casks": [
                {
                    "token": "firefox",
                    "desc": "Web browser",
                    "homepage": "https://www.mozilla.org/firefox/",
                    "version": "128.0"
                }
            ]
        }"#;
        let packages = parse_brew_info(json).unwrap();
        assert_eq!(packages.len(), 1);
        let pkg = &packages[0];
        assert_eq!(pkg.name, "firefox");
        assert_eq!(pkg.version, "128.0");
        assert_eq!(pkg.description.as_deref(), Some("Web browser"));
        assert_eq!(pkg.url.as_deref(), Some("https://www.mozilla.org/firefox/"));
        assert_eq!(pkg.source, PackageSource::Brew);
        assert!(pkg.licenses.is_empty());
    }

    #[test]
    fn parse_mixed_output() {
        let json = r#"{
            "formulae": [
                {
                    "name": "git",
                    "desc": "Distributed revision control system",
                    "license": "GPL-2.0-only",
                    "homepage": "https://git-scm.com",
                    "installed": [{"version": "2.45.0"}]
                }
            ],
            "casks": [
                {
                    "token": "firefox",
                    "desc": "Web browser",
                    "homepage": "https://www.mozilla.org/firefox/",
                    "version": "128.0"
                }
            ]
        }"#;
        let packages = parse_brew_info(json).unwrap();
        assert_eq!(packages.len(), 2);
        assert_eq!(packages[0].name, "git");
        assert_eq!(packages[0].source, PackageSource::Brew);
        assert_eq!(packages[1].name, "firefox");
        assert_eq!(packages[1].source, PackageSource::Brew);
    }

    #[test]
    fn parse_empty_output() {
        let json = r#"{"formulae": [], "casks": []}"#;
        let packages = parse_brew_info(json).unwrap();
        assert!(packages.is_empty());
    }

    #[test]
    fn parse_formula_no_license() {
        let json = r#"{
            "formulae": [
                {
                    "name": "somepkg",
                    "desc": "A package",
                    "license": null,
                    "homepage": "https://example.com",
                    "installed": [{"version": "1.0"}]
                }
            ],
            "casks": []
        }"#;
        let packages = parse_brew_info(json).unwrap();
        assert_eq!(packages.len(), 1);
        assert!(packages[0].licenses.is_empty());
    }

    #[test]
    fn parse_formula_no_homepage() {
        let json = r#"{
            "formulae": [
                {
                    "name": "somepkg",
                    "desc": "A package",
                    "license": "MIT",
                    "homepage": null,
                    "installed": [{"version": "2.0"}]
                }
            ],
            "casks": []
        }"#;
        let packages = parse_brew_info(json).unwrap();
        assert_eq!(packages.len(), 1);
        assert!(packages[0].url.is_none());
        assert_eq!(packages[0].licenses, vec!["MIT"]);
    }

    #[test]
    fn parse_formula_multiple_versions() {
        let json = r#"{
            "formulae": [
                {
                    "name": "python",
                    "desc": "Interpreted, interactive, object-oriented programming language",
                    "license": "Python-2.0",
                    "homepage": "https://www.python.org/",
                    "installed": [
                        {"version": "3.12.4"},
                        {"version": "3.11.9"}
                    ]
                }
            ],
            "casks": []
        }"#;
        let packages = parse_brew_info(json).unwrap();
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].version, "3.12.4");
    }
}
