//! CraftHub-owned directories and the only code paths allowed to delete anything.
//!
//! Deletion rules:
//! * application files are removed **only** if listed in the registry manifest for that
//!   exact version directory; unknown files (user data, plug-ins) are left in place and reported;
//! * directories are removed only when empty;
//! * every target is canonicalised and must sit at `<Apps>/<app>/<version-dir>`;
//! * links/junctions are never followed or removed through.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{CoreError, Result};
use crate::platform::{dunce_canonicalize, is_link_like};
use crate::validate::{is_safe_component, is_safe_relative_path, is_valid_app_id};

/// File placed in every CraftHub install root ("library").
pub const LIBRARY_MARKER: &str = ".crafthub-library";
const MARKER_TEXT: &str = "This folder is managed by CraftHub. Apps/ holds installed applications; Staging/ and Downloads/ are temporary.\r\n";

/// Validates a user-chosen install root and prepares it. Returns the canonical path.
///
/// Rejected: relative/UNC/device paths, drive roots, network/optical drives, `.`/`..`,
/// links or junctions anywhere in the path, folders inside `forbidden` (system folders,
/// CraftHub's own data, other libraries' internals), non-empty folders that are not
/// already CraftHub libraries, and folders the current user cannot write to (CraftHub
/// never elevates).
pub fn validate_library_root(candidate: &Path, forbidden: &[PathBuf]) -> Result<PathBuf> {
    use std::path::Component;
    let bad = |m: &str| Err(CoreError::InvalidInput(m.to_string()));
    if !candidate.is_absolute() {
        return bad("Choose a complete folder path.");
    }
    #[cfg(windows)]
    {
        use std::path::Prefix;
        match candidate.components().next() {
            Some(Component::Prefix(p)) if matches!(p.kind(), Prefix::Disk(_)) => {}
            _ => {
                return bad(
                    "Network, UNC and device paths are not supported. Choose a folder on a local drive.",
                );
            }
        }
    }
    let mut normal = 0;
    for c in candidate.components() {
        match c {
            Component::Normal(n) => {
                normal += 1;
                if !is_safe_component(&n.to_string_lossy()) {
                    return bad("The path contains a name Windows does not allow.");
                }
            }
            Component::CurDir | Component::ParentDir => {
                return bad("The path must not contain '.' or '..'.");
            }
            _ => {}
        }
    }
    if normal == 0 {
        return bad("Choose a folder, not the root of a drive.");
    }
    crate::platform::check_local_drive(candidate)?;

    std::fs::create_dir_all(candidate).map_err(|_| {
        CoreError::InvalidInput(
            "CraftHub cannot create this folder. Choose one you can write to without administrator rights."
                .into(),
        )
    })?;
    let meta = std::fs::symlink_metadata(candidate).map_err(CoreError::io("checking folder"))?;
    if is_link_like(&meta) || !meta.is_dir() {
        return bad("The folder is a link or junction. Choose a regular folder.");
    }
    let canon = dunce_canonicalize(candidate)
        .ok_or_else(|| CoreError::InvalidInput("The folder could not be resolved.".into()))?;
    let key = |p: &Path| {
        let s = p
            .to_string_lossy()
            .trim_end_matches(['\\', '/'])
            .to_string();
        if cfg!(windows) { s.to_lowercase() } else { s }
    };
    if key(&canon) != key(candidate) {
        return bad("The path goes through a link or junction. Choose a regular folder.");
    }
    let ck = key(&canon);
    for f in forbidden {
        let fk = key(&dunce_canonicalize(f).unwrap_or_else(|| f.clone()));
        if fk.is_empty() {
            continue;
        }
        if ck == fk || ck.starts_with(&format!("{fk}{}", std::path::MAIN_SEPARATOR)) {
            return bad(
                "CraftHub can't use this folder because it is inside a system folder or one CraftHub uses internally.",
            );
        }
    }
    let is_library = canon.join(LIBRARY_MARKER).is_file();
    let empty = std::fs::read_dir(&canon)
        .map_err(CoreError::io("reading folder"))?
        .next()
        .is_none();
    if !empty && !is_library {
        return bad("Choose an empty folder (or one CraftHub set up before).");
    }
    let probe = canon.join(format!(".crafthub-write-test-{}", std::process::id()));
    let writable = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .is_ok();
    let _ = std::fs::remove_file(&probe);
    if !writable {
        return bad(
            "CraftHub cannot write to this folder. Choose one in your user profile or on a data drive.",
        );
    }
    ManagedPaths::new(&canon).ensure()?;
    Ok(canon)
}

