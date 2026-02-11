// SPDX-License-Identifier: GPL-3.0-or-later

use std::fs;

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

fn syld(config_home: &std::path::Path) -> Command {
    let mut cmd: Command = cargo_bin_cmd!("syld").into();
    cmd.env("XDG_CONFIG_HOME", config_home);
    cmd
}

#[test]
fn config_show_outputs_valid_toml() {
    let tmp = tempfile::tempdir().unwrap();
    syld(tmp.path())
        .args(["config", "show"])
        .assert()
        .success()
        .stdout(predicate::str::contains("enrich"))
        .stdout(predicate::str::contains("[budget]"))
        .stdout(predicate::str::contains("currency"));
}

#[test]
fn config_show_default_values() {
    let tmp = tempfile::tempdir().unwrap();
    syld(tmp.path())
        .args(["config", "show"])
        .assert()
        .success()
        .stdout(predicate::str::contains("enrich = false"))
        .stdout(predicate::str::contains("currency = \"USD\""))
        .stdout(predicate::str::contains("cadence = \"monthly\""));
}

#[test]
fn config_show_prints_path_to_stderr() {
    let tmp = tempfile::tempdir().unwrap();
    syld(tmp.path())
        .args(["config", "show"])
        .assert()
        .success()
        .stderr(predicate::str::contains("config.toml"));
}

#[test]
fn config_show_reflects_custom_config() {
    let tmp = tempfile::tempdir().unwrap();
    let config_dir = tmp.path().join("syld");
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(
        config_dir.join("config.toml"),
        "enrich = true\n\n[budget]\namount = 42.0\ncurrency = \"EUR\"\ncadence = \"yearly\"\n",
    )
    .unwrap();

    syld(tmp.path())
        .args(["config", "show"])
        .assert()
        .success()
        .stdout(predicate::str::contains("enrich = true"))
        .stdout(predicate::str::contains("amount = 42.0"))
        .stdout(predicate::str::contains("currency = \"EUR\""))
        .stdout(predicate::str::contains("cadence = \"yearly\""));
}

#[test]
fn config_show_output_is_valid_toml() {
    let tmp = tempfile::tempdir().unwrap();
    let output = syld(tmp.path()).args(["config", "show"]).output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let _: toml::Value = toml::from_str(&stdout).expect("config show output is not valid TOML");
}

#[test]
fn bare_config_defaults_to_show() {
    let tmp = tempfile::tempdir().unwrap();

    let show_output = syld(tmp.path()).args(["config", "show"]).output().unwrap();

    let bare_output = syld(tmp.path()).args(["config"]).output().unwrap();

    assert_eq!(show_output.stdout, bare_output.stdout);
}

#[test]
fn config_edit_creates_file_when_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let config_path = tmp.path().join("syld").join("config.toml");

    assert!(!config_path.exists());

    // Use `true` as the editor — it exits 0 immediately without modifying anything
    syld(tmp.path())
        .env("VISUAL", "true")
        .args(["config", "edit"])
        .assert()
        .success()
        .stderr(predicate::str::contains("Created default config"));

    assert!(config_path.exists());
    let content = fs::read_to_string(&config_path).unwrap();
    let _: toml::Value = toml::from_str(&content).expect("created config is not valid TOML");
}

#[test]
fn config_edit_preserves_existing_file() {
    let tmp = tempfile::tempdir().unwrap();
    let config_dir = tmp.path().join("syld");
    fs::create_dir_all(&config_dir).unwrap();
    let config_path = config_dir.join("config.toml");
    let custom = "enrich = true\n\n[budget]\namount = 99.0\n";
    fs::write(&config_path, custom).unwrap();

    syld(tmp.path())
        .env("VISUAL", "true")
        .args(["config", "edit"])
        .assert()
        .success()
        // Should NOT say "Created default config" since it already exists
        .stderr(predicate::str::contains("Created default config").not());

    // File content should be unchanged (editor was `true`, a no-op)
    let after = fs::read_to_string(&config_path).unwrap();
    assert_eq!(after, custom);
}

#[test]
fn config_edit_fails_with_bad_editor() {
    let tmp = tempfile::tempdir().unwrap();
    syld(tmp.path())
        .env("VISUAL", "false") // `false` exits with status 1
        .args(["config", "edit"])
        .assert()
        .failure();
}
