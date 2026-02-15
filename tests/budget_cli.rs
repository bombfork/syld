// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::Path;

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

use syld::config::{BudgetConfig, Cadence};
use syld::discover::{InstalledPackage, PackageSource};
use syld::project::{FundingChannel, UpstreamProject};
use syld::storage::Storage;

fn syld_with_db(config_home: &Path, data_home: &Path) -> Command {
    let mut cmd: Command = cargo_bin_cmd!("syld").into();
    cmd.env("XDG_CONFIG_HOME", config_home);
    cmd.env("XDG_DATA_HOME", data_home);
    cmd
}

fn open_storage(data_home: &Path) -> Storage {
    let db_dir = data_home.join("syld");
    std::fs::create_dir_all(&db_dir).unwrap();
    Storage::open_path(&db_dir.join("syld.db")).unwrap()
}

fn seed_scan(data_home: &Path) {
    let storage = open_storage(data_home);
    storage
        .save_scan(&[
            InstalledPackage {
                name: "firefox".to_string(),
                version: "128.0".to_string(),
                description: Some("Web browser".to_string()),
                url: Some("https://github.com/nicotine-plus/nicotine-plus".to_string()),
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
        ])
        .unwrap();
}

fn seed_enrichment_with_funding(data_home: &Path) {
    let storage = open_storage(data_home);
    let project = UpstreamProject {
        name: "nicotine-plus".to_string(),
        repo_url: Some("https://github.com/nicotine-plus/nicotine-plus".to_string()),
        homepage: None,
        licenses: vec!["GPL-3.0".to_string()],
        funding: vec![FundingChannel {
            platform: "GitHub Sponsors".to_string(),
            url: "https://github.com/sponsors/nicotine-plus".to_string(),
        }],
        bug_tracker: None,
        contributing_url: None,
        is_open_source: Some(true),
        documentation_url: None,
        good_first_issues_url: None,
        stars: Some(500),
    };
    storage
        .save_enrichment("https://github.com/nicotine-plus/nicotine-plus", &project)
        .unwrap();
}

#[test]
fn budget_set_persists() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();

    syld_with_db(tmp.path(), data.path())
        .args(["budget", "set", "10"])
        .assert()
        .success()
        .stderr(predicate::str::contains("Budget set:"));

    syld_with_db(tmp.path(), data.path())
        .args(["budget", "show"])
        .assert()
        .success()
        .stdout(predicate::str::contains("USD"))
        .stdout(predicate::str::contains("10.00"))
        .stdout(predicate::str::contains("monthly"));
}

#[test]
fn budget_set_yearly() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();

    syld_with_db(tmp.path(), data.path())
        .args(["budget", "set", "120", "--cadence", "yearly"])
        .assert()
        .success()
        .stderr(predicate::str::contains("yearly"));

    syld_with_db(tmp.path(), data.path())
        .args(["budget", "show"])
        .assert()
        .success()
        .stdout(predicate::str::contains("120.00"))
        .stdout(predicate::str::contains("yearly"));
}

#[test]
fn budget_show_when_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();

    syld_with_db(tmp.path(), data.path())
        .args(["budget", "show"])
        .assert()
        .success()
        .stderr(predicate::str::contains("No budget configured"));
}

#[test]
fn budget_plan_no_budget() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    seed_scan(data.path());

    syld_with_db(tmp.path(), data.path())
        .args(["budget", "plan"])
        .assert()
        .success()
        .stderr(predicate::str::contains("No budget configured"));
}

#[test]
fn budget_plan_no_fundable_projects() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    seed_scan(data.path());

    // Set a budget but don't seed any enrichment with funding
    let storage = open_storage(data.path());
    storage
        .save_budget(&BudgetConfig {
            amount: Some(10.0),
            currency: "USD".to_string(),
            cadence: Cadence::Monthly,
        })
        .unwrap();

    // Seed enrichment without funding channels
    let project = UpstreamProject {
        name: "nicotine-plus".to_string(),
        repo_url: Some("https://github.com/nicotine-plus/nicotine-plus".to_string()),
        homepage: None,
        licenses: vec![],
        funding: vec![], // no funding
        bug_tracker: None,
        contributing_url: None,
        is_open_source: None,
        documentation_url: None,
        good_first_issues_url: None,
        stars: None,
    };
    storage
        .save_enrichment("https://github.com/nicotine-plus/nicotine-plus", &project)
        .unwrap();

    syld_with_db(tmp.path(), data.path())
        .args(["budget", "plan"])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "No projects with funding channels found",
        ));
}

#[test]
fn budget_plan_equal() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    seed_scan(data.path());
    seed_enrichment_with_funding(data.path());

    let storage = open_storage(data.path());
    storage
        .save_budget(&BudgetConfig {
            amount: Some(10.0),
            currency: "USD".to_string(),
            cadence: Cadence::Monthly,
        })
        .unwrap();

    syld_with_db(tmp.path(), data.path())
        .args(["budget", "plan"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Donation plan"))
        .stdout(predicate::str::contains("Strategy: equal"))
        .stdout(predicate::str::contains("USD"))
        .stdout(predicate::str::contains("monthly"))
        .stdout(predicate::str::contains(
            "github.com/sponsors/nicotine-plus",
        ));
}

#[test]
fn budget_plan_weighted() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    seed_scan(data.path());
    seed_enrichment_with_funding(data.path());

    let storage = open_storage(data.path());
    storage
        .save_budget(&BudgetConfig {
            amount: Some(10.0),
            currency: "USD".to_string(),
            cadence: Cadence::Monthly,
        })
        .unwrap();

    syld_with_db(tmp.path(), data.path())
        .args(["budget", "plan", "--strategy", "weighted"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Strategy: weighted"))
        .stdout(predicate::str::contains("USD"));
}
