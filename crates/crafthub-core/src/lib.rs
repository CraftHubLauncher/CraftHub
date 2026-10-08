//! CraftHub core: an unofficial, independent manager for Storytold's Craft apps.
//!
//! This crate owns every side effect (network, filesystem, processes, SQLite). The Tauri
//! shell and the `crafthub-cli` binary are thin front-ends over [`engine::Engine`].

pub mod catalog;
pub mod engine;
pub mod error;
pub mod installer;
pub mod locks;
pub mod net;
pub mod paths;
pub mod platform;
pub mod registry;
pub mod releases;
pub mod util;
pub mod validate;

pub use engine::{AppView, Engine, EngineConfig, ProgressEvent, ProgressSink};
pub use error::{CoreError, Result};

/// Tauri bundle identifier (must equal `identifier` in `src-tauri/tauri.conf.json`); also
/// names the per-user data folder shared by GUI and CLI.
pub const APP_IDENTIFIER: &str = "io.github.crafthublauncher.crafthub";

/// Data-folder names used by earlier builds. When the identifier changes, add the old value
/// here: on first start the old folder is renamed to the new one, keeping installs, history
/// and settings. The old provisional identifier remains listed so migration is one-time and
/// retryable without touching the real installed-app library.
const LEGACY_DATA_DIR_NAMES: &[&str] = &["io.github.crafthub-community.crafthub"];

/// `%LOCALAPPDATA%\<identifier>` (database, logs). `CRAFTHUB_ROOT` overrides it to
/// `<CRAFTHUB_ROOT>\Data` for isolated testing.
pub fn default_data_dir() -> Result<Option<std::path::PathBuf>> {
    if let Some(r) = std::env::var_os("CRAFTHUB_ROOT") {
        return Ok(Some(std::path::PathBuf::from(r).join("Data")));
    }
    let Some(local) = std::env::var_os("LOCALAPPDATA") else {
        return Ok(None);
    };
    let local = std::path::PathBuf::from(local);
    Ok(Some(migrate_data_dir(
        &local,
        APP_IDENTIFIER,
        LEGACY_DATA_DIR_NAMES,
    )?))
}

/// Returns `<base>/<current>`, first renaming the newest existing legacy folder into place
/// if the current one does not exist yet. A failed rename leaves everything untouched.
fn migrate_data_dir(
    base: &std::path::Path,
    current: &str,
    legacy: &[&str],
) -> Result<std::path::PathBuf> {
    let target = base.join(current);
    if let Ok(meta) = std::fs::symlink_metadata(&target)
        && crate::platform::is_link_like(&meta)
    {
        return Err(CoreError::InvalidInput(
            "Current data folder is a link or junction; migration stopped safely.".into(),
        ));
    }
    for old in legacy.iter().rev() {
        let from = base.join(old);
        let Ok(from_meta) = std::fs::symlink_metadata(&from) else {
            continue;
        };
        if crate::platform::is_link_like(&from_meta) {
            return Err(CoreError::InvalidInput(
                "Legacy data folder is a link or junction; migration stopped safely.".into(),
            ));
        }
        let legacy_db = from.join("crafthub.db");
        if !legacy_db.is_file() {
            return Err(CoreError::Db(
                "Legacy data folder has no readable registry database; migration stopped safely."
                    .into(),
            ));
        }
        if crate::platform::is_link_like(
            &std::fs::symlink_metadata(&legacy_db)
                .map_err(CoreError::io("checking legacy registry database"))?,
        ) {
            return Err(CoreError::InvalidInput(
                "Legacy registry database is a link or junction; migration stopped safely.".into(),
            ));
        }
        let destination_db = target.join("crafthub.db");
        if destination_db.exists()
            && crate::platform::is_link_like(
                &std::fs::symlink_metadata(&destination_db)
                    .map_err(CoreError::io("checking current registry database"))?,
            )
        {
            return Err(CoreError::InvalidInput(
                "Current registry database is a link or junction; migration stopped safely.".into(),
            ));
        }
        std::fs::create_dir_all(&target).map_err(CoreError::io("creating current data folder"))?;
        let report = crate::registry::Registry::merge_legacy_database(&destination_db, &legacy_db)?;
        tracing::info!(
            imported_apps = report.imported_apps,
            skipped_conflicts = report.skipped_conflicts,
            already_complete = report.already_complete,
            "legacy registry migration completed; legacy data retained"
        );
        break;
    }
    Ok(target)
}

