// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};

use super::{Discoverer, InstalledPackage, PackageSource};

/// Discovers packages installed via dnf/rpm (Fedora, RHEL, and derivatives).
///
/// Queries the RPM database using `rpm -qa --queryformat` to enumerate all
/// installed packages. Falls back to the `rpm` command rather than linking
/// against librpm directly.
pub struct DnfDiscoverer;

impl Discoverer for DnfDiscoverer {
    fn name(&self) -> &str {
        "dnf"
    }

    fn is_available(&self) -> bool {
        Path::new("/usr/bin/rpm").is_file() || Path::new("/var/lib/rpm").is_dir()
    }

    fn discover(&self) -> Result<Vec<InstalledPackage>> {
        let output = Command::new("rpm")
            .args([
                "-qa",
                "--queryformat",
                "%{NAME}\t%{VERSION}-%{RELEASE}\t%{SUMMARY}\t%{URL}\t%{LICENSE}\n",
            ])
            .output()
            .context("Failed to run rpm -qa")?;

        if !output.status.success() {
            anyhow::bail!(
                "rpm -qa failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let stdout =
            String::from_utf8(output.stdout).context("rpm -qa output is not valid UTF-8")?;

        parse_rpm_output(&stdout)
    }
}

/// Parse the tab-separated output of `rpm -qa --queryformat`.
///
/// Expected columns: NAME, VERSION-RELEASE, SUMMARY, URL, LICENSE.
fn parse_rpm_output(output: &str) -> Result<Vec<InstalledPackage>> {
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
            let result = parse_rpm_line(line);
            pb.inc(1);
            match result {
                Ok(pkg) => Some(pkg),
                Err(e) => {
                    pb.suspend(|| {
                        eprintln!("  Warning: failed to parse rpm entry: {e}");
                    });
                    None
                }
            }
        })
        .collect();

    pb.finish_and_clear();

    Ok(packages)
}

/// Parse a single tab-separated line from rpm query output.
///
/// Expected columns: NAME, VERSION-RELEASE, SUMMARY, URL, LICENSE.
/// RPM uses the literal string `(none)` for missing fields.
fn parse_rpm_line(line: &str) -> Result<InstalledPackage> {
    let fields: Vec<&str> = line.split('\t').collect();

    let name = fields
        .first()
        .filter(|s| !s.is_empty())
        .context("Missing package name")?
        .to_string();

    let version = fields
        .get(1)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let description = fields
        .get(2)
        .filter(|s| !s.is_empty() && **s != "(none)")
        .map(|s| s.to_string());

    let url = fields
        .get(3)
        .filter(|s| !s.is_empty() && **s != "(none)")
        .map(|s| s.to_string());

    let licenses = fields
        .get(4)
        .filter(|s| !s.is_empty() && **s != "(none)")
        .map(|s| vec![s.to_string()])
        .unwrap_or_default();

    Ok(InstalledPackage {
        name,
        version,
        description,
        url,
        source: PackageSource::Dnf,
        licenses,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_full_line() {
        let output = "bash\t5.2.26-3.fc40\tThe GNU Bourne Again shell\thttps://www.gnu.org/software/bash\tGPL-3.0-or-later\n";
        let packages = parse_rpm_output(output).unwrap();
        assert_eq!(packages.len(), 1);
        let pkg = &packages[0];
        assert_eq!(pkg.name, "bash");
        assert_eq!(pkg.version, "5.2.26-3.fc40");
        assert_eq!(
            pkg.description.as_deref(),
            Some("The GNU Bourne Again shell")
        );
        assert_eq!(
            pkg.url.as_deref(),
            Some("https://www.gnu.org/software/bash")
        );
        assert_eq!(pkg.source, PackageSource::Dnf);
        assert_eq!(pkg.licenses, vec!["GPL-3.0-or-later"]);
    }

    #[test]
    fn parse_multiple_packages() {
        let output = "\
bash\t5.2.26-3.fc40\tThe GNU Bourne Again shell\thttps://www.gnu.org/software/bash\tGPL-3.0-or-later
kernel\t6.8.5-301.fc40\tThe Linux kernel\thttps://www.kernel.org\tGPL-2.0-only
vim-enhanced\t9.1.158-1.fc40\tA version of the VIM editor\thttps://www.vim.org\tVim AND MIT
";
        let packages = parse_rpm_output(output).unwrap();
        assert_eq!(packages.len(), 3);
        assert_eq!(packages[0].name, "bash");
        assert_eq!(packages[1].name, "kernel");
        assert_eq!(packages[2].name, "vim-enhanced");
    }

    #[test]
    fn parse_none_url() {
        let output = "gpg-pubkey\t1234abcd-5678ef01\tgpg(Fedora 40)\t(none)\t(none)\n";
        let packages = parse_rpm_output(output).unwrap();
        assert_eq!(packages.len(), 1);
        let pkg = &packages[0];
        assert_eq!(pkg.name, "gpg-pubkey");
        assert_eq!(pkg.url, None);
        assert!(pkg.licenses.is_empty());
    }

    #[test]
    fn parse_none_description() {
        let output = "some-pkg\t1.0-1.fc40\t(none)\thttps://example.com\tMIT\n";
        let packages = parse_rpm_output(output).unwrap();
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].description, None);
    }

    #[test]
    fn parse_minimal_line() {
        let output = "some-pkg\t1.0\n";
        let packages = parse_rpm_output(output).unwrap();
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].name, "some-pkg");
        assert_eq!(packages[0].version, "1.0");
        assert_eq!(packages[0].description, None);
        assert_eq!(packages[0].url, None);
        assert!(packages[0].licenses.is_empty());
    }

    #[test]
    fn parse_empty_output() {
        let packages = parse_rpm_output("").unwrap();
        assert!(packages.is_empty());
    }

    #[test]
    fn parse_skips_blank_lines() {
        let output = "\nbash\t5.2.26-3.fc40\tThe GNU Bourne Again shell\thttps://www.gnu.org/software/bash\tGPL-3.0-or-later\n\n";
        let packages = parse_rpm_output(output).unwrap();
        assert_eq!(packages.len(), 1);
    }

    #[test]
    fn parse_empty_name_skipped() {
        let output = "\t1.0\tSome package\thttps://example.com\tMIT\n";
        let packages = parse_rpm_output(output).unwrap();
        assert!(packages.is_empty());
    }

    #[test]
    fn parse_missing_version_defaults_to_unknown() {
        let output = "some-pkg\t\tA description\thttps://example.com\tMIT\n";
        let packages = parse_rpm_output(output).unwrap();
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].version, "unknown");
    }
}
