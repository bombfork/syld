// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use serde::Deserialize;

use super::{Discoverer, InstalledPackage, PackageSource};

/// Discovers container images available in the local Docker daemon.
///
/// Runs `docker image ls --format '{{json .}}'` to enumerate locally available
/// images, then inspects each image via `docker inspect` to extract OCI metadata
/// labels (description, source URL, licenses). Dangling images (those with
/// `<none>` as repository) are filtered out.
pub struct DockerDiscoverer;

impl Discoverer for DockerDiscoverer {
    fn name(&self) -> &str {
        "docker"
    }

    fn is_available(&self) -> bool {
        Path::new("/usr/bin/docker").is_file()
            || Path::new("/usr/local/bin/docker").is_file()
            || std::env::var_os("HOME")
                .map(|h| Path::new(&h).join(".docker/bin/docker").is_file())
                .unwrap_or(false)
    }

    fn discover(&self) -> Result<Vec<InstalledPackage>> {
        let output = Command::new("docker")
            .args(["image", "ls", "--format", "{{json .}}"])
            .output()
            .context("Failed to run docker image ls")?;

        if !output.status.success() {
            anyhow::bail!(
                "docker image ls failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let stdout = String::from_utf8(output.stdout)
            .context("docker image ls output is not valid UTF-8")?;

        let images = parse_image_list(&stdout)?;

        let pb = ProgressBar::new(images.len() as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("  {bar:30} {pos}/{len} packages")
                .unwrap(),
        );

        let packages: Vec<InstalledPackage> = images
            .iter()
            .filter_map(|image| {
                let labels = fetch_image_labels(&image.id);
                let result = build_package(image, &labels);
                pb.inc(1);
                match result {
                    Ok(pkg) => Some(pkg),
                    Err(e) => {
                        pb.suspend(|| {
                            eprintln!(
                                "  Warning: failed to process docker image {}: {e}",
                                image.id
                            );
                        });
                        None
                    }
                }
            })
            .collect();

        pb.finish_and_clear();

        Ok(packages)
    }
}

/// A single entry from `docker image ls --format '{{json .}}'`.
#[derive(Debug, Deserialize)]
struct DockerImage {
    #[serde(rename = "Repository")]
    repository: String,
    #[serde(rename = "Tag")]
    tag: String,
    #[serde(rename = "ID")]
    id: String,
}

/// Parse the JSON-lines output of `docker image ls --format '{{json .}}'`.
///
/// Each line is a separate JSON object. Images with `<none>` as the repository
/// (dangling/intermediate images) are filtered out.
fn parse_image_list(output: &str) -> Result<Vec<DockerImage>> {
    let mut images = Vec::new();

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let image: DockerImage =
            serde_json::from_str(trimmed).context("Failed to parse docker image JSON line")?;

        // Filter out dangling/intermediate images
        if image.repository == "<none>" {
            continue;
        }

        images.push(image);
    }

    Ok(images)
}

/// Fetch OCI labels for a given image ID via `docker inspect`.
///
/// Returns the labels as a map, or an empty map if inspection fails.
fn fetch_image_labels(image_id: &str) -> HashMap<String, String> {
    let output = Command::new("docker")
        .args(["inspect", "--format", "{{json .Config.Labels}}", image_id])
        .output();

    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => return HashMap::new(),
    };

    let stdout = match String::from_utf8(output.stdout) {
        Ok(s) => s,
        Err(_) => return HashMap::new(),
    };

    parse_labels(&stdout).unwrap_or_default()
}

/// Parse the JSON output of `docker inspect --format '{{json .Config.Labels}}'`.
///
/// The output is a single JSON object mapping label keys to values, or the
/// literal string `null` when no labels are set.
fn parse_labels(output: &str) -> Result<HashMap<String, String>> {
    let trimmed = output.trim();

    if trimmed.is_empty() || trimmed == "null" {
        return Ok(HashMap::new());
    }

    let labels: HashMap<String, String> =
        serde_json::from_str(trimmed).context("Failed to parse docker inspect labels JSON")?;

    Ok(labels)
}

