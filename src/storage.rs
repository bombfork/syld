// SPDX-License-Identifier: GPL-3.0-or-later

//! Local state persistence using SQLite.
//!
//! Stores scan results, budget settings, and enrichment cache
//! in ~/.local/share/syld/syld.db

use std::path::Path;

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use rusqlite::{Connection, params};

use crate::budget::DonationRecord;
use crate::config::{BudgetConfig, Cadence, Config};
use crate::discover::{InstalledPackage, PackageSource};
use crate::project::{FundingChannel, UpstreamProject};

/// A saved scan with its metadata and packages.
pub struct ScanRecord {
    pub id: i64,
    pub timestamp: DateTime<Utc>,
    pub packages: Vec<InstalledPackage>,
}

/// SQLite-backed local storage for syld state.
pub struct Storage {
    conn: Connection,
}

impl Storage {
    /// Open (or create) the database at the default location
    /// (`~/.local/share/syld/syld.db`) and run migrations.
    pub fn open() -> Result<Self> {
        let data_dir = Config::data_dir()?;
        std::fs::create_dir_all(&data_dir)
            .with_context(|| format!("Failed to create data directory {}", data_dir.display()))?;
        let db_path = data_dir.join("syld.db");
        Self::open_path(&db_path)
    }

    /// Open (or create) the database at a custom path and run migrations.
    ///
    /// Useful for tests (pass a tempfile path or use `:memory:`).
    pub fn open_path(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("Failed to open database at {}", path.display()))?;
        let storage = Self { conn };
        storage.migrate()?;
        Ok(storage)
    }

    /// Run schema migrations (create tables if they don't exist).
    fn migrate(&self) -> Result<()> {
        self.conn
            .execute_batch(
                "
            CREATE TABLE IF NOT EXISTS scans (
                id        INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT    NOT NULL
            );

            CREATE TABLE IF NOT EXISTS packages (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                scan_id     INTEGER NOT NULL REFERENCES scans(id) ON DELETE CASCADE,
                name        TEXT    NOT NULL,
                version     TEXT    NOT NULL,
                description TEXT,
                url         TEXT,
                source      TEXT    NOT NULL,
                licenses    TEXT    NOT NULL DEFAULT '[]'
            );

            CREATE INDEX IF NOT EXISTS idx_packages_scan_id ON packages(scan_id);

            CREATE TABLE IF NOT EXISTS enrichment_cache (
                project_url TEXT    PRIMARY KEY,
                data        TEXT    NOT NULL,
                cached_at   TEXT    NOT NULL
            );

            CREATE TABLE IF NOT EXISTS budget (
                id       INTEGER PRIMARY KEY CHECK (id = 1),
                amount   REAL,
                currency TEXT    NOT NULL DEFAULT 'USD',
                cadence  TEXT    NOT NULL DEFAULT 'monthly'
            );

            CREATE TABLE IF NOT EXISTS projects (
                url               TEXT PRIMARY KEY,
                name              TEXT NOT NULL,
                repo_url          TEXT,
                homepage          TEXT,
                licenses          TEXT NOT NULL DEFAULT '[]',
                funding           TEXT NOT NULL DEFAULT '[]',
                bug_tracker       TEXT,
                contributing_url  TEXT,
                is_open_source    INTEGER,
                documentation_url TEXT,
                good_first_issues_url TEXT,
                stars             INTEGER
            );

            CREATE TABLE IF NOT EXISTS donation_history (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                project_url TEXT    NOT NULL,
                amount      REAL   NOT NULL,
                currency    TEXT   NOT NULL DEFAULT 'USD',
                donated_at  TEXT   NOT NULL,
                via         TEXT,
                notes       TEXT
            );
            ",
            )
            .context("Failed to run database migrations")?;
        Ok(())
    }

    /// Save a scan with the current timestamp, returning the scan ID.
    pub fn save_scan(&self, packages: &[InstalledPackage]) -> Result<i64> {
        let now = Utc::now().to_rfc3339();

        let tx = self
            .conn
            .unchecked_transaction()
            .context("Failed to begin transaction")?;

        tx.execute("INSERT INTO scans (timestamp) VALUES (?1)", params![now])
            .context("Failed to insert scan")?;

        let scan_id = tx.last_insert_rowid();

        let mut stmt = tx.prepare_cached(
            "INSERT INTO packages (scan_id, name, version, description, url, source, licenses)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?;

        for pkg in packages {
            let licenses_json =
                serde_json::to_string(&pkg.licenses).context("Failed to serialize licenses")?;
            stmt.execute(params![
                scan_id,
                pkg.name,
                pkg.version,
                pkg.description,
                pkg.url,
                pkg.source.to_string(),
                licenses_json,
            ])?;
        }

        drop(stmt);
        tx.commit().context("Failed to commit scan")?;

        Ok(scan_id)
    }

    /// Retrieve the latest scan with its packages.
    ///
    /// Returns `None` if no scans exist. Otherwise returns a tuple of
    /// `(scan_id, timestamp, packages)`.
    pub fn latest_scan(&self) -> Result<Option<ScanRecord>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, timestamp FROM scans ORDER BY id DESC LIMIT 1")?;

        let row = stmt.query_row([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        });

        let (scan_id, ts_str) = match row {
            Ok(r) => r,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
            Err(e) => return Err(e).context("Failed to query latest scan"),
        };

        let timestamp: DateTime<Utc> = ts_str
            .parse()
            .with_context(|| format!("Failed to parse timestamp: {ts_str}"))?;

        let mut pkg_stmt = self.conn.prepare(
            "SELECT name, version, description, url, source, licenses
             FROM packages WHERE scan_id = ?1",
        )?;

        let packages = pkg_stmt
            .query_map(params![scan_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            })?
            .map(|r| {
                let (name, version, description, url, source_str, licenses_json) = r?;
                let source = parse_package_source(&source_str)?;
                let licenses: Vec<String> = serde_json::from_str(&licenses_json)
                    .context("Failed to deserialize licenses")?;
                Ok(InstalledPackage {
                    name,
                    version,
                    description,
                    url,
                    source,
                    licenses,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(Some(ScanRecord {
            id: scan_id,
            timestamp,
            packages,
        }))
    }

    /// Cache an enrichment result for a project URL.
    pub fn save_enrichment(&self, project_url: &str, project: &UpstreamProject) -> Result<()> {
        let data =
            serde_json::to_string(project).context("Failed to serialize upstream project")?;
        let now = Utc::now().to_rfc3339();

        self.conn.execute(
            "INSERT OR REPLACE INTO enrichment_cache (project_url, data, cached_at)
             VALUES (?1, ?2, ?3)",
            params![project_url, data, now],
        )?;

        Ok(())
    }

    /// Get a cached enrichment result, returning `None` if missing or expired
    /// (older than 7 days).
    pub fn get_enrichment(&self, project_url: &str) -> Result<Option<UpstreamProject>> {
        let mut stmt = self
            .conn
            .prepare("SELECT data, cached_at FROM enrichment_cache WHERE project_url = ?1")?;

        let row = stmt.query_row(params![project_url], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        });

        let (data, cached_at_str) = match row {
            Ok(r) => r,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
            Err(e) => return Err(e).context("Failed to query enrichment cache"),
        };

        let cached_at: DateTime<Utc> = cached_at_str
            .parse()
            .with_context(|| format!("Failed to parse cached_at: {cached_at_str}"))?;

        if Utc::now() - cached_at > Duration::days(7) {
            return Ok(None);
        }

        let project: UpstreamProject =
            serde_json::from_str(&data).context("Failed to deserialize cached project")?;

        Ok(Some(project))
    }

    /// Save budget settings (upserts a single row).
    pub fn save_budget(&self, budget: &BudgetConfig) -> Result<()> {
        let cadence_str = match budget.cadence {
            Cadence::Monthly => "monthly",
            Cadence::Yearly => "yearly",
        };

        self.conn.execute(
            "INSERT OR REPLACE INTO budget (id, amount, currency, cadence)
             VALUES (1, ?1, ?2, ?3)",
            params![budget.amount, budget.currency, cadence_str],
        )?;

        Ok(())
    }

    /// Get the saved budget settings, or `None` if not yet configured.
    pub fn get_budget(&self) -> Result<Option<BudgetConfig>> {
        let mut stmt = self
            .conn
            .prepare("SELECT amount, currency, cadence FROM budget WHERE id = 1")?;

        let row = stmt.query_row([], |row| {
            Ok((
                row.get::<_, Option<f64>>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        });

        let (amount, currency, cadence_str) = match row {
            Ok(r) => r,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
            Err(e) => return Err(e).context("Failed to query budget"),
        };

        let cadence = match cadence_str.as_str() {
            "yearly" => Cadence::Yearly,
            _ => Cadence::Monthly,
        };

        Ok(Some(BudgetConfig {
            amount,
            currency,
            cadence,
        }))
    }

    // --- Project CRUD ---

    /// Save (upsert) an upstream project, keyed by its repo or homepage URL.
    ///
    /// The `url` is derived from `repo_url` falling back to `homepage`.
    /// Returns an error if neither is set.
    pub fn save_project(&self, project: &UpstreamProject) -> Result<()> {
        let url = project
            .repo_url
            .as_deref()
            .or(project.homepage.as_deref())
            .context("Project has no repo_url or homepage to use as key")?;

        let licenses_json =
            serde_json::to_string(&project.licenses).context("Failed to serialize licenses")?;
        let funding_json =
            serde_json::to_string(&project.funding).context("Failed to serialize funding")?;

        self.conn.execute(
            "INSERT OR REPLACE INTO projects
             (url, name, repo_url, homepage, licenses, funding, bug_tracker,
              contributing_url, is_open_source, documentation_url,
              good_first_issues_url, stars)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                url,
                project.name,
                project.repo_url,
                project.homepage,
                licenses_json,
                funding_json,
                project.bug_tracker,
                project.contributing_url,
                project.is_open_source,
                project.documentation_url,
                project.good_first_issues_url,
                project.stars.map(|s| s as i64),
            ],
        )?;

        Ok(())
    }

    /// Get a project by its URL key.
    pub fn get_project(&self, url: &str) -> Result<Option<UpstreamProject>> {
        let mut stmt = self.conn.prepare(
            "SELECT name, repo_url, homepage, licenses, funding, bug_tracker,
                    contributing_url, is_open_source, documentation_url,
                    good_first_issues_url, stars
             FROM projects WHERE url = ?1",
        )?;

        let row = stmt.query_row(params![url], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, Option<bool>>(7)?,
                row.get::<_, Option<String>>(8)?,
                row.get::<_, Option<String>>(9)?,
                row.get::<_, Option<i64>>(10)?,
            ))
        });

        match row {
            Ok((
                name,
                repo_url,
                homepage,
                licenses_json,
                funding_json,
                bug_tracker,
                contributing_url,
                is_open_source,
                documentation_url,
                good_first_issues_url,
                stars,
            )) => {
                let licenses: Vec<String> = serde_json::from_str(&licenses_json)
                    .context("Failed to deserialize licenses")?;
                let funding: Vec<FundingChannel> =
                    serde_json::from_str(&funding_json).context("Failed to deserialize funding")?;
                Ok(Some(UpstreamProject {
                    name,
                    repo_url,
                    homepage,
                    licenses,
                    funding,
                    bug_tracker,
                    contributing_url,
                    is_open_source,
                    documentation_url,
                    good_first_issues_url,
                    stars: stars.map(|s| s as u64),
                }))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e).context("Failed to query project"),
        }
    }

    /// Get all saved projects.
    pub fn all_projects(&self) -> Result<Vec<UpstreamProject>> {
        let mut stmt = self.conn.prepare(
            "SELECT name, repo_url, homepage, licenses, funding, bug_tracker,
                    contributing_url, is_open_source, documentation_url,
                    good_first_issues_url, stars
             FROM projects ORDER BY name",
        )?;

        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<bool>>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, Option<String>>(9)?,
                    row.get::<_, Option<i64>>(10)?,
                ))
            })?
            .map(|r| {
                let (
                    name,
                    repo_url,
                    homepage,
                    licenses_json,
                    funding_json,
                    bug_tracker,
                    contributing_url,
                    is_open_source,
                    documentation_url,
                    good_first_issues_url,
                    stars,
                ) = r?;
                let licenses: Vec<String> = serde_json::from_str(&licenses_json)
                    .context("Failed to deserialize licenses")?;
                let funding: Vec<FundingChannel> =
                    serde_json::from_str(&funding_json).context("Failed to deserialize funding")?;
                Ok(UpstreamProject {
                    name,
                    repo_url,
                    homepage,
                    licenses,
                    funding,
                    bug_tracker,
                    contributing_url,
                    is_open_source,
                    documentation_url,
                    good_first_issues_url,
                    stars: stars.map(|s| s as u64),
                })
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(rows)
    }

    // --- Donation history ---

    /// Record a donation, returning the row ID.
    pub fn save_donation(
        &self,
        project_url: &str,
        amount: f64,
        currency: &str,
        donated_at: DateTime<Utc>,
        via: Option<&str>,
        notes: Option<&str>,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO donation_history (project_url, amount, currency, donated_at, via, notes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                project_url,
                amount,
                currency,
                donated_at.to_rfc3339(),
                via,
                notes,
            ],
        )?;

        Ok(self.conn.last_insert_rowid())
    }

    /// Get all donations since a given timestamp.
    pub fn donations_since(&self, since: DateTime<Utc>) -> Result<Vec<DonationRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, project_url, amount, currency, donated_at, via, notes
             FROM donation_history
             WHERE donated_at >= ?1
             ORDER BY donated_at",
        )?;

        let rows = stmt
            .query_map(params![since.to_rfc3339()], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, f64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                ))
            })?
            .map(|r| {
                let (id, project_url, amount, currency, donated_at_str, via, notes) = r?;
                let donated_at: DateTime<Utc> = donated_at_str
                    .parse()
                    .with_context(|| format!("Failed to parse donated_at: {donated_at_str}"))?;
                Ok(DonationRecord {
                    id,
                    project_url,
                    amount,
                    currency,
                    donated_at,
                    via,
                    notes,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(rows)
    }
}

/// Parse a package source string back into the enum.
fn parse_package_source(s: &str) -> Result<PackageSource> {
    match s {
        "pacman" => Ok(PackageSource::Pacman),
        "apt" => Ok(PackageSource::Apt),
        "dnf" => Ok(PackageSource::Dnf),
        "flatpak" => Ok(PackageSource::Flatpak),
        "snap" => Ok(PackageSource::Snap),
        "nix" => Ok(PackageSource::Nix),
        "mise" => Ok(PackageSource::Mise),
        "brew" => Ok(PackageSource::Brew),
        "docker" => Ok(PackageSource::Docker),
        "podman" => Ok(PackageSource::Podman),
        other => anyhow::bail!("Unknown package source: {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::FundingChannel;

    /// Helper: open an in-memory database for testing.
    fn open_memory() -> Storage {
        Storage::open_path(Path::new(":memory:")).expect("Failed to open in-memory database")
    }

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

    // --- Migration tests ---

    #[test]
    fn open_creates_tables() {
        let storage = open_memory();
        // Verify tables exist by querying them
        let count = |table: &str| -> i64 {
            storage
                .conn
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap_or_else(|_| panic!("{table} table should exist"))
        };
        assert_eq!(count("scans"), 0);
        assert_eq!(count("packages"), 0);
        assert_eq!(count("enrichment_cache"), 0);
        assert_eq!(count("budget"), 0);
        assert_eq!(count("projects"), 0);
        assert_eq!(count("donation_history"), 0);
    }

    #[test]
    fn open_twice_is_idempotent() {
        let storage = open_memory();
        // Running migrate again should not fail
        storage.migrate().expect("second migration should succeed");
    }

    // --- Scan tests ---

    #[test]
    fn save_and_retrieve_scan() {
        let storage = open_memory();
        let packages = sample_packages();

        let scan_id = storage.save_scan(&packages).expect("save_scan failed");
        assert_eq!(scan_id, 1);

        let scan = storage
            .latest_scan()
            .expect("latest_scan failed")
            .expect("should have a scan");

        assert_eq!(scan.id, 1);
        assert_eq!(scan.packages.len(), 2);
        assert_eq!(scan.packages[0].name, "firefox");
        assert_eq!(scan.packages[0].version, "128.0");
        assert_eq!(
            scan.packages[0].description,
            Some("Web browser".to_string())
        );
        assert_eq!(
            scan.packages[0].url,
            Some("https://www.mozilla.org/firefox/".to_string())
        );
        assert_eq!(scan.packages[0].source, PackageSource::Pacman);
        assert_eq!(scan.packages[0].licenses, vec!["MPL-2.0".to_string()]);

        assert_eq!(scan.packages[1].name, "linux");
        assert_eq!(scan.packages[1].description, None);
    }

    #[test]
    fn latest_scan_returns_newest() {
        let storage = open_memory();

        let pkgs1 = vec![InstalledPackage {
            name: "old-pkg".to_string(),
            version: "1.0".to_string(),
            description: None,
            url: None,
            source: PackageSource::Apt,
            licenses: vec![],
        }];
        storage.save_scan(&pkgs1).expect("first save");

        let pkgs2 = vec![InstalledPackage {
            name: "new-pkg".to_string(),
            version: "2.0".to_string(),
            description: None,
            url: None,
            source: PackageSource::Dnf,
            licenses: vec![],
        }];
        let id2 = storage.save_scan(&pkgs2).expect("second save");

        let scan = storage
            .latest_scan()
            .expect("latest_scan failed")
            .expect("should have a scan");

        assert_eq!(scan.id, id2);
        assert_eq!(scan.packages.len(), 1);
        assert_eq!(scan.packages[0].name, "new-pkg");
    }

    #[test]
    fn latest_scan_empty_db() {
        let storage = open_memory();
        let result = storage.latest_scan().expect("latest_scan failed");
        assert!(result.is_none());
    }

    #[test]
    fn save_empty_scan() {
        let storage = open_memory();
        let scan_id = storage.save_scan(&[]).expect("save empty scan");
        assert_eq!(scan_id, 1);

        let scan = storage
            .latest_scan()
            .expect("latest_scan failed")
            .expect("should have a scan");
        assert_eq!(scan.id, 1);
        assert!(scan.packages.is_empty());
    }

    // --- Enrichment cache tests ---

    #[test]
    fn save_and_get_enrichment() {
        let storage = open_memory();
        let project = UpstreamProject {
            name: "Firefox".to_string(),
            repo_url: Some("https://github.com/nicotine-plus/nicotine-plus".to_string()),
            homepage: Some("https://mozilla.org".to_string()),
            licenses: vec!["MPL-2.0".to_string()],
            funding: vec![FundingChannel {
                platform: "Open Collective".to_string(),
                url: "https://opencollective.com/firefox".to_string(),
            }],
            bug_tracker: Some("https://bugzilla.mozilla.org".to_string()),
            contributing_url: None,
            is_open_source: None,
            documentation_url: None,
            good_first_issues_url: None,
            stars: None,
        };

        storage
            .save_enrichment("https://mozilla.org", &project)
            .expect("save enrichment failed");

        let loaded = storage
            .get_enrichment("https://mozilla.org")
            .expect("get enrichment failed")
            .expect("should have cached project");

        assert_eq!(loaded.name, "Firefox");
        assert_eq!(loaded.funding.len(), 1);
        assert_eq!(loaded.funding[0].platform, "Open Collective");
        assert_eq!(
            loaded.bug_tracker,
            Some("https://bugzilla.mozilla.org".to_string())
        );
        assert!(loaded.contributing_url.is_none());
    }

    #[test]
    fn get_enrichment_missing() {
        let storage = open_memory();
        let result = storage
            .get_enrichment("https://nonexistent.org")
            .expect("get enrichment failed");
        assert!(result.is_none());
    }

    #[test]
    fn enrichment_overwrite() {
        let storage = open_memory();

        let project1 = UpstreamProject {
            name: "Old".to_string(),
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
        storage
            .save_enrichment("https://example.org", &project1)
            .unwrap();

        let project2 = UpstreamProject {
            name: "New".to_string(),
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
        storage
            .save_enrichment("https://example.org", &project2)
            .unwrap();

        let loaded = storage
            .get_enrichment("https://example.org")
            .unwrap()
            .unwrap();
        assert_eq!(loaded.name, "New");
    }

    // --- Budget tests ---

    #[test]
    fn save_and_get_budget() {
        let storage = open_memory();
        let budget = BudgetConfig {
            amount: Some(25.0),
            currency: "EUR".to_string(),
            cadence: Cadence::Yearly,
        };

        storage.save_budget(&budget).expect("save budget failed");

        let loaded = storage
            .get_budget()
            .expect("get budget failed")
            .expect("should have budget");

        assert_eq!(loaded.amount, Some(25.0));
        assert_eq!(loaded.currency, "EUR");
        assert!(matches!(loaded.cadence, Cadence::Yearly));
    }

    #[test]
    fn get_budget_empty() {
        let storage = open_memory();
        let result = storage.get_budget().expect("get budget failed");
        assert!(result.is_none());
    }

    #[test]
    fn budget_upsert() {
        let storage = open_memory();

        let budget1 = BudgetConfig {
            amount: Some(10.0),
            currency: "USD".to_string(),
            cadence: Cadence::Monthly,
        };
        storage.save_budget(&budget1).unwrap();

        let budget2 = BudgetConfig {
            amount: Some(50.0),
            currency: "GBP".to_string(),
            cadence: Cadence::Yearly,
        };
        storage.save_budget(&budget2).unwrap();

        let loaded = storage.get_budget().unwrap().unwrap();
        assert_eq!(loaded.amount, Some(50.0));
        assert_eq!(loaded.currency, "GBP");
        assert!(matches!(loaded.cadence, Cadence::Yearly));
    }

    #[test]
    fn budget_with_no_amount() {
        let storage = open_memory();
        let budget = BudgetConfig {
            amount: None,
            currency: "USD".to_string(),
            cadence: Cadence::Monthly,
        };
        storage.save_budget(&budget).unwrap();

        let loaded = storage.get_budget().unwrap().unwrap();
        assert!(loaded.amount.is_none());
    }

    // --- Package source round-trip ---

    #[test]
    fn all_package_sources_round_trip() {
        let sources = vec![
            PackageSource::Pacman,
            PackageSource::Apt,
            PackageSource::Dnf,
            PackageSource::Flatpak,
            PackageSource::Snap,
            PackageSource::Nix,
            PackageSource::Mise,
            PackageSource::Brew,
            PackageSource::Docker,
            PackageSource::Podman,
        ];

        for source in sources {
            let s = source.to_string();
            let parsed = parse_package_source(&s).expect(&format!("Failed to parse {s}"));
            assert_eq!(parsed, source);
        }
    }

    #[test]
    fn parse_unknown_source_errors() {
        let result = parse_package_source("unknown_manager");
        assert!(result.is_err());
    }

    // --- Tempfile test (exercises open_path with a real file) ---

    #[test]
    fn open_with_tempfile() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let db_path = dir.path().join("test.db");
        let storage = Storage::open_path(&db_path).expect("open tempfile db");

        storage.save_scan(&sample_packages()).unwrap();
        let scan = storage.latest_scan().unwrap().unwrap();
        assert_eq!(scan.packages.len(), 2);

        // Re-open the same file and verify data persists
        let storage2 = Storage::open_path(&db_path).expect("reopen tempfile db");
        let scan2 = storage2.latest_scan().unwrap().unwrap();
        assert_eq!(scan2.packages.len(), 2);
    }

    // --- Project CRUD tests ---

    fn sample_project() -> UpstreamProject {
        UpstreamProject {
            name: "Firefox".to_string(),
            repo_url: Some("https://github.com/nicotine-plus/nicotine-plus".to_string()),
            homepage: Some("https://mozilla.org".to_string()),
            licenses: vec!["MPL-2.0".to_string()],
            funding: vec![FundingChannel {
                platform: "Open Collective".to_string(),
                url: "https://opencollective.com/firefox".to_string(),
            }],
            bug_tracker: Some("https://bugzilla.mozilla.org".to_string()),
            contributing_url: Some(
                "https://firefox-source-docs.mozilla.org/contributing/".to_string(),
            ),
            is_open_source: Some(true),
            documentation_url: Some("https://firefox-source-docs.mozilla.org".to_string()),
            good_first_issues_url: Some("https://codetribute.mozilla.org".to_string()),
            stars: Some(1234),
        }
    }

    #[test]
    fn save_and_get_project() {
        let storage = open_memory();
        let project = sample_project();

        storage.save_project(&project).expect("save_project failed");

        // Key is repo_url
        let loaded = storage
            .get_project("https://github.com/nicotine-plus/nicotine-plus")
            .expect("get_project failed")
            .expect("should have project");

        assert_eq!(loaded.name, "Firefox");
        assert_eq!(loaded.repo_url, project.repo_url);
        assert_eq!(loaded.homepage, project.homepage);
        assert_eq!(loaded.licenses, vec!["MPL-2.0".to_string()]);
        assert_eq!(loaded.funding.len(), 1);
        assert_eq!(loaded.funding[0].platform, "Open Collective");
        assert_eq!(loaded.bug_tracker, project.bug_tracker);
        assert_eq!(loaded.contributing_url, project.contributing_url);
        assert_eq!(loaded.is_open_source, Some(true));
        assert_eq!(loaded.documentation_url, project.documentation_url);
        assert_eq!(loaded.good_first_issues_url, project.good_first_issues_url);
        assert_eq!(loaded.stars, Some(1234));
    }

    #[test]
    fn get_project_missing() {
        let storage = open_memory();
        let result = storage
            .get_project("https://nonexistent.org")
            .expect("get_project failed");
        assert!(result.is_none());
    }

    #[test]
    fn save_project_uses_homepage_as_fallback_key() {
        let storage = open_memory();
        let project = UpstreamProject {
            name: "HomepageOnly".to_string(),
            repo_url: None,
            homepage: Some("https://example.org".to_string()),
            licenses: vec![],
            funding: vec![],
            bug_tracker: None,
            contributing_url: None,
            is_open_source: None,
            documentation_url: None,
            good_first_issues_url: None,
            stars: None,
        };

        storage.save_project(&project).unwrap();
        let loaded = storage.get_project("https://example.org").unwrap().unwrap();
        assert_eq!(loaded.name, "HomepageOnly");
    }

    #[test]
    fn save_project_no_url_errors() {
        let storage = open_memory();
        let project = UpstreamProject {
            name: "NoUrl".to_string(),
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

        assert!(storage.save_project(&project).is_err());
    }

    #[test]
    fn all_projects_returns_sorted() {
        let storage = open_memory();

        let mut p1 = sample_project();
        p1.name = "Zebra".to_string();
        p1.repo_url = Some("https://github.com/zebra".to_string());
        storage.save_project(&p1).unwrap();

        let mut p2 = sample_project();
        p2.name = "Alpha".to_string();
        p2.repo_url = Some("https://github.com/alpha".to_string());
        storage.save_project(&p2).unwrap();

        let all = storage.all_projects().unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].name, "Alpha");
        assert_eq!(all[1].name, "Zebra");
    }

    #[test]
    fn all_projects_empty() {
        let storage = open_memory();
        let all = storage.all_projects().unwrap();
        assert!(all.is_empty());
    }

    #[test]
    fn project_upsert() {
        let storage = open_memory();
        let mut project = sample_project();
        storage.save_project(&project).unwrap();

        project.stars = Some(9999);
        storage.save_project(&project).unwrap();

        let loaded = storage
            .get_project(project.repo_url.as_deref().unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(loaded.stars, Some(9999));

        // Should still be one project, not two
        let all = storage.all_projects().unwrap();
        assert_eq!(all.len(), 1);
    }

    // --- Donation history tests ---

    #[test]
    fn save_and_query_donations() {
        let storage = open_memory();
        let now = Utc::now();

        let id = storage
            .save_donation(
                "https://github.com/example",
                10.0,
                "USD",
                now,
                Some("GitHub Sponsors"),
                Some("Monthly donation"),
            )
            .expect("save_donation failed");

        assert_eq!(id, 1);

        let donations = storage
            .donations_since(now - Duration::hours(1))
            .expect("donations_since failed");

        assert_eq!(donations.len(), 1);
        assert_eq!(donations[0].id, 1);
        assert_eq!(donations[0].project_url, "https://github.com/example");
        assert_eq!(donations[0].amount, 10.0);
        assert_eq!(donations[0].currency, "USD");
        assert_eq!(donations[0].via, Some("GitHub Sponsors".to_string()));
        assert_eq!(donations[0].notes, Some("Monthly donation".to_string()));
    }

    #[test]
    fn donations_since_filters_by_date() {
        let storage = open_memory();
        let old = Utc::now() - Duration::days(30);
        let recent = Utc::now() - Duration::hours(1);

        storage
            .save_donation("https://example.org/old", 5.0, "USD", old, None, None)
            .unwrap();
        storage
            .save_donation("https://example.org/new", 15.0, "EUR", recent, None, None)
            .unwrap();

        // Query since 7 days ago — should only get the recent one
        let donations = storage
            .donations_since(Utc::now() - Duration::days(7))
            .unwrap();

        assert_eq!(donations.len(), 1);
        assert_eq!(donations[0].project_url, "https://example.org/new");
        assert_eq!(donations[0].amount, 15.0);
        assert_eq!(donations[0].currency, "EUR");
    }

    #[test]
    fn donations_since_empty() {
        let storage = open_memory();
        let donations = storage
            .donations_since(Utc::now() - Duration::days(30))
            .unwrap();
        assert!(donations.is_empty());
    }

    // --- Backward-compatible deserialization test ---

    #[test]
    fn enrichment_cache_backward_compatible() {
        let storage = open_memory();

        // Simulate an old cache entry missing the new fields
        let old_json = r#"{
            "name": "OldProject",
            "repo_url": null,
            "homepage": null,
            "licenses": [],
            "funding": [],
            "bug_tracker": null,
            "contributing_url": null
        }"#;
        let now = Utc::now().to_rfc3339();
        storage
            .conn
            .execute(
                "INSERT INTO enrichment_cache (project_url, data, cached_at) VALUES (?1, ?2, ?3)",
                params!["https://old.example.org", old_json, now],
            )
            .unwrap();

        let loaded = storage
            .get_enrichment("https://old.example.org")
            .unwrap()
            .expect("should deserialize old entry");

        assert_eq!(loaded.name, "OldProject");
        assert!(loaded.is_open_source.is_none());
        assert!(loaded.documentation_url.is_none());
        assert!(loaded.good_first_issues_url.is_none());
        assert!(loaded.stars.is_none());
    }
}
