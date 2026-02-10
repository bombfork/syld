// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashMap;

use comfy_table::{ContentArrangement, Table};

use crate::discover::{InstalledPackage, PackageSource};

/// Sort packages alphabetically by name (case-insensitive), then by source.
pub fn sort_packages(packages: &mut [InstalledPackage]) {
    packages.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.source.cmp(&b.source))
    });
}

/// Print a summary of discovered packages to the terminal.
pub fn print_summary(packages: &[InstalledPackage]) {
    if packages.is_empty() {
        println!("No packages found.");
        return;
    }

    // Group by source
    let mut by_source: HashMap<&PackageSource, Vec<&InstalledPackage>> = HashMap::new();
    for pkg in packages {
        by_source.entry(&pkg.source).or_default().push(pkg);
    }

    println!();

    let mut summary_table = Table::new();
    summary_table.set_content_arrangement(ContentArrangement::Dynamic);
    summary_table.set_header(vec!["Source", "Packages"]);

    let mut sources: Vec<_> = by_source.keys().collect();
    sources.sort();
    for source in sources {
        summary_table.add_row(vec![
            source.to_string(),
            by_source[source].len().to_string(),
        ]);
    }

    println!("{summary_table}");
    println!();

    // Show packages with URLs (likely to have upstream projects)
    let with_url: Vec<_> = packages.iter().filter(|p| p.url.is_some()).collect();

    if with_url.is_empty() {
        return;
    }

    let mut detail_table = Table::new();
    detail_table.set_content_arrangement(ContentArrangement::Dynamic);
    detail_table.set_header(vec!["Package", "Version", "Source", "URL"]);

    // Show first 20 as a preview
    for pkg in with_url.iter().take(20) {
        detail_table.add_row(vec![
            &pkg.name,
            &pkg.version,
            &pkg.source.to_string(),
            pkg.url.as_deref().unwrap_or(""),
        ]);
    }

    println!("{detail_table}");

    if with_url.len() > 20 {
        println!(
            "\n  ... and {} more packages with upstream URLs",
            with_url.len() - 20
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_pkg(name: &str, source: PackageSource) -> InstalledPackage {
        InstalledPackage {
            name: name.to_string(),
            version: "1.0".to_string(),
            description: None,
            url: None,
            source,
            licenses: vec![],
        }
    }

    #[test]
    fn sort_alphabetically_case_insensitive() {
        let mut packages = vec![
            make_pkg("zsh", PackageSource::Pacman),
            make_pkg("Alacritty", PackageSource::Pacman),
            make_pkg("bash", PackageSource::Pacman),
        ];
        sort_packages(&mut packages);
        let names: Vec<_> = packages.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["Alacritty", "bash", "zsh"]);
    }

    #[test]
    fn sort_same_name_by_source() {
        let mut packages = vec![
            make_pkg("firefox", PackageSource::Flatpak),
            make_pkg("firefox", PackageSource::Pacman),
        ];
        sort_packages(&mut packages);
        assert_eq!(packages[0].source, PackageSource::Pacman);
        assert_eq!(packages[1].source, PackageSource::Flatpak);
    }

    #[test]
    fn sort_empty_is_noop() {
        let mut packages: Vec<InstalledPackage> = vec![];
        sort_packages(&mut packages);
        assert!(packages.is_empty());
    }

    #[test]
    fn sort_single_element() {
        let mut packages = vec![make_pkg("vim", PackageSource::Pacman)];
        sort_packages(&mut packages);
        assert_eq!(packages[0].name, "vim");
    }

    #[test]
    fn sort_already_sorted() {
        let mut packages = vec![
            make_pkg("aaa", PackageSource::Pacman),
            make_pkg("bbb", PackageSource::Pacman),
            make_pkg("ccc", PackageSource::Pacman),
        ];
        sort_packages(&mut packages);
        let names: Vec<_> = packages.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["aaa", "bbb", "ccc"]);
    }
}