/// Build an [`InstalledPackage`] from a Docker image entry and its OCI labels.
fn build_package(
    image: &DockerImage,
    labels: &HashMap<String, String>,
) -> Result<InstalledPackage> {
    let name = image.repository.clone();

    let version = if image.tag == "<none>" {
        "unknown".to_string()
    } else {
        image.tag.clone()
    };

    // Extract URL: prefer source (e.g. GitHub repo), fall back to generic URL
    let url = labels
        .get("org.opencontainers.image.source")
        .or_else(|| labels.get("org.opencontainers.image.url"))
        .cloned();

    let description = labels.get("org.opencontainers.image.description").cloned();

    let licenses = labels
        .get("org.opencontainers.image.licenses")
        .map(|l| vec![l.clone()])
        .unwrap_or_default();

    Ok(InstalledPackage {
        name,
        version,
        description,
        url,
        source: PackageSource::Docker,
        licenses,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_image_list_basic() {
        let output = r#"{"Containers":"N/A","CreatedAt":"2024-01-15 10:30:00 +0000 UTC","CreatedSince":"2 months ago","Digest":"\u003cnone\u003e","ID":"abc123def456","Repository":"nginx","SharedSize":"N/A","Size":"187MB","Tag":"latest","UniqueSize":"N/A","VirtualSize":"187MB"}
{"Containers":"N/A","CreatedAt":"2024-01-10 08:00:00 +0000 UTC","CreatedSince":"2 months ago","Digest":"\u003cnone\u003e","ID":"def456abc789","Repository":"postgres","SharedSize":"N/A","Size":"412MB","Tag":"16.2","UniqueSize":"N/A","VirtualSize":"412MB"}"#;

        let images = parse_image_list(output).unwrap();
        assert_eq!(images.len(), 2);
        assert_eq!(images[0].repository, "nginx");
        assert_eq!(images[0].tag, "latest");
        assert_eq!(images[0].id, "abc123def456");
        assert_eq!(images[1].repository, "postgres");
        assert_eq!(images[1].tag, "16.2");
    }

    #[test]
    fn parse_image_list_filters_dangling() {
        let output = r#"{"Containers":"N/A","CreatedAt":"2024-01-15 10:30:00 +0000 UTC","CreatedSince":"2 months ago","Digest":"\u003cnone\u003e","ID":"abc123","Repository":"nginx","SharedSize":"N/A","Size":"187MB","Tag":"latest","UniqueSize":"N/A","VirtualSize":"187MB"}
{"Containers":"N/A","CreatedAt":"2024-01-10 08:00:00 +0000 UTC","CreatedSince":"2 months ago","Digest":"\u003cnone\u003e","ID":"dangling1","Repository":"\u003cnone\u003e","SharedSize":"N/A","Size":"100MB","Tag":"\u003cnone\u003e","UniqueSize":"N/A","VirtualSize":"100MB"}"#;

        let images = parse_image_list(output).unwrap();
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].repository, "nginx");
    }

    #[test]
    fn parse_image_list_empty() {
        let images = parse_image_list("").unwrap();
        assert!(images.is_empty());
    }

    #[test]
    fn parse_image_list_blank_lines() {
        let output = r#"
{"Containers":"N/A","CreatedAt":"2024-01-15 10:30:00 +0000 UTC","CreatedSince":"2 months ago","Digest":"\u003cnone\u003e","ID":"abc123","Repository":"nginx","SharedSize":"N/A","Size":"187MB","Tag":"latest","UniqueSize":"N/A","VirtualSize":"187MB"}

"#;
        let images = parse_image_list(output).unwrap();
        assert_eq!(images.len(), 1);
    }

    #[test]
    fn parse_labels_with_oci_metadata() {
        let output = r#"{"org.opencontainers.image.source":"https://github.com/nginx/nginx","org.opencontainers.image.url":"https://nginx.org","org.opencontainers.image.description":"Official nginx image","org.opencontainers.image.licenses":"BSD-2-Clause","maintainer":"NGINX Docker Maintainers"}"#;

        let labels = parse_labels(output).unwrap();
        assert_eq!(
            labels.get("org.opencontainers.image.source").unwrap(),
            "https://github.com/nginx/nginx"
        );
        assert_eq!(
            labels.get("org.opencontainers.image.url").unwrap(),
            "https://nginx.org"
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
        let image = DockerImage {
            repository: "nginx".to_string(),
            tag: "1.25.4".to_string(),
            id: "abc123".to_string(),
        };

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

        let pkg = build_package(&image, &labels).unwrap();
        assert_eq!(pkg.name, "nginx");
        assert_eq!(pkg.version, "1.25.4");
        assert_eq!(pkg.description.as_deref(), Some("Official nginx image"));
        assert_eq!(pkg.url.as_deref(), Some("https://github.com/nginx/nginx"));
        assert_eq!(pkg.source, PackageSource::Docker);
        assert_eq!(pkg.licenses, vec!["BSD-2-Clause"]);
    }

    #[test]
    fn build_package_no_labels() {
        let image = DockerImage {
            repository: "myapp".to_string(),
            tag: "dev".to_string(),
            id: "xyz789".to_string(),
        };

        let labels = HashMap::new();
        let pkg = build_package(&image, &labels).unwrap();
        assert_eq!(pkg.name, "myapp");
        assert_eq!(pkg.version, "dev");
        assert!(pkg.description.is_none());
        assert!(pkg.url.is_none());
        assert_eq!(pkg.source, PackageSource::Docker);
        assert!(pkg.licenses.is_empty());
    }

    #[test]
    fn build_package_none_tag_becomes_unknown() {
        let image = DockerImage {
            repository: "myapp".to_string(),
            tag: "<none>".to_string(),
            id: "xyz789".to_string(),
        };

        let labels = HashMap::new();
        let pkg = build_package(&image, &labels).unwrap();
        assert_eq!(pkg.version, "unknown");
    }

    #[test]
    fn build_package_url_prefers_source_over_url() {
        let image = DockerImage {
            repository: "nginx".to_string(),
            tag: "latest".to_string(),
            id: "abc123".to_string(),
        };

        let mut labels = HashMap::new();
        labels.insert(
            "org.opencontainers.image.source".to_string(),
            "https://github.com/nginx/nginx".to_string(),
        );
        labels.insert(
            "org.opencontainers.image.url".to_string(),
            "https://nginx.org".to_string(),
        );

        let pkg = build_package(&image, &labels).unwrap();
        assert_eq!(pkg.url.as_deref(), Some("https://github.com/nginx/nginx"));
    }

    #[test]
    fn build_package_url_falls_back_to_url_label() {
        let image = DockerImage {
            repository: "nginx".to_string(),
            tag: "latest".to_string(),
            id: "abc123".to_string(),
        };

        let mut labels = HashMap::new();
        labels.insert(
            "org.opencontainers.image.url".to_string(),
            "https://nginx.org".to_string(),
        );

        let pkg = build_package(&image, &labels).unwrap();
        assert_eq!(pkg.url.as_deref(), Some("https://nginx.org"));
    }

    #[test]
    fn parse_image_list_namespaced_repository() {
        let output = r#"{"Containers":"N/A","CreatedAt":"2024-01-15 10:30:00 +0000 UTC","CreatedSince":"2 months ago","Digest":"\u003cnone\u003e","ID":"abc123","Repository":"ghcr.io/owner/myapp","SharedSize":"N/A","Size":"50MB","Tag":"v1.2.3","UniqueSize":"N/A","VirtualSize":"50MB"}"#;

        let images = parse_image_list(output).unwrap();
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].repository, "ghcr.io/owner/myapp");
        assert_eq!(images[0].tag, "v1.2.3");
    }

    #[test]
    fn parse_image_list_multiple_tags_same_repo() {
        let output = r#"{"Containers":"N/A","CreatedAt":"2024-01-15 10:30:00 +0000 UTC","CreatedSince":"2 months ago","Digest":"\u003cnone\u003e","ID":"abc123","Repository":"python","SharedSize":"N/A","Size":"900MB","Tag":"3.12","UniqueSize":"N/A","VirtualSize":"900MB"}
{"Containers":"N/A","CreatedAt":"2024-01-10 08:00:00 +0000 UTC","CreatedSince":"2 months ago","Digest":"\u003cnone\u003e","ID":"def456","Repository":"python","SharedSize":"N/A","Size":"850MB","Tag":"3.11","UniqueSize":"N/A","VirtualSize":"850MB"}"#;

        let images = parse_image_list(output).unwrap();
        assert_eq!(images.len(), 2);
        assert_eq!(images[0].tag, "3.12");
        assert_eq!(images[1].tag, "3.11");
    }
}
