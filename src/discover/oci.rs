// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared helpers for OCI-compatible container image discoverers (Docker, Podman).
//!
//! Both Docker and Podman use the same OCI label conventions and produce
//! structurally similar output. This module extracts the common parsing logic
//! so that each backend only needs to handle its own command invocation and
//! image-list format.

use std::collections::HashMap;

use anyhow::{Context, Result};

use super::{InstalledPackage, PackageSource};

/// Parse a JSON object of OCI labels into a `HashMap`.
///
/// Handles the common cases: a valid JSON object, the literal `null` (no
/// labels), and empty input.
pub fn parse_labels(output: &str) -> Result<HashMap<String, String>> {
    let trimmed = output.trim();

    if trimmed.is_empty() || trimmed == "null" {
        return Ok(HashMap::new());
    }

    let labels: HashMap<String, String> =
        serde_json::from_str(trimmed).context("Failed to parse container inspect labels JSON")?;

    Ok(labels)
}

/// Build an [`InstalledPackage`] from a container image name, tag, OCI labels,
/// and the [`PackageSource`] identifying which runtime discovered it.
pub fn build_package_from_labels(
    name: &str,
    tag: &str,
    labels: &HashMap<String, String>,
    source: PackageSource,
) -> InstalledPackage {
    let version = if tag == "<none>" {
        "unknown".to_string()
    } else {
        tag.to_string()
    };

    let url = labels
        .get("org.opencontainers.image.source")
        .or_else(|| labels.get("org.opencontainers.image.url"))
        .cloned();

    let description = labels.get("org.opencontainers.image.description").cloned();

    let licenses = labels
        .get("org.opencontainers.image.licenses")
        .map(|l| vec![l.clone()])
        .unwrap_or_default();

    InstalledPackage {
        name: name.to_string(),
        version,
        description,
        url,
        source,
        licenses,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_labels_with_oci_metadata() {
        let output = r#"{"org.opencontainers.image.source":"https://github.com/nginx/nginx","org.opencontainers.image.url":"https://nginx.org","org.opencontainers.image.description":"Official nginx image","org.opencontainers.image.licenses":"BSD-2-Clause","maintainer":"NGINX Docker Maintainers"}"#;

        let labels = parse_labels(output).unwrap();
        assert_eq!(
            labels.get("org.opencontainers.image.source").unwrap(),
            "https://github.com/nginx/nginx"
        );
        assert_eq!(
            labels.get("org.opencontainers.image.description").unwrap(),
            "Official nginx image"
        );
        assert_eq!(
            labels.get("org.opencontainers.image.licenses").unwrap(),
            "BSD-2-Clause"
        );
    }

    #[test]
    fn parse_labels_null() {
        let labels = parse_labels("null\n").unwrap();
        assert!(labels.is_empty());
    }

    #[test]
    fn parse_labels_empty() {
        let labels = parse_labels("").unwrap();
        assert!(labels.is_empty());
    }

    #[test]
    fn parse_labels_empty_object() {
        let labels = parse_labels("{}").unwrap();
        assert!(labels.is_empty());
    }

    #[test]
    fn build_package_full_metadata() {
        let mut labels = HashMap::new();
        labels.insert(
            "org.opencontainers.image.source".to_string(),
            "https://github.com/nginx/nginx".to_string(),
        );
        labels.insert(
            "org.opencontainers.image.description".to_string(),
            "Official nginx image".to_string(),
        );
        labels.insert(
            "org.opencontainers.image.licenses".to_string(),
            "BSD-2-Clause".to_string(),
        );

        let pkg = build_package_from_labels("nginx", "1.25.4", &labels, PackageSource::Docker);
        assert_eq!(pkg.name, "nginx");
        assert_eq!(pkg.version, "1.25.4");
        assert_eq!(pkg.description.as_deref(), Some("Official nginx image"));
        assert_eq!(pkg.url.as_deref(), Some("https://github.com/nginx/nginx"));
        assert_eq!(pkg.source, PackageSource::Docker);
        assert_eq!(pkg.licenses, vec!["BSD-2-Clause"]);
    }

    #[test]
    fn build_package_no_labels() {
        let labels = HashMap::new();
        let pkg = build_package_from_labels("myapp", "dev", &labels, PackageSource::Podman);
        assert_eq!(pkg.name, "myapp");
        assert_eq!(pkg.version, "dev");
        assert!(pkg.description.is_none());
        assert!(pkg.url.is_none());
        assert_eq!(pkg.source, PackageSource::Podman);
        assert!(pkg.licenses.is_empty());
    }

    #[test]
    fn build_package_none_tag_becomes_unknown() {
        let labels = HashMap::new();
        let pkg = build_package_from_labels("myapp", "<none>", &labels, PackageSource::Docker);
        assert_eq!(pkg.version, "unknown");
    }

    #[test]
    fn build_package_url_prefers_source_over_url() {
        let mut labels = HashMap::new();
        labels.insert(
            "org.opencontainers.image.source".to_string(),
            "https://github.com/nginx/nginx".to_string(),
        );
        labels.insert(
            "org.opencontainers.image.url".to_string(),
            "https://nginx.org".to_string(),
        );

        let pkg = build_package_from_labels("nginx", "latest", &labels, PackageSource::Docker);
        assert_eq!(pkg.url.as_deref(), Some("https://github.com/nginx/nginx"));
    }

    #[test]
    fn build_package_url_falls_back_to_url_label() {
        let mut labels = HashMap::new();
        labels.insert(
            "org.opencontainers.image.url".to_string(),
            "https://nginx.org".to_string(),
        );

        let pkg = build_package_from_labels("nginx", "latest", &labels, PackageSource::Podman);
        assert_eq!(pkg.url.as_deref(), Some("https://nginx.org"));
    }
}
