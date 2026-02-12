// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::discover::InstalledPackage;

/// A JSON-serializable report of a scan.
#[derive(Serialize)]
pub struct JsonReport {
    pub scan_timestamp: DateTime<Utc>,
    pub total_packages: usize,
    pub packages: Vec<InstalledPackage>,
}

/// Generate a JSON report and print it to stdout.
pub fn print_json(packages: &[InstalledPackage], timestamp: DateTime<Utc>) -> Result<()> {
    let report = JsonReport {
        scan_timestamp: timestamp,
        total_packages: packages.len(),
        packages: packages.to_vec(),
    };

    let json = serde_json::to_string_pretty(&report)?;
    println!("{json}");
    Ok(())
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
    fn json_report_structure() {
        let packages = sample_packages();
        let timestamp = "2025-01-15T10:30:00Z".parse::<DateTime<Utc>>().unwrap();

        let report = JsonReport {
            scan_timestamp: timestamp,
            total_packages: packages.len(),
            packages: packages.clone(),
        };

        let json = serde_json::to_string_pretty(&report).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["total_packages"], 2);
        assert_eq!(parsed["packages"][0]["name"], "firefox");
        assert_eq!(parsed["packages"][0]["version"], "128.0");
        assert_eq!(parsed["packages"][0]["source"], "Pacman");
        assert_eq!(parsed["packages"][0]["licenses"][0], "MPL-2.0");
        assert_eq!(parsed["packages"][1]["name"], "linux");
        assert!(parsed["scan_timestamp"].as_str().unwrap().contains("2025"));
    }

    #[test]
    fn json_report_empty_packages() {
        let timestamp = "2025-01-15T10:30:00Z".parse::<DateTime<Utc>>().unwrap();

        let report = JsonReport {
            scan_timestamp: timestamp,
            total_packages: 0,
            packages: vec![],
        };

        let json = serde_json::to_string_pretty(&report).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["total_packages"], 0);
        assert!(parsed["packages"].as_array().unwrap().is_empty());
    }

    #[test]
    fn json_report_optional_fields() {
        let packages = vec![InstalledPackage {
            name: "orphan".to_string(),
            version: "1.0".to_string(),
            description: None,
            url: None,
            source: PackageSource::Pacman,
            licenses: vec![],
        }];
        let timestamp = "2025-01-15T10:30:00Z".parse::<DateTime<Utc>>().unwrap();

        let report = JsonReport {
            scan_timestamp: timestamp,
            total_packages: 1,
            packages,
        };

        let json = serde_json::to_string_pretty(&report).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert!(parsed["packages"][0]["description"].is_null());
        assert!(parsed["packages"][0]["url"].is_null());
        assert!(
            parsed["packages"][0]["licenses"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
}
