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
fn hook_list_succeeds_and_shows_pacman() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    syld_with_db(tmp.path(), data.path())
        .args(["hook", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("pacman-post-transaction"));
}

#[test]
fn hook_run_unknown_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    syld_with_db(tmp.path(), data.path())
        .args(["hook", "run", "unknown-hook"])
        .write_stdin("")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Unknown hook"));
}

#[test]
fn hook_run_pacman_empty_stdin_succeeds() {
    // Skip on non-Arch systems where pacman is not available
    if !Path::new("/var/lib/pacman/local").is_dir() {
        eprintln!("Skipping: pacman not available on this system");
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    syld_with_db(tmp.path(), data.path())
        .args(["hook", "run", "pacman-post-transaction"])
        .write_stdin("")
        .assert()
        .success();
}

#[test]
fn hook_list_succeeds_and_shows_apt() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    syld_with_db(tmp.path(), data.path())
        .args(["hook", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("apt-post-invoke"));
}

#[test]
fn hook_run_apt_empty_stdin_succeeds() {
    // Skip on non-Debian systems where dpkg is not available
    if !Path::new("/var/lib/dpkg/status").is_file() {
        eprintln!("Skipping: dpkg not available on this system");
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    syld_with_db(tmp.path(), data.path())
        .args(["hook", "run", "apt-post-invoke"])
        .write_stdin("")
        .assert()
        .success();
}

#[test]
fn hook_list_shows_availability_status() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let output = syld_with_db(tmp.path(), data.path())
        .args(["hook", "list"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    // Should show either "available" or "not available"
    assert!(
        stdout.contains("available"),
        "hook list should show availability status"
    );
}
