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
fn setup_help_shows_wizard_description() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    syld_with_db(tmp.path(), data.path())
        .args(["setup", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("setup wizard"));
}

#[test]
fn install_service_help_shows_flags() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    syld_with_db(tmp.path(), data.path())
        .args(["install", "service", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--frequency"))
        .stdout(predicate::str::contains("--enable"));
}

#[test]
fn install_hook_help_shows_subcommand() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    syld_with_db(tmp.path(), data.path())
        .args(["install", "hook", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("hook"));
}

#[test]
fn install_service_writes_files_with_correct_content() {
    let config_home = tempfile::tempdir().unwrap();
    let data_home = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();

    // Set HOME so systemd_user_dir resolves to our temp dir
    let systemd_dir = config_home.path().join("systemd/user");

    syld_with_db(config_home.path(), data_home.path())
        .env("HOME", home.path())
        .args(["install", "service", "--frequency", "weekly"])
        .assert()
        .success();

    let service_path = systemd_dir.join("syld.service");
    let timer_path = systemd_dir.join("syld.timer");

    assert!(service_path.exists(), "syld.service should be written");
    assert!(timer_path.exists(), "syld.timer should be written");

    let service_content = std::fs::read_to_string(&service_path).unwrap();
    assert!(
        service_content.contains("ExecStart="),
        "service should have ExecStart"
    );
    assert!(
        service_content.contains("syld scan"),
        "service should run syld scan"
    );

    let timer_content = std::fs::read_to_string(&timer_path).unwrap();
    assert!(
        timer_content.contains("OnCalendar=weekly"),
        "timer should use weekly calendar"
    );
}

#[test]
fn install_hook_unknown_name_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    syld_with_db(tmp.path(), data.path())
        .args(["install", "hook", "nonexistent"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Unknown hook"));
}

#[test]
fn scan_silent_produces_no_stdout() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    syld_with_db(tmp.path(), data.path())
        .args(["scan", "--silent"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}

#[test]
fn scan_help_shows_silent_flag() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    syld_with_db(tmp.path(), data.path())
        .args(["scan", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--silent"));
}

#[test]
fn top_level_help_shows_setup_in_workflow() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    syld_with_db(tmp.path(), data.path())
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("syld setup"));
}
