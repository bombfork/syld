// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use serde::Deserialize;

use super::oci;
use super::{Discoverer, InstalledPackage, PackageSource};

/// Discovers container images available in the local Podman store.
///
/// Runs `podman image ls --format json` to enumerate locally available images,
/// then inspects each image via `podman inspect` to extract OCI metadata labels
/// (description, source URL, licenses). Dangling images (those with `<none>`
/// as repository) are filtered out.
///
/// Podman supports both rootful and rootless modes; this discoverer queries
/// the current user's image store.
pub struct PodmanDiscoverer;

impl Discoverer for PodmanDiscoverer {
    fn name(&self) -> &str {
        "podman"
    }

    fn is_available(&self) -> bool {
        Path::new("/usr/bin/podman").is_file() || Path::new("/usr/local/bin/podman").is_file()
    }

    fn discover(&self) -> Result<Vec<InstalledPackage>> {
        let output = Command::new("podman")
            .args(["image", "ls", "--format", "json"])
            .output()
            .context("Failed to run podman image ls")?;

        if !output.status.success() {
            anyhow::bail!(
                "podman image ls failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let stdout = String::from_utf8(output.stdout)
            .context("podman image ls output is not valid UTF-8")?;

        let images = parse_image_list(&stdout)?;

        let pb = ProgressBar::new(images.len() as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("  {bar:30} {pos}/{len} packages")
                .unwrap(),
        );

        let packages: Vec<InstalledPackage> = images
            .iter()
            .map(|image| {
                let labels = fetch_image_labels(&image.id);
                let (name, tag) = image.name_and_tag();
                let pkg =
                    oci::build_package_from_labels(&name, &tag, &labels, PackageSource::Podman);
                pb.inc(1);
                pkg
            })
            .collect();

        pb.finish_and_clear();

        Ok(packages)
    }
}

/// A single entry from `podman image ls --format json`.
///
/// Podman outputs a JSON array where each element has lowercase field names,
/// unlike Docker's PascalCase JSON-lines format. The repository and tag are
/// combined in the `names` array (e.g. `["docker.io/library/nginx:latest"]`).
#[derive(Debug, Deserialize)]
struct PodmanImage {
    #[serde(rename = "Id")]
    id: String,
    #[serde(rename = "Names", default)]
    names: Vec<String>,
}

impl PodmanImage {
    /// Extract the repository name and tag from the first entry in `names`.
    ///
    /// Podman stores full references like `docker.io/library/nginx:latest`.
    /// This method returns the full reference (minus the tag) as the name, and
    /// the tag portion separately. If no names are present, returns the short
    /// image ID.
    fn name_and_tag(&self) -> (String, String) {
        let reference = match self.names.first() {
            Some(n) => n.as_str(),
            None => return (short_id(&self.id), "<none>".to_string()),
        };

        match reference.rsplit_once(':') {
            Some((name, tag)) => (name.to_string(), tag.to_string()),
            None => (reference.to_string(), "<none>".to_string()),
        }
    }
}

/// Truncate a full image ID to a short 12-character prefix.
fn short_id(id: &str) -> String {
    let hex = id.strip_prefix("sha256:").unwrap_or(id);
    hex.chars().take(12).collect()
}

/// Parse the JSON array output of `podman image ls --format json`.
///
/// Images with no names (dangling/intermediate images) are filtered out.
fn parse_image_list(output: &str) -> Result<Vec<PodmanImage>> {
    let trimmed = output.trim();

    if trimmed.is_empty() || trimmed == "[]" || trimmed == "null" {
        return Ok(Vec::new());
    }

    let images: Vec<PodmanImage> =
        serde_json::from_str(trimmed).context("Failed to parse podman image ls JSON")?;

    // Filter out dangling images (no names)
    Ok(images.into_iter().filter(|i| !i.names.is_empty()).collect())
}

/// Fetch OCI labels for a given image ID via `podman inspect`.
///
/// Returns the labels as a map, or an empty map if inspection fails.
fn fetch_image_labels(image_id: &str) -> HashMap<String, String> {
    let output = Command::new("podman")
        .args(["inspect", "--format", "{{json .Labels}}", image_id])
        .output();

    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => return HashMap::new(),
    };

    let stdout = match String::from_utf8(output.stdout) {
        Ok(s) => s,
        Err(_) => return HashMap::new(),
    };

    oci::parse_labels(&stdout).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_image_list_basic() {
        let output = r#"[
            {
                "Id": "sha256:abc123def456789",
                "Names": ["docker.io/library/nginx:latest"],
                "Digest": "sha256:aaa",
                "Size": 187000000
            },
            {
                "Id": "sha256:def456abc789012",
                "Names": ["docker.io/library/postgres:16.2"],
                "Digest": "sha256:bbb",
                "Size": 412000000
            }
        ]"#;

        let images = parse_image_list(output).unwrap();
        assert_eq!(images.len(), 2);

        let (name, tag) = images[0].name_and_tag();
        assert_eq!(name, "docker.io/library/nginx");
        assert_eq!(tag, "latest");

        let (name, tag) = images[1].name_and_tag();
        assert_eq!(name, "docker.io/library/postgres");
        assert_eq!(tag, "16.2");
    }

