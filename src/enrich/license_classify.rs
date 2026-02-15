// SPDX-License-Identifier: GPL-3.0-or-later

//! OSI license classification backend.
//!
//! Determines whether a project's licenses are OSI-approved using a built-in
//! list of SPDX identifiers. No network access required.

use anyhow::Result;

use super::EnrichmentBackend;
use crate::project::UpstreamProject;

pub struct LicenseClassifyBackend;

impl EnrichmentBackend for LicenseClassifyBackend {
    fn name(&self) -> &str {
        "license_classify"
    }

    fn is_available(&self) -> bool {
        true
    }

    fn enrich(&self, project: &UpstreamProject) -> Result<UpstreamProject> {
        let mut enriched = project.clone();

        if !project.licenses.is_empty() {
            let all_osi = project
                .licenses
                .iter()
                .all(|l| is_osi_approved(&normalize_spdx(l)));
            enriched.is_open_source = Some(all_osi);
        }

        Ok(enriched)
    }
}

/// Normalize an SPDX identifier for lookup: lowercase, strip `-or-later`/`-only`
/// suffixes, and strip `+` suffix.
fn normalize_spdx(id: &str) -> String {
    let s = id.trim().to_lowercase();
    let s = s.strip_suffix("-or-later").unwrap_or(&s).to_string();
    let s = s.strip_suffix("-only").unwrap_or(&s).to_string();
    s.strip_suffix('+').unwrap_or(&s).to_string()
}

/// Check if a normalized SPDX identifier is on the OSI-approved list.
fn is_osi_approved(normalized: &str) -> bool {
    OSI_APPROVED.contains(&normalized)
}

