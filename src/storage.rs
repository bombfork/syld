// SPDX-License-Identifier: GPL-3.0-or-later

//! Local state persistence using SQLite.
//!
//! Stores scan results, budget settings, and enrichment cache
//! in ~/.local/share/syld/syld.db

use std::path::Path;

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use rusqlite::{Connection, params};

use crate::config::{BudgetConfig, Cadence, Config};
use crate::discover::{InstalledPackage, PackageSource};
use crate::project::UpstreamProject;

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
}