/// Builds the production engine configuration rooted at `%LOCALAPPDATA%\Programs\CraftHub`
/// (apps) with the database stored in `data_dir`. `CRAFTHUB_ROOT` overrides the apps root.
pub fn production_config(data_dir: &std::path::Path) -> Result<EngineConfig> {
    let root = match std::env::var_os("CRAFTHUB_ROOT") {
        Some(r) => std::path::PathBuf::from(r),
        None => paths::ManagedPaths::default_root()
            .ok_or_else(|| CoreError::Unsupported("LOCALAPPDATA is not set".into()))?,
    };
    std::fs::create_dir_all(data_dir).map_err(CoreError::io("creating data folder"))?;
    Ok(EngineConfig {
        paths: paths::ManagedPaths::new(root),
        db_path: Some(data_dir.join("crafthub.db")),
        github: releases::GithubConfig::production(),
        extract_limits: installer::extract::ExtractLimits::default(),
        catalog: catalog::Catalog::load_embedded()?,
    })
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use crate::registry::{InstallRecord, Registry, Settings};

    fn db(dir: &Path) -> PathBuf {
        dir.join("crafthub.db")
    }

    fn seed_registry(path: &Path, app_id: &str, version_prefix: &str) -> Registry {
        let registry = Registry::open(path).unwrap();
        let library = Some(format!("D:\\CraftApps\\{app_id}"));
        let shortcut = Some(format!("C:\\Users\\Example\\Desktop\\{app_id}.lnk"));
        for (n, version) in [
            (1, format!("{version_prefix}.1.0")),
            (2, format!("{version_prefix}.2.0")),
        ] {
            let op = format!("op-{app_id}-{n}");
            let dir = format!("{version}_abcd");
            registry
                .begin_operation(
                    &op,
                    app_id,
                    "install",
                    Some(&version),
                    Some(&dir),
                    library.as_deref(),
                    n,
                )
                .unwrap();
            registry
                .prepare_activation(
                    &op,
                    app_id,
                    &dir,
                    library.as_deref(),
                    &[("app.exe".into(), 10)],
                )
                .unwrap();
            registry
                .commit_activation(
                    &op,
                    &InstallRecord {
                        app_id: app_id.into(),
                        version,
                        tag: format!("v{n}"),
                        dir_name: dir,
                        executable: "app.exe".into(),
                        asset_name: "app.zip".into(),
                        sha256: format!("{n:064x}"),
                        verification: "test".into(),
                        installed_at: n,
                        previous: None,
                        library: library.clone(),
                        shortcut_path: shortcut.clone(),
                    },
                )
                .unwrap();
        }
        registry.save_settings(&Settings::default()).unwrap();
        registry
            .record_event(
                app_id,
                "install",
                Some("2.2.0"),
                "success",
                Some("fixture"),
                2,
            )
            .unwrap();
        registry.kv_set("fixture", "preserve").unwrap();
        registry
    }

    #[test]
    fn identifier_matches_tauri_config() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../../../src-tauri/tauri.conf.json")).unwrap();
        assert_eq!(conf["identifier"], super::APP_IDENTIFIER);
    }

    #[test]
    fn migrates_legacy_only_and_retains_source() {
        let t = tempfile::tempdir().unwrap();
        let legacy = t.path().join("old.id");
        std::fs::create_dir_all(&legacy).unwrap();
        let _source = seed_registry(&db(&legacy), "photocraft", "1");
        assert!(legacy.join("crafthub.db-wal").exists());
        let current = super::migrate_data_dir(t.path(), "new.id", &["old.id"]).unwrap();
        let imported = Registry::open(&db(&current)).unwrap();
        let record = imported.get_install("photocraft").unwrap().unwrap();
        assert_eq!(record.version, "1.2.0");
        assert_eq!(record.previous.unwrap().version, "1.1.0");
        assert_eq!(
            imported
                .files_for("photocraft", "1.2.0_abcd")
                .unwrap()
                .len(),
            1
        );
        assert!(legacy.join("crafthub.db").exists());
    }

    #[test]
    fn current_only_is_unchanged() {
        let t = tempfile::tempdir().unwrap();
        let current = t.path().join(super::APP_IDENTIFIER);
        std::fs::create_dir_all(&current).unwrap();
        let _destination = seed_registry(&db(&current), "photocraft", "9");
        let result = super::migrate_data_dir(t.path(), super::APP_IDENTIFIER, &["old.id"]).unwrap();
        assert_eq!(result, current);
        assert_eq!(
            Registry::open(&db(&current))
                .unwrap()
                .get_install("photocraft")
                .unwrap()
                .unwrap()
                .version,
            "9.2.0"
        );
    }

    #[test]
    fn both_directories_import_into_empty_destination_and_are_idempotent() {
        let t = tempfile::tempdir().unwrap();
        let legacy = t.path().join("old.id");
        let current = t.path().join("new.id");
        std::fs::create_dir_all(&legacy).unwrap();
        let _source = seed_registry(&db(&legacy), "photocraft", "1");
        std::fs::create_dir_all(&current).unwrap();
        let _empty = Registry::open(&db(&current)).unwrap();

        super::migrate_data_dir(t.path(), "new.id", &["old.id"]).unwrap();
        super::migrate_data_dir(t.path(), "new.id", &["old.id"]).unwrap();
        let destination = Registry::open(&db(&current)).unwrap();
        assert_eq!(destination.list_installs().unwrap().len(), 1);
        assert_eq!(destination.recent_events(10).unwrap().len(), 1);
        assert_eq!(
            destination.kv_get("fixture").unwrap().as_deref(),
            Some("preserve")
        );
    }

    #[test]
    fn populated_destination_wins_conflicts() {
        let t = tempfile::tempdir().unwrap();
        let legacy = t.path().join("old.id");
        let current = t.path().join("new.id");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::create_dir_all(&current).unwrap();
        let _source = seed_registry(&db(&legacy), "photocraft", "1");
        drop(_source);
        let _source_other = seed_registry(&db(&legacy), "designcraft", "3");
        let _destination = seed_registry(&db(&current), "photocraft", "9");
        let report = Registry::merge_legacy_database(&db(&current), &db(&legacy)).unwrap();
        assert_eq!(report.imported_apps, 1);
        assert_eq!(report.skipped_conflicts, 1);
        let destination = Registry::open(&db(&current)).unwrap();
        assert_eq!(
            destination
                .get_install("photocraft")
                .unwrap()
                .unwrap()
                .version,
            "9.2.0"
        );
        let imported = destination.get_install("designcraft").unwrap().unwrap();
        assert_eq!(imported.version, "3.2.0");
        assert_eq!(
            imported.library.as_deref(),
            Some("D:\\CraftApps\\designcraft")
        );
        assert!(imported.shortcut_path.is_some());
        assert_eq!(imported.previous.unwrap().version, "3.1.0");
    }

    #[test]
    fn failed_migration_is_safe_and_retryable() {
        let t = tempfile::tempdir().unwrap();
        let legacy = t.path().join("old.id");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("crafthub.db"), b"not sqlite").unwrap();
        assert!(super::migrate_data_dir(t.path(), "new.id", &["old.id"]).is_err());
        assert!(legacy.exists());

        std::fs::remove_file(legacy.join("crafthub.db")).unwrap();
        let _source = seed_registry(&db(&legacy), "photocraft", "1");
        let current = super::migrate_data_dir(t.path(), "new.id", &["old.id"]).unwrap();
        assert!(
            Registry::open(&db(&current))
                .unwrap()
                .get_install("photocraft")
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn unsupported_legacy_schema_fails_without_replacing_destination() {
        let t = tempfile::tempdir().unwrap();
        let legacy = t.path().join("old.id");
        let current = t.path().join("new.id");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::create_dir_all(&current).unwrap();
        let _destination = seed_registry(&db(&current), "photocraft", "9");
        let legacy_db = rusqlite::Connection::open(db(&legacy)).unwrap();
        legacy_db.pragma_update(None, "user_version", 99).unwrap();
        drop(legacy_db);

        assert!(super::migrate_data_dir(t.path(), "new.id", &["old.id"]).is_err());
        assert_eq!(
            Registry::open(&db(&current))
                .unwrap()
                .get_install("photocraft")
                .unwrap()
                .unwrap()
                .version,
            "9.2.0"
        );
        assert!(legacy.join("crafthub.db").exists());
    }
}
