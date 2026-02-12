// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::Path;

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

use syld::discover::{InstalledPackage, PackageSource};
use syld::storage::Storage;

fn syld_with_db(config_home: &Path, data_home: &Path) -> Command {
    let mut cmd: Command = cargo_bin_cmd!("syld").into();
    cmd.env("XDG_CONFIG_HOME", config_home);
    cmd.env("XDG_DATA_HOME", data_home);
    cmd
}

fn seed_scan(data_home: &Path) {
    let db_dir = data_home.join("syld");
    std::fs::create_dir_all(&db_dir).unwrap();
    let storage = Storage::open_path(&db_dir.join("syld.db")).unwrap();

    let packages = vec![
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
        InstalledPackage {
            name: "orphan".to_string(),
            version: "1.0".to_string(),
            description: None,
            url: None,
            source: PackageSource::Pacman,
            licenses: vec![],
        },
    ];
    storage.save_scan(&packages).unwrap();
}

#[test]
fn report_no_scan_shows_message() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    syld_with_db(tmp.path(), data.path())
        .args(["report"])
        .assert()
        .success()
        .stderr(predicate::str::contains("No scan data found"));
}

#[test]
fn report_terminal_shows_table() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    seed_scan(data.path());

    syld_with_db(tmp.path(), data.path())
        .args(["report", "--format", "terminal"])
        .assert()
        .success()
        .stdout(predicate::str::contains("pacman"))
        .stdout(predicate::str::contains("firefox"));
}

#[test]
fn report_json_is_valid() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    seed_scan(data.path());

    let output = syld_with_db(tmp.path(), data.path())
        .args(["report", "--format", "json"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("not valid JSON");
    assert_eq!(parsed["total_packages"], 3);
    assert_eq!(parsed["packages"][0]["name"], "firefox");
}

#[test]
fn report_html_contains_structure() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    seed_scan(data.path());

    syld_with_db(tmp.path(), data.path())
        .args(["report", "--format", "html"])
        .assert()
        .success()
        .stdout(predicate::str::contains("<!DOCTYPE html>"))
        .stdout(predicate::str::contains("<title>syld report</title>"))
        .stdout(predicate::str::contains("firefox"))
        .stdout(predicate::str::contains("kernel.org"));
}