#[derive(Debug, Clone)]
pub struct ManagedPaths {
    pub root: PathBuf,
    pub apps: PathBuf,
    pub staging: PathBuf,
    pub downloads: PathBuf,
}

impl ManagedPaths {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            apps: root.join("Apps"),
            staging: root.join("Staging"),
            downloads: root.join("Downloads"),
            root,
        }
    }

    /// Default per-user location: `%LOCALAPPDATA%\Programs\CraftHub`.
    pub fn default_root() -> Option<PathBuf> {
        std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("Programs").join("CraftHub"))
    }

    pub fn ensure(&self) -> Result<()> {
        for d in [&self.root, &self.apps, &self.staging, &self.downloads] {
            std::fs::create_dir_all(d)
                .map_err(CoreError::io(format!("creating {}", d.display())))?;
            let meta =
                std::fs::symlink_metadata(d).map_err(CoreError::io("checking managed folder"))?;
            if is_link_like(&meta) || !meta.is_dir() {
                return Err(CoreError::PathSafety(format!(
                    "{} is a link or not a folder; refusing to use it",
                    d.display()
                )));
            }
        }
        let marker = self.root.join(LIBRARY_MARKER);
        if !marker.exists() {
            std::fs::write(&marker, MARKER_TEXT)
                .map_err(CoreError::io("writing library marker"))?;
        }
        Ok(())
    }

    /// True if this root carries CraftHub's library marker. Nothing is ever deleted in a
    /// root without it, even if the registry (which is treated as untrusted) says so.
    pub fn has_marker(&self) -> bool {
        std::fs::symlink_metadata(self.root.join(LIBRARY_MARKER)).is_ok_and(|m| m.is_file())
    }

    pub fn app_dir(&self, app_id: &str) -> Result<PathBuf> {
        if !is_valid_app_id(app_id) {
            return Err(CoreError::InvalidInput(format!("bad app id {app_id:?}")));
        }
        Ok(self.apps.join(app_id))
    }

    pub fn version_dir(&self, app_id: &str, dir_name: &str) -> Result<PathBuf> {
        if !is_safe_component(dir_name) {
            return Err(CoreError::PathSafety(format!(
                "bad version folder {dir_name:?}"
            )));
        }
        Ok(self.app_dir(app_id)?.join(dir_name))
    }

    pub fn staging_dir(&self, op_id: &str) -> Result<PathBuf> {
        check_op_id(op_id)?;
        Ok(self.staging.join(op_id))
    }

    pub fn download_file(&self, op_id: &str) -> Result<PathBuf> {
        check_op_id(op_id)?;
        Ok(self.downloads.join(format!("{op_id}.zip.part")))
    }

    /// Removes a CraftHub-owned temporary item (staging folder or partial download)
    /// after verifying it is a direct child of the given temp root.
    pub fn remove_temp(&self, path: &Path) -> Result<()> {
        let Ok(meta) = std::fs::symlink_metadata(path) else {
            return Ok(());
        };
        let parent = path.parent().and_then(dunce_canonicalize);
        let allowed = [&self.staging, &self.downloads]
            .iter()
            .filter_map(|d| dunce_canonicalize(d))
            .any(|d| Some(&d) == parent.as_ref());
        if !allowed {
            return Err(CoreError::PathSafety(format!(
                "{} is not a CraftHub temp item",
                path.display()
            )));
        }
        let res = if is_link_like(&meta) {
            // Never traverse a link; remove the link itself only.
            if meta.is_dir() {
                std::fs::remove_dir(path)
            } else {
                std::fs::remove_file(path)
            }
        } else if meta.is_dir() {
            // std's remove_dir_all does not follow symlinks/junctions (CVE-2022-21658 fix).
            std::fs::remove_dir_all(path)
        } else {
            std::fs::remove_file(path)
        };
        res.map_err(CoreError::io(format!("removing {}", path.display())))
    }

    /// Lists CraftHub temp items (for crash-recovery sweeps).
    pub fn temp_items(&self) -> Vec<PathBuf> {
        [&self.staging, &self.downloads]
            .iter()
            .filter_map(|d| std::fs::read_dir(d).ok())
            .flatten()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .collect()
    }
}

