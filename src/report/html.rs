// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::discover::{InstalledPackage, PackageSource};
use crate::report::terminal::{group_by_project, sort_packages};

/// Escape HTML special characters.
fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Format a package name with an optional source badge.
///
/// Badges are only shown when the report contains packages from multiple
/// sources, since a single-source report would just add visual noise.
fn format_package_html(pkg: &InstalledPackage, show_badge: bool) -> String {
    if show_badge {
        format!(
            "{}<span class=\"badge\">{}</span>",
            escape_html(&pkg.name),
            escape_html(&pkg.source.to_string()),
        )
    } else {
        escape_html(&pkg.name)
    }
}

/// Generate an HTML report and print it to stdout.
pub fn print_html(packages: &[InstalledPackage], timestamp: DateTime<Utc>) {
    let mut sorted = packages.to_vec();
    sort_packages(&mut sorted);

    let mut by_source: HashMap<&PackageSource, usize> = HashMap::new();
    for pkg in &sorted {
        *by_source.entry(&pkg.source).or_default() += 1;
    }
    let mut sources: Vec<_> = by_source.iter().collect();
    sources.sort_by_key(|(s, _)| **s);

    let has_multiple_sources = sources.len() > 1;

    let groups = group_by_project(&sorted);

    let mut html = String::new();

    html.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n");
    html.push_str("<meta charset=\"utf-8\">\n");
    html.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    html.push_str("<title>syld report</title>\n");
    html.push_str("<style>\n");
    html.push_str(
        "body { font-family: system-ui, sans-serif; max-width: 960px; margin: 2rem auto; padding: 0 1rem; color: #1a1a1a; }\n",
    );
    html.push_str("h1, h2 { margin-top: 2rem; }\n");
    html.push_str("table { border-collapse: collapse; width: 100%; margin: 1rem 0; }\n");
    html.push_str(
        "th, td { text-align: left; padding: 0.5rem 1rem; border-bottom: 1px solid #ddd; }\n",
    );
    html.push_str("th { background: #f5f5f5; }\n");
    html.push_str("tr:hover { background: #fafafa; }\n");
    html.push_str(".meta { color: #666; font-size: 0.9rem; }\n");
    html.push_str(".badge { display: inline-block; font-size: 0.7rem; padding: 0.1rem 0.4rem; border-radius: 3px; background: #e8e8e8; color: #555; margin-left: 0.3rem; vertical-align: middle; }\n");
    html.push_str("</style>\n");
    html.push_str("</head>\n<body>\n");

    let with_url_count = groups.iter().filter(|g| !g.url.is_empty()).count();
    let without_url_count = sorted.iter().filter(|p| p.url.is_none()).count();

    html.push_str("<h1>syld report</h1>\n");
    html.push_str(&format!(
        "<p class=\"meta\">Scan date: {}</p>\n",
        escape_html(&timestamp.format("%Y-%m-%d %H:%M UTC").to_string())
    ));
    html.push_str(&format!(
        "<p class=\"meta\">Total packages: {}</p>\n",
        sorted.len()
    ));
    html.push_str(&format!(
        "<p class=\"meta\">Upstream projects: {}</p>\n",
        with_url_count
    ));
    html.push_str(&format!(
        "<p class=\"meta\">Packages without URL: {}</p>\n",
        without_url_count
    ));

    // Source summary
    html.push_str("<h2>Sources</h2>\n");
    html.push_str("<table>\n<tr><th>Source</th><th>Packages</th></tr>\n");
    for (source, count) in &sources {
        html.push_str(&format!(
            "<tr><td>{}</td><td>{}</td></tr>\n",
            escape_html(&source.to_string()),
            count,
        ));
    }
    html.push_str("</table>\n");

    // Projects
    if !groups.is_empty() {
        html.push_str("<h2>Upstream projects</h2>\n");
        html.push_str(&format!(
            "<p class=\"meta\">{} packages grouped into {} projects</p>\n",
            sorted.len(),
            with_url_count
        ));
        html.push_str("<table>\n<tr><th>Project</th><th>Packages</th></tr>\n");

        for group in &groups {
            let pkg_names: Vec<_> = group
                .packages
                .iter()
                .map(|p| format_package_html(p, has_multiple_sources))
                .collect();
            let url_cell = if group.url.is_empty() {
                "<em>no project URL</em>".to_string()
            } else {
                escape_html(&group.url)
            };
            html.push_str(&format!(
                "<tr><td>{}</td><td>{}</td></tr>\n",
                url_cell,
                pkg_names.join(", "),
            ));
        }

        html.push_str("</table>\n");
    }

    html.push_str("</body>\n</html>\n");

    print!("{html}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discover::PackageSource;

    fn sample_packages() -> Vec<InstalledPackage> {
        vec![
            InstalledPackage {
                name: "firefox".to_string(),
                version: "128.0".to_string(),
                description: Some("Web browser".to_string()),
                url: Some("https://www.mozilla.org/firefox/".to_string()),
                source: PackageSource::Pacman,
                licenses: vec!["MPL-2.0".to_string()],
            },
            InstalledPackage {
                name: "linux".to_string(),
                version: "6.9.7".to_string(),
                description: None,
                url: Some("https://kernel.org".to_string()),
                source: PackageSource::Pacman,
                licenses: vec!["GPL-2.0".to_string()],
            },
        ]
    }

    #[test]
    fn html_escapes_special_chars() {
        assert_eq!(escape_html("<script>"), "&lt;script&gt;");
        assert_eq!(escape_html("a&b"), "a&amp;b");
        assert_eq!(escape_html("\"quoted\""), "&quot;quoted&quot;");
    }

    #[test]
    fn html_contains_expected_structure() {
        let packages = sample_packages();
        let timestamp = "2025-01-15T10:30:00Z".parse::<DateTime<Utc>>().unwrap();

        let mut sorted = packages.to_vec();
        sort_packages(&mut sorted);

        let groups = group_by_project(&sorted);
        let with_url: Vec<_> = groups.iter().filter(|g| !g.url.is_empty()).collect();

        assert_eq!(sorted.len(), 2);
        assert_eq!(with_url.len(), 2);
        assert!(
            timestamp
                .format("%Y-%m-%d %H:%M UTC")
                .to_string()
                .contains("2025")
        );
    }

    #[test]
    fn html_empty_packages() {
        let packages: Vec<InstalledPackage> = vec![];
        let sorted = packages.clone();
        let groups = group_by_project(&sorted);
        let with_url: Vec<_> = groups.iter().filter(|g| !g.url.is_empty()).collect();
        assert!(with_url.is_empty());
    }

    #[test]
    fn format_package_without_badge() {
        let pkg = InstalledPackage {
            name: "firefox".to_string(),
            version: "128.0".to_string(),
            description: None,
            url: None,
            source: PackageSource::Pacman,
            licenses: vec![],
        };
        assert_eq!(format_package_html(&pkg, false), "firefox");
    }

    #[test]
    fn format_package_with_badge() {
        let pkg = InstalledPackage {
            name: "firefox".to_string(),
            version: "128.0".to_string(),
            description: None,
            url: None,
            source: PackageSource::Flatpak,
            licenses: vec![],
        };
        let html = format_package_html(&pkg, true);
        assert!(html.contains("firefox"));
        assert!(html.contains("flatpak"));
        assert!(html.contains("badge"));
    }

    #[test]
    fn format_package_escapes_name() {
        let pkg = InstalledPackage {
            name: "<script>".to_string(),
            version: "1.0".to_string(),
            description: None,
            url: None,
            source: PackageSource::Pacman,
            licenses: vec![],
        };
        let html = format_package_html(&pkg, true);
        assert!(html.contains("&lt;script&gt;"));
        assert!(!html.contains("<script>"));
    }
}
