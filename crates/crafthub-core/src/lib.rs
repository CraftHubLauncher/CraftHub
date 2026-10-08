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
pub fn default_data_dir() -> Option<std::path::PathBuf> {
    if let Some(r) = std::env::var_os("CRAFTHUB_ROOT") {
        return Some(std::path::PathBuf::from(r).join("Data"));
    }
    let local = std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA")?);
    Some(migrate_data_dir(
        &local,
        APP_IDENTIFIER,
        LEGACY_DATA_DIR_NAMES,
    ))
}

/// Returns `<base>/<current>`, first renaming the newest existing legacy folder into place
/// if the current one does not exist yet. A failed rename leaves everything untouched.
fn migrate_data_dir(base: &std::path::Path, current: &str, legacy: &[&str]) -> std::path::PathBuf {
    let target = base.join(current);
    if !target.exists() {
        for old in legacy.iter().rev() {
            let from = base.join(old);
            if from.is_dir() {
                match std::fs::rename(&from, &target) {
                    Ok(()) => tracing::info!(from = %old, "migrated data folder"),
                    Err(e) => tracing::warn!(error = %e, "could not migrate data folder"),
                }
                break;
            }
        }
    }
    target
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
    #[test]
    fn identifier_matches_tauri_config() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../../../src-tauri/tauri.conf.json")).unwrap();
        assert_eq!(conf["identifier"], super::APP_IDENTIFIER);
    }

    #[test]
    fn data_dir_migration_renames_legacy_folder_once() {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(t.path().join("old.id")).unwrap();
        std::fs::write(t.path().join("old.id").join("crafthub.db"), b"db").unwrap();
        let dir = super::migrate_data_dir(t.path(), "new.id", &["old.id"]);
        assert_eq!(std::fs::read(dir.join("crafthub.db")).unwrap(), b"db");
        assert!(!t.path().join("old.id").exists());
        // An existing current folder is never overwritten.
        std::fs::create_dir_all(t.path().join("old.id")).unwrap();
        let again = super::migrate_data_dir(t.path(), "new.id", &["old.id"]);
        assert_eq!(again, dir);
        assert!(t.path().join("old.id").exists());
    }

    #[test]
    fn final_identifier_migrates_provisional_data_without_overwriting_current_data() {
        let t = tempfile::tempdir().unwrap();
        let legacy = t.path().join("io.github.crafthub-community.crafthub");
        let current = t.path().join(super::APP_IDENTIFIER);
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("crafthub.db"), b"legacy database").unwrap();

        let migrated = super::migrate_data_dir(
            t.path(),
            super::APP_IDENTIFIER,
            super::LEGACY_DATA_DIR_NAMES,
        );
        assert_eq!(migrated, current);
        assert_eq!(
            std::fs::read(current.join("crafthub.db")).unwrap(),
            b"legacy database"
        );
        assert!(!legacy.exists());

        // A retry never overwrites a current folder or deletes a remaining legacy folder.
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("keep.txt"), b"keep").unwrap();
        assert_eq!(
            super::migrate_data_dir(
                t.path(),
                super::APP_IDENTIFIER,
                super::LEGACY_DATA_DIR_NAMES
            ),
            current
        );
        assert_eq!(
            std::fs::read(current.join("crafthub.db")).unwrap(),
            b"legacy database"
        );
        assert!(legacy.join("keep.txt").exists());
    }
}
