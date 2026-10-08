//! SQLite-backed state: managed installs, per-version file manifests, the operation
//! journal used for crash recovery, the GitHub ETag cache, settings and event history.

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use crate::error::{CoreError, Result};
use crate::releases::Channel;

const SCHEMA_VERSION: i64 = 3;

/// v2: install "libraries" (user-selectable install roots). `library` NULL means the
/// default root, so v1 rows keep pointing at the folder they were installed into.
const MIGRATION_2: &str = r#"
ALTER TABLE installs ADD COLUMN library TEXT;
ALTER TABLE install_files ADD COLUMN library TEXT;
ALTER TABLE operations ADD COLUMN library TEXT;
CREATE TABLE kv (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#;

const MIGRATION_3: &str = r#"
ALTER TABLE installs ADD COLUMN shortcut_path TEXT;
"#;

const MIGRATION_1: &str = r#"
CREATE TABLE installs (
    app_id           TEXT PRIMARY KEY,
    version          TEXT NOT NULL,
    tag              TEXT NOT NULL,
    dir_name         TEXT NOT NULL,
    executable       TEXT NOT NULL,
    asset_name       TEXT NOT NULL,
    sha256           TEXT NOT NULL,
    verification     TEXT NOT NULL,
    installed_at     INTEGER NOT NULL,
    prev_version     TEXT,
    prev_tag         TEXT,
    prev_dir_name    TEXT,
    prev_sha256      TEXT,
    prev_verification TEXT
);
CREATE TABLE install_files (
    app_id    TEXT NOT NULL,
    dir_name  TEXT NOT NULL,
    rel_path  TEXT NOT NULL,
    size      INTEGER NOT NULL,
    PRIMARY KEY (app_id, dir_name, rel_path)
);
CREATE TABLE operations (
    id           TEXT PRIMARY KEY,
    app_id       TEXT NOT NULL,
    kind         TEXT NOT NULL,
    state        TEXT NOT NULL,
    target_version TEXT,
    target_dir_name TEXT,
    started_at   INTEGER NOT NULL,
    finished_at  INTEGER,
    error        TEXT
);
CREATE TABLE http_cache (
    url        TEXT PRIMARY KEY,
    etag       TEXT,
    body       TEXT NOT NULL,
    fetched_at INTEGER NOT NULL
);
CREATE TABLE events (
    id       INTEGER PRIMARY KEY AUTOINCREMENT,
    app_id   TEXT NOT NULL,
    kind     TEXT NOT NULL,
    version  TEXT,
    outcome  TEXT NOT NULL,
    message  TEXT,
    at       INTEGER NOT NULL
);
CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InstallRecord {
    pub app_id: String,
    pub version: String,
    pub tag: String,
    pub dir_name: String,
    pub executable: String,
    pub asset_name: String,
    pub sha256: String,
    pub verification: String,
    pub installed_at: i64,
    pub previous: Option<PreviousVersion>,
    /// Install root for this app (active and previous share it); `None` = default root.
    pub library: Option<String>,
    /// CraftHub-owned desktop shortcut, when the user opted into one.
    pub shortcut_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PreviousVersion {
    pub version: String,
    pub tag: String,
    pub dir_name: String,
    pub sha256: String,
    pub verification: String,
}

#[derive(Debug, Clone)]
pub struct CacheEntry {
    pub etag: Option<String>,
    pub body: String,
    pub fetched_at: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OperationRow {
    pub id: String,
    pub app_id: String,
    pub kind: String,
    pub state: String,
    pub target_version: Option<String>,
    pub target_dir_name: Option<String>,
    pub library: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, serde::Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum UpdateMode {
    /// Updates are shown in CraftHub only; nothing runs in the background except the
    /// optional startup check.
    Manual,
    /// Periodic background checks with (optional) Windows notifications.
    #[default]
    Notify,
    /// Periodic checks, and verified updates are installed for apps that are not running.
    Automatic,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EventRow {
    pub id: i64,
    pub app_id: String,
    pub kind: String,
    pub version: Option<String>,
    pub outcome: String,
    pub message: Option<String>,
    pub at: i64,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub channel: Channel,
    pub check_on_startup: bool,
    /// 0 disables periodic checks while CraftHub is open.
    pub check_interval_hours: u32,
    #[serde(default)]
    pub update_mode: UpdateMode,
    /// Native Windows notifications for newly available updates.
    #[serde(default = "yes")]
    pub notifications: bool,
    /// Closing the window keeps CraftHub running in the notification area.
    #[serde(default = "yes")]
    pub minimize_to_tray: bool,
    /// Root folder for *new* installs; `None` = default per-user folder. Only changed via
    /// the validated `Engine::set_install_root`, never directly from the UI.
    #[serde(default)]
    pub install_root: Option<String>,
    /// Default for new installations; individual dialogs can override it.
    #[serde(default = "yes")]
    pub create_shortcuts: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            channel: Channel::Stable,
            check_on_startup: true,
            check_interval_hours: 6,
            update_mode: UpdateMode::Notify,
            notifications: true,
            minimize_to_tray: true,
            install_root: None,
            create_shortcuts: true,
        }
    }
}

pub struct Registry {
    conn: Mutex<Connection>,
}

impl Registry {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "FULL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(CoreError::Db(format!(
                "database schema {version} is newer than this CraftHub supports ({SCHEMA_VERSION})"
            )));
        }
        if version < 1 {
            conn.execute_batch(&format!(
                "BEGIN; {MIGRATION_1} PRAGMA user_version = 1; COMMIT;"
            ))?;
        }
        if version < 2 {
            conn.execute_batch(&format!(
                "BEGIN; {MIGRATION_2} PRAGMA user_version = 2; COMMIT;"
            ))?;
        }
        if version < 3 {
            conn.execute_batch(&format!(
                "BEGIN; {MIGRATION_3} PRAGMA user_version = 3; COMMIT;"
            ))?;
        }
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn with<T>(&self, f: impl FnOnce(&mut Connection) -> rusqlite::Result<T>) -> Result<T> {
        let mut guard = self
            .conn
            .lock()
            .map_err(|_| CoreError::Db("registry lock poisoned".into()))?;
        Ok(f(&mut guard)?)
    }

    // ---------- installs ----------

    pub fn get_install(&self, app_id: &str) -> Result<Option<InstallRecord>> {
        self.with(|c| {
            c.query_row(
                "SELECT app_id, version, tag, dir_name, executable, asset_name, sha256, verification,
                        installed_at, prev_version, prev_tag, prev_dir_name, prev_sha256, prev_verification, library, shortcut_path
                 FROM installs WHERE app_id = ?1",
                [app_id],
                row_to_install,
            )
            .optional()
        })
    }

    pub fn list_installs(&self) -> Result<Vec<InstallRecord>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT app_id, version, tag, dir_name, executable, asset_name, sha256, verification,
                        installed_at, prev_version, prev_tag, prev_dir_name, prev_sha256, prev_verification, library, shortcut_path
                 FROM installs ORDER BY app_id",
            )?;
            let rows = stmt.query_map([], row_to_install)?;
            rows.collect()
        })
    }

    /// Records the file manifest for a version directory *before* it is moved into place,
    /// together with the journal state, so a crash mid-activation can be cleaned up.
    pub fn prepare_activation(
        &self,
        op_id: &str,
        app_id: &str,
        dir_name: &str,
        library: Option<&str>,
        files: &[(String, u64)],
    ) -> Result<()> {
        self.with(|c| {
            let tx = c.transaction()?;
            {
                let mut ins = tx.prepare(
                    "INSERT INTO install_files (app_id, dir_name, rel_path, size, library) VALUES (?1, ?2, ?3, ?4, ?5)",
                )?;
                for (p, s) in files {
                    ins.execute(params![app_id, dir_name, p, *s as i64, library])?;
                }
            }
            set_op_state(&tx, op_id, "activating")?;
            tx.commit()
        })
    }

    /// Atomically makes `rec` the active install, demoting the old active version to
    /// "previous". Returns the dir name of the version that falls out of retention, if any.
    pub fn commit_activation(&self, op_id: &str, rec: &InstallRecord) -> Result<Option<String>> {
        self.with(|c| {
            let tx = c.transaction()?;
            let old: Option<(String, String, String, String, String, Option<String>)> = tx
                .query_row(
                    "SELECT version, tag, dir_name, sha256, verification, prev_dir_name FROM installs WHERE app_id = ?1",
                    [&rec.app_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
                )
                .optional()?;
            let (prev, evicted) = match old {
                Some((v, t, d, s, ver, old_prev)) => (Some((v, t, d, s, ver)), old_prev),
                None => (None, None),
            };
            tx.execute(
                "INSERT OR REPLACE INTO installs (app_id, version, tag, dir_name, executable, asset_name, sha256,
                    verification, installed_at, prev_version, prev_tag, prev_dir_name, prev_sha256, prev_verification, library, shortcut_path)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
                params![
                    rec.app_id, rec.version, rec.tag, rec.dir_name, rec.executable, rec.asset_name,
                    rec.sha256, rec.verification, rec.installed_at,
                    prev.as_ref().map(|p| &p.0), prev.as_ref().map(|p| &p.1), prev.as_ref().map(|p| &p.2),
                    prev.as_ref().map(|p| &p.3), prev.as_ref().map(|p| &p.4), rec.library, rec.shortcut_path,
                ],
            )?;
            set_op_state(&tx, op_id, "committed")?;
            tx.commit()?;
            Ok(evicted)
        })
    }

    /// Swaps the active and previous versions (rollback).
    pub fn swap_active_and_previous(&self, app_id: &str, now: i64) -> Result<()> {
        self.with(|c| {
            let n = c.execute(
                "UPDATE installs SET
                    version = prev_version, prev_version = version,
                    tag = prev_tag, prev_tag = tag,
                    dir_name = prev_dir_name, prev_dir_name = dir_name,
                    sha256 = prev_sha256, prev_sha256 = sha256,
                    verification = prev_verification, prev_verification = verification,
                    installed_at = ?2
                 WHERE app_id = ?1 AND prev_dir_name IS NOT NULL",
                params![app_id, now],
            )?;
            if n == 1 {
                Ok(())
            } else {
                Err(rusqlite::Error::QueryReturnedNoRows)
            }
        })
    }

    pub fn clear_previous(&self, app_id: &str) -> Result<()> {
        self.with(|c| {
            c.execute(
                "UPDATE installs SET prev_version = NULL, prev_tag = NULL, prev_dir_name = NULL,
                    prev_sha256 = NULL, prev_verification = NULL WHERE app_id = ?1",
                [app_id],
            )
            .map(|_| ())
        })
    }

    pub fn delete_install(&self, app_id: &str) -> Result<()> {
        self.with(|c| {
            c.execute("DELETE FROM installs WHERE app_id = ?1", [app_id])
                .map(|_| ())
        })
    }

    pub fn set_shortcut_path(&self, app_id: &str, path: Option<&str>) -> Result<()> {
        self.with(|c| {
            c.execute(
                "UPDATE installs SET shortcut_path = ?2 WHERE app_id = ?1",
                params![app_id, path],
            )
            .map(|_| ())
        })
    }

    pub fn files_for(&self, app_id: &str, dir_name: &str) -> Result<Vec<(String, u64)>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT rel_path, size FROM install_files WHERE app_id = ?1 AND dir_name = ?2 ORDER BY rel_path",
            )?;
            let rows = stmt.query_map([app_id, dir_name], |r| Ok((r.get(0)?, r.get::<_, i64>(1)? as u64)))?;
            rows.collect()
        })
    }

    pub fn delete_files_for(&self, app_id: &str, dir_name: &str) -> Result<()> {
        self.with(|c| {
            c.execute(
                "DELETE FROM install_files WHERE app_id = ?1 AND dir_name = ?2",
                [app_id, dir_name],
            )
            .map(|_| ())
        })
    }

    /// The library a manifest belongs to: `None` if no manifest exists,
    /// `Some(None)` for the default root.
    pub fn manifest_library(&self, app_id: &str, dir_name: &str) -> Result<Option<Option<String>>> {
        self.with(|c| {
            c.query_row(
                "SELECT library FROM install_files WHERE app_id = ?1 AND dir_name = ?2 LIMIT 1",
                [app_id, dir_name],
                |r| r.get(0),
            )
            .optional()
        })
    }

    /// Every non-default library referenced anywhere in the registry.
    pub fn known_libraries(&self) -> Result<Vec<String>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT library FROM installs WHERE library IS NOT NULL
                 UNION SELECT library FROM install_files WHERE library IS NOT NULL
                 UNION SELECT library FROM operations WHERE library IS NOT NULL",
            )?;
            let rows = stmt.query_map([], |r| r.get(0))?;
            rows.collect()
        })
    }

    /// Directory names that have a manifest for `app_id`.
    pub fn manifest_dirs(&self, app_id: &str) -> Result<Vec<String>> {
        self.with(|c| {
            let mut stmt =
                c.prepare("SELECT DISTINCT dir_name FROM install_files WHERE app_id = ?1")?;
            let rows = stmt.query_map([app_id], |r| r.get(0))?;
            rows.collect()
        })
    }

    // ---------- operation journal ----------

    #[allow(clippy::too_many_arguments)]
    pub fn begin_operation(
        &self,
        id: &str,
        app_id: &str,
        kind: &str,
        target_version: Option<&str>,
        target_dir: Option<&str>,
        library: Option<&str>,
        now: i64,
    ) -> Result<()> {
        self.with(|c| {
            c.execute(
                "INSERT INTO operations (id, app_id, kind, state, target_version, target_dir_name, library, started_at)
                 VALUES (?1, ?2, ?3, 'started', ?4, ?5, ?6, ?7)",
                params![id, app_id, kind, target_version, target_dir, library, now],
            )
            .map(|_| ())
        })
    }

    /// The app an operation belongs to, if the operation is known.
    pub fn operation_app(&self, id: &str) -> Result<Option<String>> {
        self.with(|c| {
            c.query_row("SELECT app_id FROM operations WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .optional()
        })
    }

    pub fn set_operation_state(&self, id: &str, state: &str) -> Result<()> {
        self.with(|c| set_op_state(c, id, state))
    }

    pub fn finish_operation(
        &self,
        id: &str,
        state: &str,
        error: Option<&str>,
        now: i64,
    ) -> Result<()> {
        self.with(|c| {
            c.execute(
                "UPDATE operations SET state = ?2, error = ?3, finished_at = ?4 WHERE id = ?1",
                params![id, state, error, now],
            )
            .map(|_| ())
        })
    }

    pub fn unfinished_operations(&self) -> Result<Vec<OperationRow>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT id, app_id, kind, state, target_version, target_dir_name, library FROM operations
                 WHERE finished_at IS NULL ORDER BY started_at",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok(OperationRow {
                    id: r.get(0)?,
                    app_id: r.get(1)?,
                    kind: r.get(2)?,
                    state: r.get(3)?,
                    target_version: r.get(4)?,
                    target_dir_name: r.get(5)?,
                    library: r.get(6)?,
                })
            })?;
            rows.collect()
        })
    }

    // ---------- http cache ----------

    pub fn http_cache_get(&self, url: &str) -> Result<Option<CacheEntry>> {
        self.with(|c| {
            c.query_row(
                "SELECT etag, body, fetched_at FROM http_cache WHERE url = ?1",
                [url],
                |r| {
                    Ok(CacheEntry {
                        etag: r.get(0)?,
                        body: r.get(1)?,
                        fetched_at: r.get(2)?,
                    })
                },
            )
            .optional()
        })
    }

    pub fn http_cache_put(
        &self,
        url: &str,
        etag: Option<&str>,
        body: &str,
        now: i64,
    ) -> Result<()> {
        self.with(|c| {
            c.execute(
                "INSERT OR REPLACE INTO http_cache (url, etag, body, fetched_at) VALUES (?1, ?2, ?3, ?4)",
                params![url, etag, body, now],
            )
            .map(|_| ())
        })
    }

    pub fn http_cache_touch(&self, url: &str, now: i64) -> Result<()> {
        self.with(|c| {
            c.execute(
                "UPDATE http_cache SET fetched_at = ?2 WHERE url = ?1",
                params![url, now],
            )
            .map(|_| ())
        })
    }

    pub fn http_cache_clear(&self) -> Result<()> {
        self.with(|c| c.execute("DELETE FROM http_cache", []).map(|_| ()))
    }

    // ---------- events ----------

    pub fn record_event(
        &self,
        app_id: &str,
        kind: &str,
        version: Option<&str>,
        outcome: &str,
        message: Option<&str>,
        now: i64,
    ) -> Result<()> {
        self.with(|c| {
            c.execute(
                "INSERT INTO events (app_id, kind, version, outcome, message, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![app_id, kind, version, outcome, message, now],
            )?;
            // Bounded history.
            c.execute("DELETE FROM events WHERE id <= (SELECT MAX(id) FROM events) - 1000", [])?;
            Ok(())
        })
    }

    pub fn recent_events(&self, limit: u32) -> Result<Vec<EventRow>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT id, app_id, kind, version, outcome, message, at FROM events ORDER BY id DESC LIMIT ?1",
            )?;
            let rows = stmt.query_map([limit], |r| {
                Ok(EventRow {
                    id: r.get(0)?,
                    app_id: r.get(1)?,
                    kind: r.get(2)?,
                    version: r.get(3)?,
                    outcome: r.get(4)?,
                    message: r.get(5)?,
                    at: r.get(6)?,
                })
            })?;
            rows.collect()
        })
    }

    // ---------- key/value ----------

    pub fn kv_get(&self, key: &str) -> Result<Option<String>> {
        self.with(|c| {
            c.query_row("SELECT value FROM kv WHERE key = ?1", [key], |r| r.get(0))
                .optional()
        })
    }

    pub fn kv_set(&self, key: &str, value: &str) -> Result<()> {
        self.with(|c| {
            c.execute(
                "INSERT OR REPLACE INTO kv (key, value) VALUES (?1, ?2)",
                [key, value],
            )
            .map(|_| ())
        })
    }

    // ---------- settings ----------

    pub fn settings(&self) -> Result<Settings> {
        let raw: Option<String> = self.with(|c| {
            c.query_row(
                "SELECT value FROM settings WHERE key = 'settings'",
                [],
                |r| r.get(0),
            )
            .optional()
        })?;
        // Unreadable settings fall back to safe defaults rather than blocking startup.
        Ok(raw
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default())
    }

    pub fn save_settings(&self, s: &Settings) -> Result<()> {
        let json = serde_json::to_string(s).map_err(|e| CoreError::Db(e.to_string()))?;
        self.with(|c| {
            c.execute(
                "INSERT OR REPLACE INTO settings (key, value) VALUES ('settings', ?1)",
                [json],
            )
            .map(|_| ())
        })
    }
}