    #[test]
    fn parse_image_list_filters_dangling() {
        let output = r#"[
            {
                "Id": "sha256:abc123",
                "Names": ["docker.io/library/nginx:latest"],
                "Size": 187000000
            },
            {
                "Id": "sha256:dangling1",
                "Names": [],
                "Size": 100000000
            }
        ]"#;

        let images = parse_image_list(output).unwrap();
        assert_eq!(images.len(), 1);
        let (name, _) = images[0].name_and_tag();
        assert_eq!(name, "docker.io/library/nginx");
    }

    #[test]
    fn parse_image_list_empty_array() {
        let images = parse_image_list("[]").unwrap();
        assert!(images.is_empty());
    }

    #[test]
    fn parse_image_list_empty_string() {
        let images = parse_image_list("").unwrap();
        assert!(images.is_empty());
    }

    #[test]
    fn parse_image_list_null() {
        let images = parse_image_list("null").unwrap();
        assert!(images.is_empty());
    }

    #[test]
    fn name_and_tag_with_registry() {
        let image = PodmanImage {
            id: "sha256:abc123".to_string(),
            names: vec!["ghcr.io/owner/myapp:v1.2.3".to_string()],
        };

        let (name, tag) = image.name_and_tag();
        assert_eq!(name, "ghcr.io/owner/myapp");
        assert_eq!(tag, "v1.2.3");
    }

    #[test]
    fn name_and_tag_no_names_uses_short_id() {
        let image = PodmanImage {
            id: "sha256:abc123def456789abcdef".to_string(),
            names: vec![],
        };

        let (name, tag) = image.name_and_tag();
        assert_eq!(name, "abc123def456");
        assert_eq!(tag, "<none>");
    }

    #[test]
    fn name_and_tag_no_tag_separator() {
        let image = PodmanImage {
            id: "sha256:abc123".to_string(),
            names: vec!["localhost/myimage".to_string()],
        };

        let (name, tag) = image.name_and_tag();
        assert_eq!(name, "localhost/myimage");
        assert_eq!(tag, "<none>");
    }

    #[test]
    fn name_and_tag_multiple_names_uses_first() {
        let image = PodmanImage {
            id: "sha256:abc123".to_string(),
            names: vec![
                "docker.io/library/nginx:latest".to_string(),
                "docker.io/library/nginx:1.25".to_string(),
            ],
        };

        let (name, tag) = image.name_and_tag();
        assert_eq!(name, "docker.io/library/nginx");
        assert_eq!(tag, "latest");
    }

    #[test]
    fn short_id_strips_sha256_prefix() {
        assert_eq!(short_id("sha256:abc123def456789"), "abc123def456");
    }

    #[test]
    fn short_id_without_prefix() {
        assert_eq!(short_id("abc123def456789"), "abc123def456");
    }

    #[test]
    fn short_id_shorter_than_12() {
        assert_eq!(short_id("abc"), "abc");
    }

    #[test]
    fn parse_image_list_missing_names_field() {
        // When Names field is absent entirely, serde default gives empty vec
        let output = r#"[
            {
                "Id": "sha256:abc123",
                "Size": 100000000
            }
        ]"#;

        let images = parse_image_list(output).unwrap();
        assert!(images.is_empty()); // filtered as dangling
    }
}
