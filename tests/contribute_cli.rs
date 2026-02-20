// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::Path;

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

fn syld_with_db(config_home: &Path, data_home: &Path) -> Command {
    let mut cmd: Command = cargo_bin_cmd!("syld").into();
    cmd.env("XDG_CONFIG_HOME", config_home);
    cmd.env("XDG_DATA_HOME", data_home);
    cmd
}

#[test]
fn contribute_help_shows_star_subcommand() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    syld_with_db(tmp.path(), data.path())
        .args(["contribute", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("star"));
}

#[test]
fn contribute_star_help_shows_project_flag() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    syld_with_db(tmp.path(), data.path())
        .args(["contribute", "star", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--project"));
}

#[test]
fn contribute_star_without_project_no_scan() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    // With an empty database and no scan data, it should tell the user to scan first.
    syld_with_db(tmp.path(), data.path())
        .args(["contribute", "star"])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "No unstarred GitHub projects found",
        ));
}

#[test]
fn contribute_help_shows_issue_subcommand() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    syld_with_db(tmp.path(), data.path())
        .args(["contribute", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("issue"));
}

#[test]
fn contribute_issue_help_shows_project_flag() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    syld_with_db(tmp.path(), data.path())
        .args(["contribute", "issue", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--project"));
}

#[test]
fn contribute_issue_without_project_no_scan() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    // With an empty database and no scan data, it should tell the user to scan first.
    syld_with_db(tmp.path(), data.path())
        .args(["contribute", "issue"])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "No projects with good first issues found",
        ));
}

#[test]
fn contribute_help_shows_donate_subcommand() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    syld_with_db(tmp.path(), data.path())
        .args(["contribute", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("donate"));
}

#[test]
fn contribute_donate_help_shows_project_flag() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    syld_with_db(tmp.path(), data.path())
        .args(["contribute", "donate", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--project"));
}

#[test]
fn contribute_donate_without_project_no_scan() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    // With an empty database and no scan data, it should tell the user to scan first.
    syld_with_db(tmp.path(), data.path())
        .args(["contribute", "donate"])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "No projects with funding channels found",
        ));
}

#[test]
fn contribute_help_shows_docs_subcommand() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    syld_with_db(tmp.path(), data.path())
        .args(["contribute", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("docs"));
}

#[test]
fn contribute_docs_help_shows_project_flag() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    syld_with_db(tmp.path(), data.path())
        .args(["contribute", "docs", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--project"));
}

#[test]
fn contribute_docs_without_project_no_scan() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    // With an empty database and no scan data, it should tell the user to scan first.
    syld_with_db(tmp.path(), data.path())
        .args(["contribute", "docs"])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "No projects with contributing guides found",
        ));
}

#[test]
fn contribute_docs_with_project_prints_url() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    // Unset GitHub tokens so `is_gh_available()` returns false, avoiding
    // flaky API calls in CI where GITHUB_TOKEN is set automatically.
    syld_with_db(tmp.path(), data.path())
        .env_remove("GH_TOKEN")
        .env_remove("GITHUB_TOKEN")
        .args(["contribute", "docs", "--project", "curl/curl"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Contributing guide:"))
        .stdout(predicate::str::contains(
            "https://github.com/curl/curl/blob/HEAD/CONTRIBUTING.md",
        ));
}