fn check_op_id(op_id: &str) -> Result<()> {
    let ok = (8..=64).contains(&op_id.len())
        && op_id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-');
    if ok {
        Ok(())
    } else {
        Err(CoreError::InvalidInput("bad operation id".into()))
    }
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RemovalReport {
    pub removed_files: usize,
    /// Entries left behind because CraftHub did not install them (e.g. user data).
    pub kept_entries: Vec<String>,
    pub dir_removed: bool,
}

/// Removes the manifest-listed files of `<apps>/<app_id>/<dir_name>` and then any
/// directories left empty. Unlisted content is preserved.
pub fn remove_version_dir(
    paths: &ManagedPaths,
    app_id: &str,
    dir_name: &str,
    manifest: &[(String, u64)],
) -> Result<RemovalReport> {
    let dir = paths.version_dir(app_id, dir_name)?;
    let Ok(meta) = std::fs::symlink_metadata(&dir) else {
        return Ok(RemovalReport {
            dir_removed: true,
            ..Default::default()
        });
    };
    if is_link_like(&meta) || !meta.is_dir() {
        return Err(CoreError::PathSafety(format!(
            "{} is a link or not a folder",
            dir.display()
        )));
    }
    if !paths.has_marker() {
        return Err(CoreError::PathSafety(format!(
            "{} is not a CraftHub library (marker file missing); nothing was deleted",
            paths.root.display()
        )));
    }
    let canon_apps = dunce_canonicalize(&paths.apps)
        .ok_or_else(|| CoreError::PathSafety("apps folder is missing".into()))?;
    let canon_dir = dunce_canonicalize(&dir)
        .ok_or_else(|| CoreError::PathSafety("version folder vanished".into()))?;
    let depth_ok = canon_dir.parent().and_then(Path::parent) == Some(canon_apps.as_path());
    if !depth_ok || !canon_dir.starts_with(&canon_apps) {
        return Err(CoreError::PathSafety(format!(
            "{} is outside the managed apps folder",
            dir.display()
        )));
    }

    let mut report = RemovalReport::default();
    for (rel, _) in manifest {
        if !is_safe_relative_path(rel) {
            return Err(CoreError::PathSafety(format!(
                "manifest entry {rel:?} is not a safe path"
            )));
        }
        let target = rel.split('/').fold(canon_dir.clone(), |p, c| p.join(c));
        // Refuse to act through linked parent directories.
        if parent_chain_has_link(&canon_dir, &target) {
            continue;
        }
        match std::fs::symlink_metadata(&target) {
            Ok(m) if m.is_file() && !is_link_like(&m) => {
                std::fs::remove_file(&target)
                    .map_err(CoreError::io(format!("removing {}", target.display())))?;
                report.removed_files += 1;
            }
            _ => {}
        }
    }

    remove_empty_dirs(&canon_dir);
    match std::fs::remove_dir(&canon_dir) {
        Ok(()) => report.dir_removed = true,
        Err(_) => {
            report.kept_entries = std::fs::read_dir(&canon_dir)
                .map(|rd| {
                    rd.filter_map(|e| e.ok())
                        .map(|e| e.file_name().to_string_lossy().into_owned())
                        .collect()
                })
                .unwrap_or_default();
            report.kept_entries.sort();
        }
    }
    Ok(report)
}

fn parent_chain_has_link(base: &Path, target: &Path) -> bool {
    let mut cur = target.parent();
    while let Some(p) = cur {
        if p == base {
            return false;
        }
        if std::fs::symlink_metadata(p)
            .map(|m| is_link_like(&m))
            .unwrap_or(true)
        {
            return true;
        }
        cur = p.parent();
    }
    true
}

/// Removes empty sub-directories bottom-up without following links.
fn remove_empty_dirs(dir: &Path) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let Ok(m) = std::fs::symlink_metadata(e.path()) else {
            continue;
        };
        if m.is_dir() && !is_link_like(&m) {
            remove_empty_dirs(&e.path());
            let _ = std::fs::remove_dir(e.path());
        }
    }
}