/// OSI-approved SPDX license identifiers (normalized to lowercase, base form).
///
/// Source: <https://opensource.org/licenses/>
const OSI_APPROVED: &[&str] = &[
    "0bsd",
    "aal",
    "afl-3.0",
    "agpl-3.0",
    "apache-1.1",
    "apache-2.0",
    "apsl-2.0",
    "artistic-1.0",
    "artistic-2.0",
    "blueoak-1.0.0",
    "bsd-1-clause",
    "bsd-2-clause",
    "bsd-2-clause-patent",
    "bsd-3-clause",
    "bsd-3-clause-lbnl",
    "bsl-1.0",
    "cal-1.0",
    "cal-1.0-combined-work-exception",
    "catosl-1.1",
    "cern-ohl-p-2.0",
    "cern-ohl-s-2.0",
    "cern-ohl-w-2.0",
    "cnri-python",
    "cpal-1.0",
    "cua-opl-1.0",
    "ecl-1.0",
    "ecl-2.0",
    "ecos-2.0",
    "efl-1.0",
    "efl-2.0",
    "entessa",
    "epl-1.0",
    "epl-2.0",
    "eupl-1.1",
    "eupl-1.2",
    "fair",
    "frameworx-1.0",
    "gpl-2.0",
    "gpl-3.0",
    "hpnd",
    "intel",
    "ipa",
    "ipl-1.0",
    "isc",
    "jam",
    "lgpl-2.0",
    "lgpl-2.1",
    "lgpl-3.0",
    "liliq-p-1.1",
    "liliq-r-1.1",
    "liliq-rplus-1.1",
    "lpl-1.0",
    "lpl-1.02",
    "lppl-1.0",
    "lppl-1.1",
    "lppl-1.2",
    "lppl-1.3a",
    "lppl-1.3c",
    "mit",
    "mit-0",
    "mit-modern-variant",
    "motosoto",
    "mpl-1.0",
    "mpl-1.1",
    "mpl-2.0",
    "ms-pl",
    "ms-rl",
    "mulanpsl-2.0",
    "multics",
    "nasa-1.3",
    "ncsa",
    "ngpl",
    "nokia",
    "nposl-3.0",
    "ntp",
    "oclc-2.0",
    "ofl-1.0",
    "ofl-1.1",
    "ogtsl",
    "oldap-2.8",
    "oset-pl-2.1",
    "osl-1.0",
    "osl-1.1",
    "osl-2.0",
    "osl-2.1",
    "osl-3.0",
    "php-3.0",
    "php-3.01",
    "postgresql",
    "python-2.0",
    "qpl-1.0",
    "rpl-1.1",
    "rpl-1.5",
    "rpsl-1.0",
    "rscpl",
    "simpl-2.0",
    "sissl",
    "sleepycat",
    "spl-1.0",
    "ucl-1.0",
    "unicode-dfs-2016",
    "unlicense",
    "upl-1.0",
    "vsl-1.0",
    "w3c",
    "watcom-1.0",
    "xnet",
    "zlib",
    "zpl-2.0",
    "zpl-2.1",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_or_later() {
        assert_eq!(normalize_spdx("GPL-3.0-or-later"), "gpl-3.0");
    }

    #[test]
    fn normalize_strips_only() {
        assert_eq!(normalize_spdx("GPL-2.0-only"), "gpl-2.0");
    }

    #[test]
    fn normalize_strips_plus() {
        assert_eq!(normalize_spdx("LGPL-2.1+"), "lgpl-2.1");
    }

    #[test]
    fn normalize_lowercases() {
        assert_eq!(normalize_spdx("MIT"), "mit");
    }

    #[test]
    fn common_licenses_are_osi() {
        for id in &[
            "MIT",
            "Apache-2.0",
            "GPL-3.0-or-later",
            "BSD-2-Clause",
            "ISC",
            "MPL-2.0",
        ] {
            assert!(
                is_osi_approved(&normalize_spdx(id)),
                "{id} should be OSI-approved"
            );
        }
    }

    #[test]
    fn non_osi_license() {
        assert!(!is_osi_approved(&normalize_spdx("WTFPL")));
        assert!(!is_osi_approved(&normalize_spdx("CC-BY-4.0")));
    }

    #[test]
    fn classify_sets_open_source_true() {
        let backend = LicenseClassifyBackend;
        let project = UpstreamProject {
            name: "test".to_string(),
            repo_url: None,
            homepage: None,
            licenses: vec!["MIT".to_string()],
            funding: vec![],
            bug_tracker: None,
            contributing_url: None,
            is_open_source: None,
            documentation_url: None,
            good_first_issues_url: None,
            stars: None,
        };

        let enriched = backend.enrich(&project).unwrap();
        assert_eq!(enriched.is_open_source, Some(true));
    }

    #[test]
    fn classify_sets_open_source_false() {
        let backend = LicenseClassifyBackend;
        let project = UpstreamProject {
            name: "test".to_string(),
            repo_url: None,
            homepage: None,
            licenses: vec!["proprietary".to_string()],
            funding: vec![],
            bug_tracker: None,
            contributing_url: None,
            is_open_source: None,
            documentation_url: None,
            good_first_issues_url: None,
            stars: None,
        };

        let enriched = backend.enrich(&project).unwrap();
        assert_eq!(enriched.is_open_source, Some(false));
    }

    #[test]
    fn classify_skips_empty_licenses() {
        let backend = LicenseClassifyBackend;
        let project = UpstreamProject {
            name: "test".to_string(),
            repo_url: None,
            homepage: None,
            licenses: vec![],
            funding: vec![],
            bug_tracker: None,
            contributing_url: None,
            is_open_source: None,
            documentation_url: None,
            good_first_issues_url: None,
            stars: None,
        };

        let enriched = backend.enrich(&project).unwrap();
        assert!(enriched.is_open_source.is_none());
    }

    #[test]
    fn classify_mixed_licenses_is_false() {
        let backend = LicenseClassifyBackend;
        let project = UpstreamProject {
            name: "test".to_string(),
            repo_url: None,
            homepage: None,
            licenses: vec!["MIT".to_string(), "proprietary".to_string()],
            funding: vec![],
            bug_tracker: None,
            contributing_url: None,
            is_open_source: None,
            documentation_url: None,
            good_first_issues_url: None,
            stars: None,
        };

        let enriched = backend.enrich(&project).unwrap();
        assert_eq!(enriched.is_open_source, Some(false));
    }
}
