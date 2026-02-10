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

/// A group of packages that share the same upstream project URL.
#[derive(Debug)]
pub struct ProjectGroup<'a> {
    /// Normalized upstream URL used as the grouping key
    pub url: String,
    /// All packages belonging to this project
    pub packages: Vec<&'a InstalledPackage>,
}

/// Normalize a URL for grouping purposes.
///
/// Strips trailing slashes, the scheme, and a leading "www." so that
/// `https://www.qemu.org/` and `https://qemu.org` group together.
pub fn normalize_url(url: &str) -> String {
    let s = url.trim().trim_end_matches('/');
    let s = s
        .strip_prefix("https://")
        .or_else(|| s.strip_prefix("http://"))
        .unwrap_or(s);
    let s = s.strip_prefix("www.").unwrap_or(s);
    s.to_lowercase()
}

/// Group packages by their normalized upstream URL.
///
/// Packages without a URL are collected under a single empty-string key.
/// The returned groups are sorted alphabetically by URL.
pub fn group_by_project<'a>(packages: &'a [InstalledPackage]) -> Vec<ProjectGroup<'a>> {
    let mut map: HashMap<String, Vec<&'a InstalledPackage>> = HashMap::new();

    for pkg in packages {
        let key = match &pkg.url {
            Some(url) => normalize_url(url),
            None => String::new(),
        };
        map.entry(key).or_default().push(pkg);
    }

    let mut groups: Vec<ProjectGroup<'a>> = map
        .into_iter()
        .map(|(url, packages)| ProjectGroup { url, packages })
        .collect();

    groups.sort_by(|a, b| a.url.cmp(&b.url));
    groups
}

/// Return a page of items from a slice, plus how many remain.
///
/// A `limit` of 0 means "show all".
pub fn paginate<T>(items: &[T], limit: usize) -> (&[T], usize) {
    if limit == 0 || limit >= items.len() {
        (items, 0)
    } else {
        (&items[..limit], items.len() - limit)
    }
}