/// Removes `<apps>/<app_id>` if it is empty.
pub fn remove_app_dir_if_empty(paths: &ManagedPaths, app_id: &str) {
    if let Ok(d) = paths.app_dir(app_id) {
        let _ = std::fs::remove_dir(d);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, ManagedPaths) {
        let t = tempfile::tempdir().unwrap();
        let p = ManagedPaths::new(t.path().join("CraftHub"));
        p.ensure().unwrap();
        (t, p)
    }

    // Some Windows CI images expose the system temp directory through a
    // junction. Resolve it before creating the fixture so the test path is a
    // regular local-drive path rather than an unresolved link-like path.
    fn canonical_test_tempdir() -> tempfile::TempDir {
        let base = crate::platform::dunce_canonicalize(&std::env::temp_dir()).unwrap();
        tempfile::tempdir_in(base).unwrap()
    }

    #[test]
    fn removes_only_manifest_files_and_keeps_user_data() {
        let (_t, p) = setup();
        let d = p.version_dir("photocraft", "0.3.0_abcd").unwrap();
        std::fs::create_dir_all(d.join("sub")).unwrap();
        std::fs::write(d.join("photocraft.exe"), b"MZ").unwrap();
        std::fs::write(d.join("sub").join("lib.dll"), b"x").unwrap();
        std::fs::create_dir_all(d.join("PhotoCraftData")).unwrap();
        std::fs::write(d.join("PhotoCraftData").join("prefs.json"), b"{}").unwrap();

        let manifest = vec![
            ("photocraft.exe".to_string(), 2),
            ("sub/lib.dll".to_string(), 1),
        ];
        let r = remove_version_dir(&p, "photocraft", "0.3.0_abcd", &manifest).unwrap();
        assert_eq!(r.removed_files, 2);
        assert!(!r.dir_removed);
        assert_eq!(r.kept_entries, vec!["PhotoCraftData".to_string()]);
        assert!(d.join("PhotoCraftData").join("prefs.json").exists());
        assert!(!d.join("sub").exists());
    }

    #[test]
    fn removes_dir_when_fully_managed() {
        let (_t, p) = setup();
        let d = p.version_dir("photocraft", "v1").unwrap();
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("a.exe"), b"MZ").unwrap();
        let r = remove_version_dir(&p, "photocraft", "v1", &[("a.exe".into(), 2)]).unwrap();
        assert!(r.dir_removed);
        assert!(!d.exists());
    }

    #[test]
    fn rejects_traversal_in_manifest_and_dir_names() {
        let (t, p) = setup();
        let outside = t.path().join("victim.txt");
        std::fs::write(&outside, b"keep").unwrap();
        let d = p.version_dir("photocraft", "v1").unwrap();
        std::fs::create_dir_all(&d).unwrap();
        let bad = vec![("../../../victim.txt".to_string(), 4)];
        assert!(remove_version_dir(&p, "photocraft", "v1", &bad).is_err());
        assert!(outside.exists());
        assert!(p.version_dir("photocraft", "..").is_err());
        assert!(p.version_dir("../x", "v1").is_err());
        assert!(p.staging_dir("../../x").is_err());
    }

    #[test]
    fn refuses_to_delete_in_roots_without_marker() {
        let (_t, p) = setup();
        let d = p.version_dir("photocraft", "v1").unwrap();
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("a.exe"), b"MZ").unwrap();
        std::fs::remove_file(p.root.join(LIBRARY_MARKER)).unwrap();
        assert!(remove_version_dir(&p, "photocraft", "v1", &[("a.exe".into(), 2)]).is_err());
        assert!(d.join("a.exe").exists());
    }

    #[test]
    fn library_root_validation() {
        let t = canonical_test_tempdir();
        let forbidden = vec![t.path().join("system")];
        std::fs::create_dir_all(&forbidden[0]).unwrap();

        // Fresh folder: accepted, marker + subfolders created.
        let ok = validate_library_root(&t.path().join("Library"), &forbidden).unwrap();
        assert!(ok.join(LIBRARY_MARKER).is_file());
        assert!(ok.join("Apps").is_dir());
        // Re-selecting an existing library is fine even though it is not empty.
        assert!(validate_library_root(&t.path().join("Library"), &forbidden).is_ok());

        // Non-empty foreign folder is refused and left untouched.
        let docs = t.path().join("Docs");
        std::fs::create_dir_all(&docs).unwrap();
        std::fs::write(docs.join("thesis.docx"), b"x").unwrap();
        assert!(validate_library_root(&docs, &forbidden).is_err());
        assert!(!docs.join(LIBRARY_MARKER).exists());

        // Inside a forbidden folder, relative, traversal, drive root.
        assert!(validate_library_root(&forbidden[0].join("x"), &forbidden).is_err());
        assert!(validate_library_root(Path::new("relative\\x"), &forbidden).is_err());
        assert!(
            validate_library_root(&t.path().join("a").join("..").join("b"), &forbidden).is_err()
        );
        #[cfg(windows)]
        {
            assert!(validate_library_root(Path::new("C:\\"), &forbidden).is_err());
            assert!(validate_library_root(Path::new(r"\\server\share\apps"), &forbidden).is_err());
            assert!(validate_library_root(Path::new(r"\\?\C:\x"), &forbidden).is_err());
        }
    }

    #[cfg(windows)]
    #[test]
    fn library_root_rejects_junction_paths() {
        let t = canonical_test_tempdir();
        let real = t.path().join("real");
        std::fs::create_dir_all(&real).unwrap();
        let link = t.path().join("link");
        let out = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&real)
            .output()
            .unwrap();
        assert!(out.status.success());
        assert!(validate_library_root(&link, &[]).is_err());
        assert!(validate_library_root(&link.join("sub"), &[]).is_err());
    }

    #[test]
    fn remove_temp_refuses_non_temp_paths() {
        let (_t, p) = setup();
        let app = p.version_dir("photocraft", "v1").unwrap();
        std::fs::create_dir_all(&app).unwrap();
        assert!(p.remove_temp(&app).is_err());
        assert!(app.exists());
        let s = p.staging_dir("0123456789abcdef").unwrap();
        std::fs::create_dir_all(s.join("x")).unwrap();
        p.remove_temp(&s).unwrap();
        assert!(!s.exists());
    }

    #[cfg(windows)]
    #[test]
    fn does_not_delete_through_junctions() {
        let (t, p) = setup();
        let victim_dir = t.path().join("victim");
        std::fs::create_dir_all(&victim_dir).unwrap();
        std::fs::write(victim_dir.join("data.txt"), b"keep").unwrap();
        let d = p.version_dir("photocraft", "v1").unwrap();
        std::fs::create_dir_all(&d).unwrap();
        // Directory junctions don't need admin rights; mklink is a cmd builtin.
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(d.join("link"))
            .arg(&victim_dir)
            .output()
            .unwrap();
        assert!(status.status.success());
        let r = remove_version_dir(&p, "photocraft", "v1", &[("link/data.txt".into(), 4)]).unwrap();
        assert_eq!(r.removed_files, 0);
        assert!(victim_dir.join("data.txt").exists());
    }
}