fn set_op_state(c: &Connection, id: &str, state: &str) -> rusqlite::Result<()> {
    c.execute(
        "UPDATE operations SET state = ?2 WHERE id = ?1",
        params![id, state],
    )
    .map(|_| ())
}

fn row_to_install(r: &rusqlite::Row<'_>) -> rusqlite::Result<InstallRecord> {
    let prev_dir: Option<String> = r.get(11)?;
    let previous = match prev_dir {
        Some(dir_name) => Some(PreviousVersion {
            version: r.get(9)?,
            tag: r.get(10)?,
            dir_name,
            sha256: r.get(12)?,
            verification: r.get(13)?,
        }),
        None => None,
    };
    Ok(InstallRecord {
        app_id: r.get(0)?,
        version: r.get(1)?,
        tag: r.get(2)?,
        dir_name: r.get(3)?,
        executable: r.get(4)?,
        asset_name: r.get(5)?,
        sha256: r.get(6)?,
        verification: r.get(7)?,
        installed_at: r.get(8)?,
        previous,
        library: r.get(14)?,
        shortcut_path: r.get(15)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(version: &str, dir: &str) -> InstallRecord {
        InstallRecord {
            app_id: "photocraft".into(),
            version: version.into(),
            tag: format!("v{version}"),
            dir_name: dir.into(),
            executable: "photocraft.exe".into(),
            asset_name: "a.zip".into(),
            sha256: "00".into(),
            verification: "github-digest".into(),
            installed_at: 1,
            previous: None,
            library: None,
            shortcut_path: None,
        }
    }

    #[test]
    fn migrates_v1_database_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db.sqlite");
        {
            let c = Connection::open(&path).unwrap();
            c.execute_batch(&format!(
                "BEGIN; {MIGRATION_1} PRAGMA user_version = 1; COMMIT;"
            ))
            .unwrap();
            c.execute(
                "INSERT INTO installs (app_id, version, tag, dir_name, executable, asset_name, sha256, verification, installed_at)
                 VALUES ('photocraft', '0.3.0', 'v0.3.0', '0.3.0_abc', 'photocraft.exe', 'a.zip', '00', 'github-digest', 1)",
                [],
            )
            .unwrap();
            c.execute(
                "INSERT INTO settings (key, value) VALUES ('settings', '{\"channel\":\"stable\",\"checkOnStartup\":true,\"checkIntervalHours\":6}')",
                [],
            )
            .unwrap();
        }
        let r = Registry::open(&path).unwrap();
        let got = r.get_install("photocraft").unwrap().unwrap();
        assert_eq!(got.version, "0.3.0");
        assert_eq!(got.library, None, "v1 rows stay in the default root");
        // Old settings JSON gains the new fields with defaults.
        let s = r.settings().unwrap();
        assert_eq!(s.update_mode, UpdateMode::Notify);
        assert!(s.minimize_to_tray && s.notifications && s.install_root.is_none());
        r.kv_set("k", "v").unwrap();
        assert_eq!(r.kv_get("k").unwrap().as_deref(), Some("v"));
    }

    #[test]
    fn activation_keeps_one_previous_version() {
        let r = Registry::open_in_memory().unwrap();
        for (i, (v, d)) in [("0.1.0", "d1"), ("0.2.0", "d2"), ("0.3.0", "d3")]
            .iter()
            .enumerate()
        {
            let op = format!("op{i}");
            r.begin_operation(&op, "photocraft", "install", Some(v), Some(d), None, 0)
                .unwrap();
            r.prepare_activation(&op, "photocraft", d, None, &[("x.exe".into(), 1)])
                .unwrap();
            let evicted = r.commit_activation(&op, &rec(v, d)).unwrap();
            match i {
                0 | 1 => assert_eq!(evicted, None),
                _ => assert_eq!(evicted.as_deref(), Some("d1")),
            }
        }
        let got = r.get_install("photocraft").unwrap().unwrap();
        assert_eq!(got.version, "0.3.0");
        assert_eq!(got.previous.as_ref().unwrap().version, "0.2.0");

        r.swap_active_and_previous("photocraft", 5).unwrap();
        let got = r.get_install("photocraft").unwrap().unwrap();
        assert_eq!(got.version, "0.2.0");
        assert_eq!(got.dir_name, "d2");
        assert_eq!(got.previous.unwrap().dir_name, "d3");
    }

    #[test]
    fn journal_and_settings_roundtrip() {
        let r = Registry::open_in_memory().unwrap();
        r.begin_operation("a", "photocraft", "install", None, None, None, 0)
            .unwrap();
        r.begin_operation("b", "photocraft", "install", None, None, None, 0)
            .unwrap();
        r.finish_operation("b", "completed", None, 1).unwrap();
        let open = r.unfinished_operations().unwrap();
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].id, "a");

        assert_eq!(r.settings().unwrap(), Settings::default());
        let s = Settings {
            channel: Channel::Beta,
            check_on_startup: false,
            check_interval_hours: 0,
            update_mode: UpdateMode::Automatic,
            notifications: false,
            minimize_to_tray: false,
            install_root: Some("D:\\Apps".into()),
            create_shortcuts: false,
        };
        r.save_settings(&s).unwrap();
        assert_eq!(r.settings().unwrap(), s);
    }

    #[test]
    fn rejects_future_schema() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db.sqlite");
        {
            let c = Connection::open(&path).unwrap();
            c.pragma_update(None, "user_version", 99).unwrap();
        }
        assert!(Registry::open(&path).is_err());
    }
}
