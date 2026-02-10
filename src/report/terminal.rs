// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashMap;

use comfy_table::{ContentArrangement, Table};

use crate::discover::{InstalledPackage, PackageSource};

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

    for (source, pkgs) in &by_source {
        summary_table.add_row(vec![source.to_string(), pkgs.len().to_string()]);
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