/// Print a summary of discovered packages to the terminal.
///
/// `limit` controls how many project groups to display (0 = all).
pub fn print_summary(packages: &[InstalledPackage], limit: usize) {
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

    // Group by upstream project
    let groups = group_by_project(packages);
    let with_url: Vec<_> = groups.iter().filter(|g| !g.url.is_empty()).collect();

    if with_url.is_empty() {
        return;
    }

    println!(
        "{} packages grouped into {} upstream projects\n",
        packages.len(),
        with_url.len()
    );

    let (page, remaining) = paginate(&with_url, limit);

    let mut detail_table = Table::new();
    detail_table.set_content_arrangement(ContentArrangement::Dynamic);
    detail_table.set_header(vec!["Project URL", "Packages"]);

    for group in page {
        let pkg_names: Vec<_> = group.packages.iter().map(|p| p.name.as_str()).collect();
        detail_table.add_row(vec![group.url.as_str(), &pkg_names.join(", ")]);
    }

    println!("{detail_table}");

    if remaining > 0 {
        println!(
            "\n  ... and {} more projects (use --limit 0 to show all)",
            remaining
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

    fn make_pkg_with_url(name: &str, url: &str) -> InstalledPackage {
        InstalledPackage {
            name: name.to_string(),
            version: "1.0".to_string(),
            description: None,
            url: Some(url.to_string()),
            source: PackageSource::Pacman,
            licenses: vec![],
        }
    }

    // --- sort tests ---

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

    // --- normalize_url tests ---

    #[test]
    fn normalize_strips_trailing_slash() {
        assert_eq!(normalize_url("https://qemu.org/"), "qemu.org");
    }

    #[test]
    fn normalize_strips_scheme() {
        assert_eq!(normalize_url("https://example.com"), "example.com");
        assert_eq!(normalize_url("http://example.com"), "example.com");
    }

    #[test]
    fn normalize_strips_www() {
        assert_eq!(normalize_url("https://www.qemu.org/"), "qemu.org");
    }

    #[test]
    fn normalize_lowercases() {
        assert_eq!(
            normalize_url("https://GitHub.com/Foo/Bar"),
            "github.com/foo/bar"
        );
    }

    #[test]
    fn normalize_preserves_path() {
        assert_eq!(
            normalize_url("https://github.com/user/repo"),
            "github.com/user/repo"
        );
    }

    // --- group_by_project tests ---

    #[test]
    fn group_merges_same_url() {
        let packages = vec![
            make_pkg_with_url("qemu-system-x86", "https://www.qemu.org/"),
            make_pkg_with_url("qemu-user", "https://qemu.org"),
            make_pkg_with_url("qemu-img", "https://www.qemu.org"),
        ];
        let groups = group_by_project(&packages);
        let with_url: Vec<_> = groups.iter().filter(|g| !g.url.is_empty()).collect();
        assert_eq!(with_url.len(), 1);
        assert_eq!(with_url[0].packages.len(), 3);
    }

    #[test]
    fn group_separates_different_urls() {
        let packages = vec![
            make_pkg_with_url("firefox", "https://www.mozilla.org/firefox/"),
            make_pkg_with_url("bash", "https://www.gnu.org/software/bash"),
        ];
        let groups = group_by_project(&packages);
        let with_url: Vec<_> = groups.iter().filter(|g| !g.url.is_empty()).collect();
        assert_eq!(with_url.len(), 2);
    }

    #[test]
    fn group_collects_no_url_packages() {
        let packages = vec![
            make_pkg("orphan1", PackageSource::Pacman),
            make_pkg("orphan2", PackageSource::Pacman),
        ];
        let groups = group_by_project(&packages);
        assert_eq!(groups.len(), 1);
        assert!(groups[0].url.is_empty());
        assert_eq!(groups[0].packages.len(), 2);
    }

    #[test]
    fn group_sorted_alphabetically() {
        let packages = vec![
            make_pkg_with_url("pkg-z", "https://z-project.org"),
            make_pkg_with_url("pkg-a", "https://a-project.org"),
        ];
        let groups = group_by_project(&packages);
        let urls: Vec<_> = groups.iter().map(|g| g.url.as_str()).collect();
        assert_eq!(urls, vec!["a-project.org", "z-project.org"]);
    }

    // --- paginate tests ---

    #[test]
    fn paginate_returns_all_when_limit_zero() {
        let items = vec![1, 2, 3, 4, 5];
        let (page, remaining) = paginate(&items, 0);
        assert_eq!(page, &[1, 2, 3, 4, 5]);
        assert_eq!(remaining, 0);
    }

    #[test]
    fn paginate_returns_all_when_limit_exceeds_len() {
        let items = vec![1, 2, 3];
        let (page, remaining) = paginate(&items, 10);
        assert_eq!(page, &[1, 2, 3]);
        assert_eq!(remaining, 0);
    }

    #[test]
    fn paginate_truncates_with_remaining() {
        let items = vec![1, 2, 3, 4, 5];
        let (page, remaining) = paginate(&items, 3);
        assert_eq!(page, &[1, 2, 3]);
        assert_eq!(remaining, 2);
    }

    #[test]
    fn paginate_limit_equals_len() {
        let items = vec![1, 2, 3];
        let (page, remaining) = paginate(&items, 3);
        assert_eq!(page, &[1, 2, 3]);
        assert_eq!(remaining, 0);
    }

    #[test]
    fn paginate_empty_slice() {
        let items: Vec<i32> = vec![];
        let (page, remaining) = paginate(&items, 5);
        assert!(page.is_empty());
        assert_eq!(remaining, 0);
    }

    #[test]
    fn paginate_limit_one() {
        let items = vec![10, 20, 30];
        let (page, remaining) = paginate(&items, 1);
        assert_eq!(page, &[10]);
        assert_eq!(remaining, 2);
    }

    // --- group tests ---

    #[test]
    fn group_mixed_url_and_no_url() {
        let packages = vec![
            make_pkg_with_url("firefox", "https://mozilla.org"),
            make_pkg("orphan", PackageSource::Pacman),
        ];
        let groups = group_by_project(&packages);
        assert_eq!(groups.len(), 2);
        // Empty-string key sorts first
        let with_url: Vec<_> = groups.iter().filter(|g| !g.url.is_empty()).collect();
        assert_eq!(with_url.len(), 1);
        assert_eq!(with_url[0].packages[0].name, "firefox");
    }
}
