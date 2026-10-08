//! Placeholder for future Linux (AppImage/tarball) and macOS (app bundle) adapters.
//! Nothing here pretends to work: every operation that matters returns `Unsupported`.

use std::path::Path;

use super::RunningProcess;
use crate::error::{CoreError, Result};

fn unsupported<T>(what: &str) -> Result<T> {
    Err(CoreError::Unsupported(format!(
        "{what} is not implemented on this platform yet."
    )))
}

pub fn process_images() -> Result<Vec<RunningProcess>> {
    unsupported("Detecting running apps")
}

pub fn available_space(_path: &Path) -> Result<u64> {
    unsupported("Checking free disk space")
}

pub fn check_local_drive(_path: &Path) -> Result<()> {
    Ok(())
}

pub fn is_reparse_point(_meta: &std::fs::Metadata) -> bool {
    false
}

pub fn spawn_detached(_exe: &Path, _cwd: &Path) -> Result<u32> {
    unsupported("Launching apps")
}

pub fn open_folder(_dir: &Path) -> Result<()> {
    unsupported("Opening folders")
}
