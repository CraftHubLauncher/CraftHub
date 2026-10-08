//! Cross-process, per-app locks so the GUI and the CLI (or two engines) can never modify
//! the same installation at once. Uses OS file locks (`LockFileEx` on Windows), which the
//! OS releases automatically if the holding process dies — no stale-lock cleanup needed.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};

use crate::error::{CoreError, Result};
use crate::validate::is_valid_app_id;

#[derive(Debug, Clone)]
pub struct LockDir {
    dir: PathBuf,
}

/// Held for the duration of an operation; dropping it releases the lock.
#[derive(Debug)]
pub struct AppLock {
    _file: File,
}

impl LockDir {
    pub fn new(dir: impl Into<PathBuf>) -> Result<Self> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir).map_err(CoreError::io("creating lock folder"))?;
        Ok(Self { dir })
    }

    pub fn path(&self) -> &Path {
        &self.dir
    }

    fn open(&self, app_id: &str) -> Result<File> {
        if !is_valid_app_id(app_id) {
            return Err(CoreError::InvalidInput(format!("bad app id {app_id:?}")));
        }
        OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.dir.join(format!("{app_id}.lock")))
            .map_err(CoreError::io("opening lock file"))
    }

    /// Returns `None` if another holder (this or another process) has the lock.
    pub fn try_lock(&self, app_id: &str) -> Result<Option<AppLock>> {
        let file = self.open(app_id)?;
        match file.try_lock() {
            Ok(()) => Ok(Some(AppLock { _file: file })),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(e)) => Err(CoreError::io("locking app")(e)),
        }
    }

    /// True if someone currently holds the lock. Briefly takes and releases it otherwise.
    pub fn is_locked(&self, app_id: &str) -> bool {
        matches!(self.try_lock(app_id), Ok(None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_is_exclusive_and_released_on_drop() {
        let t = tempfile::tempdir().unwrap();
        let a = LockDir::new(t.path()).unwrap();
        let b = LockDir::new(t.path()).unwrap(); // a second "process" view of the same dir
        let held = a.try_lock("photocraft").unwrap().expect("first lock");
        assert!(b.try_lock("photocraft").unwrap().is_none());
        assert!(b.is_locked("photocraft"));
        assert!(
            b.try_lock("vectorcraft").unwrap().is_some(),
            "locks are per app"
        );
        drop(held);
        assert!(!b.is_locked("photocraft"));
        assert!(b.try_lock("photocraft").unwrap().is_some());
        assert!(a.try_lock("../x").is_err());
    }
}
