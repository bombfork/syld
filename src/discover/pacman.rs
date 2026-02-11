// SPDX-License-Identifier: GPL-3.0-or-later

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;

use super::{Discoverer, InstalledPackage, PackageSource};

/// Discovers packages installed via pacman by reading the local database directly.
///
/// The pacman database lives at /var/lib/pacman/local/ and contains one directory
/// per installed package. Each directory has a "desc" file with package metadata.
pub struct PacmanDiscoverer;

const PACMAN_DB_PATH: &str = "/var/lib/pacman/local";

impl Discoverer for PacmanDiscoverer {
    fn name(&self) -> &str {
        "pacman"
    }

    fn is_available(&self) -> bool {
        Path::new(PACMAN_DB_PATH).is_dir()
    }

    fn discover(&self) -> Result<Vec<InstalledPackage>> {
        let db_path = Path::new(PACMAN_DB_PATH);

        let desc_paths: Vec<_> = fs::read_dir(db_path)
            .context("Failed to read pacman database directory")?
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let desc_path = entry.path().join("desc");
                desc_path.is_file().then_some(desc_path)
            })
            .collect();

        let pb = ProgressBar::new(desc_paths.len() as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("  {bar:30} {pos}/{len} packages")
                .unwrap(),
        );

        let packages: Vec<InstalledPackage> = desc_paths
            .par_iter()
            .filter_map(|desc_path| {
                let result = parse_desc(desc_path);
                pb.inc(1);
                match result {
                    Ok(pkg) => Some(pkg),
                    Err(e) => {
                        pb.suspend(|| {
                            eprintln!("  Warning: failed to parse {}: {}", desc_path.display(), e);
                        });
                        None
                    }
                }
            })
            .collect();

        pb.finish_and_clear();

        Ok(packages)
    }
}

/// Read and parse a pacman desc file into an InstalledPackage.
fn parse_desc(path: &Path) -> Result<InstalledPackage> {
    let content =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    parse_desc_content(&content)
}

/// Parse the content of a pacman desc file into an InstalledPackage.
///
/// The desc file format uses %FIELD% headers followed by values on subsequent lines,
/// separated by blank lines.
fn parse_desc_content(content: &str) -> Result<InstalledPackage> {
    let mut name = None;
    let mut version = None;
    let mut description = None;
    let mut url = None;
    let mut licenses = Vec::new();

    let mut current_field: Option<&str> = None;

    for line in content.lines() {
        let line = line.trim();

        if line.starts_with('%') && line.ends_with('%') {
            current_field = Some(line);
            continue;
        }

        if line.is_empty() {
            current_field = None;
            continue;
        }

        match current_field {
            Some("%NAME%") => name = Some(line.to_string()),
            Some("%VERSION%") => version = Some(line.to_string()),
            Some("%DESC%") => description = Some(line.to_string()),
            Some("%URL%") => url = Some(line.to_string()),
            Some("%LICENSE%") => licenses.push(line.to_string()),
            _ => {}
        }
    }

    Ok(InstalledPackage {
        name: name.context("Missing %NAME% in desc file")?,
        version: version.context("Missing %VERSION% in desc file")?,
        description,
        url,
        source: PackageSource::Pacman,
        licenses,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_full_desc() {
        let content = "\
%NAME%
firefox

%VERSION%
128.0-1

%DESC%
Fast, Private & Safe Web Browser

%URL%
https://www.mozilla.org/firefox/

%LICENSE%
MPL-2.0
GPL-2.0-only
LGPL-2.1-only

%DEPENDS%
dbus-glib
gtk3
";
        let pkg = parse_desc_content(content).unwrap();
        assert_eq!(pkg.name, "firefox");
        assert_eq!(pkg.version, "128.0-1");
        assert_eq!(
            pkg.description.as_deref(),
            Some("Fast, Private & Safe Web Browser")
        );
        assert_eq!(pkg.url.as_deref(), Some("https://www.mozilla.org/firefox/"));
        assert_eq!(pkg.source, PackageSource::Pacman);
        assert_eq!(
            pkg.licenses,
            vec!["MPL-2.0", "GPL-2.0-only", "LGPL-2.1-only"]
        );
    }

    #[test]
    fn parse_minimal_desc() {
        let content = "\
%NAME%
coreutils

%VERSION%
9.5-1
";
        let pkg = parse_desc_content(content).unwrap();
        assert_eq!(pkg.name, "coreutils");
        assert_eq!(pkg.version, "9.5-1");
        assert_eq!(pkg.description, None);
        assert_eq!(pkg.url, None);
        assert!(pkg.licenses.is_empty());
    }

    #[test]
    fn parse_missing_name_errors() {
        let content = "\
%VERSION%
1.0.0
";
        let err = parse_desc_content(content).unwrap_err();
        assert!(err.to_string().contains("NAME"));
    }

    #[test]
    fn parse_missing_version_errors() {
        let content = "\
%NAME%
something
";
        let err = parse_desc_content(content).unwrap_err();
        assert!(err.to_string().contains("VERSION"));
    }

    #[test]
    fn parse_ignores_unknown_fields() {
        let content = "\
%NAME%
pkg

%VERSION%
1.0

%BUILDDATE%
1700000000

%PACKAGER%
Someone <someone@example.com>
";
        let pkg = parse_desc_content(content).unwrap();
        assert_eq!(pkg.name, "pkg");
        assert_eq!(pkg.version, "1.0");
    }

    #[test]
    fn parse_single_license() {
        let content = "\
%NAME%
mit-pkg

%VERSION%
0.1

%LICENSE%
MIT
";
        let pkg = parse_desc_content(content).unwrap();
        assert_eq!(pkg.licenses, vec!["MIT"]);
    }
}
