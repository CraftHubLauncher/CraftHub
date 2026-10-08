//! OS-specific primitives. Windows is implemented; Linux and macOS are explicit stubs so
//! that unsupported behaviour fails loudly instead of being silently assumed.

use std::path::{Path, PathBuf};

use crate::error::Result;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use self::windows as imp;

#[cfg(not(windows))]
mod unsupported;
#[cfg(not(windows))]
use self::unsupported as imp;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningProcess {
    pub pid: u32,
    pub image: PathBuf,
}

/// Snapshot of running processes with their executable paths.
pub fn process_snapshot() -> Result<Vec<RunningProcess>> {
    imp::process_images()
}

/// Processes from `snapshot` whose executable image lives inside `dir`.
pub fn processes_in<'a>(snapshot: &'a [RunningProcess], dir: &Path) -> Vec<&'a RunningProcess> {
    let mut prefix = normalize_for_compare(dir);
    if !prefix.ends_with(std::path::MAIN_SEPARATOR) {
        prefix.push(std::path::MAIN_SEPARATOR);
    }
    // Image paths reported by the OS are already final paths; only case-fold them.
    snapshot
        .iter()
        .filter(|p| {
            let s = p.image.to_string_lossy();
            let s = if cfg!(windows) {
                s.to_lowercase()
            } else {
                s.into_owned()
            };
            s.starts_with(&prefix)
        })
        .collect()
}

/// Free bytes available to the current user on the volume containing `path`.
pub fn available_space(path: &Path) -> Result<u64> {
    imp::available_space(path)
}

/// Rejects network, optical and unknown drives for install roots.
pub fn check_local_drive(path: &Path) -> Result<()> {
    imp::check_local_drive(path)
}

/// True if `meta` describes a symlink, junction or other reparse point.
pub fn is_link_like(meta: &std::fs::Metadata) -> bool {
    meta.file_type().is_symlink() || imp::is_reparse_point(meta)
}

/// Starts `exe` as an independent child process (no shell, no arguments).
pub fn spawn_detached(exe: &Path, cwd: &Path) -> Result<u32> {
    imp::spawn_detached(exe, cwd)
}

/// Opens a folder in the system file manager without involving a shell command string.
pub fn open_folder(dir: &Path) -> Result<()> {
    imp::open_folder(dir)
}

/// Windows desktop shortcut helpers. Non-Windows builds fail explicitly so future platform
/// adapters can provide their own native shortcut implementation.
#[cfg(windows)]
mod shortcuts {
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::path::{Path, PathBuf};

    use windows::Win32::Storage::FileSystem::WIN32_FIND_DATAW;
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
        CoTaskMemFree, CoUninitialize, IPersistFile, STGM_READ,
    };
    use windows::Win32::UI::Shell::{
        FOLDERID_Desktop, IShellLinkW, KNOWN_FOLDER_FLAG, SHGetKnownFolderPath,
    };
    use windows::core::{GUID, Interface, PCWSTR};

    use super::super::error::{CoreError, Result};

    const CLSID_SHELL_LINK: GUID = GUID::from_u128(0x00021401_0000_0000_c000_000000000046);
    const OWNERSHIP_PREFIX: &str = "CraftHub managed shortcut:";

    fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
        s.encode_wide().chain(std::iter::once(0)).collect()
    }

    fn text(buf: &[u16]) -> String {
        let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
        OsString::from_wide(&buf[..end])
            .to_string_lossy()
            .into_owned()
    }

    struct ComGuard(bool);
    impl ComGuard {
        fn new() -> Result<Self> {
            let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
            if hr.is_err() {
                return Err(CoreError::Launch(format!(
                    "initializing Windows shortcut support: {hr:?}"
                )));
            }
            Ok(Self(true))
        }
    }
    impl Drop for ComGuard {
        fn drop(&mut self) {
            if self.0 {
                unsafe {
                    CoUninitialize();
                }
            }
        }
    }

    fn load(path: &Path) -> Result<(IShellLinkW, ComGuard)> {
        let com = ComGuard::new()?;
        let link: IShellLinkW =
            unsafe { CoCreateInstance(&CLSID_SHELL_LINK, None, CLSCTX_INPROC_SERVER) }
                .map_err(|e| CoreError::Launch(format!("creating Windows shortcut object: {e}")))?;
        let persist: IPersistFile = link
            .cast()
            .map_err(|e| CoreError::Launch(format!("opening Windows shortcut: {e}")))?;
        let p = wide(path.as_os_str());
        unsafe { persist.Load(PCWSTR(p.as_ptr()), STGM_READ) }
            .map_err(|e| CoreError::Launch(format!("reading Windows shortcut: {e}")))?;
        Ok((link, com))
    }

    fn read(path: &Path) -> Result<(String, PathBuf)> {
        let (link, _com) = load(path)?;
        let mut desc = [0u16; 512];
        let mut target = [0u16; 32_768];
        let mut data: WIN32_FIND_DATAW = unsafe { std::mem::zeroed() };
        unsafe {
            link.GetDescription(&mut desc)
                .map_err(|e| CoreError::Launch(format!("reading shortcut ownership: {e}")))?;
            link.GetPath(&mut target, &mut data, 0)
                .map_err(|e| CoreError::Launch(format!("reading shortcut target: {e}")))?;
        }
        Ok((text(&desc), PathBuf::from(text(&target))))
    }

    pub fn desktop_dir() -> Result<PathBuf> {
        let raw = unsafe { SHGetKnownFolderPath(&FOLDERID_Desktop, KNOWN_FOLDER_FLAG(0), None) }
            .map_err(|e| CoreError::Launch(format!("resolving the Windows Desktop folder: {e}")))?;
        let result = PathBuf::from(unsafe {
            OsString::from_wide(std::slice::from_raw_parts(
                raw.0,
                (0..).find(|i| *raw.0.add(*i) == 0).unwrap(),
            ))
        });
        unsafe {
            CoTaskMemFree(Some(raw.0 as _));
        }
        Ok(result)
    }

    pub fn is_owned(path: &Path, app_id: &str) -> bool {
        read(path).is_ok_and(|(d, _)| d == format!("{OWNERSHIP_PREFIX}{app_id}"))
    }

    pub fn target(path: &Path) -> Result<PathBuf> {
        Ok(read(path)?.1)
    }

    pub fn create_or_update(
        app_name: &str,
        app_id: &str,
        exe: &Path,
        preferred: Option<&Path>,
    ) -> Result<PathBuf> {
        let desktop = desktop_dir()?;
        let desktop_key = |p: &Path| {
            p.to_string_lossy()
                .trim_end_matches(['\\', '/'])
                .to_lowercase()
        };
        let allowed = |p: &Path| {
            p.parent()
                .is_some_and(|parent| desktop_key(parent) == desktop_key(&desktop))
        };
        if let Some(p) = preferred
            && !allowed(p)
        {
            return Err(CoreError::PathSafety(
                "the recorded shortcut is outside the Windows Desktop folder".into(),
            ));
        }
        let safe_name: String = app_name
            .chars()
            .map(|c| {
                if "<>:\"/\\|?*".contains(c) || c.is_control() {
                    '_'
                } else {
                    c
                }
            })
            .collect();
        let safe_name = safe_name.trim().trim_end_matches('.').to_string();
        let base_name = if safe_name.is_empty() {
            app_id.to_string()
        } else {
            safe_name
        };
        let mut path = preferred
            .map(PathBuf::from)
            .unwrap_or_else(|| desktop.join(format!("{base_name}.lnk")));
        if path.exists() && !is_owned(&path, app_id) {
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("CraftHub app");
            for n in 2..=1000 {
                let candidate = desktop.join(format!("{stem} ({n}).lnk"));
                if !candidate.exists() {
                    path = candidate;
                    break;
                }
            }
            if path.exists() {
                return Err(CoreError::Launch(
                    "no safe desktop shortcut filename is available".into(),
                ));
            }
        }
        let com = ComGuard::new()?;
        let link: IShellLinkW =
            unsafe { CoCreateInstance(&CLSID_SHELL_LINK, None, CLSCTX_INPROC_SERVER) }
                .map_err(|e| CoreError::Launch(format!("creating Windows shortcut object: {e}")))?;
        let target = wide(exe.as_os_str());
        let cwd = wide(exe.parent().unwrap_or_else(|| Path::new(".")).as_os_str());
        let description = format!("{OWNERSHIP_PREFIX}{app_id}");
        let desc = wide(std::ffi::OsStr::new(&description));
        unsafe {
            link.SetPath(PCWSTR(target.as_ptr()))
                .map_err(|e| CoreError::Launch(format!("setting shortcut target: {e}")))?;
            link.SetWorkingDirectory(PCWSTR(cwd.as_ptr()))
                .map_err(|e| {
                    CoreError::Launch(format!("setting shortcut working directory: {e}"))
                })?;
            link.SetIconLocation(PCWSTR(target.as_ptr()), 0)
                .map_err(|e| CoreError::Launch(format!("setting shortcut icon: {e}")))?;
            link.SetDescription(PCWSTR(desc.as_ptr()))
                .map_err(|e| CoreError::Launch(format!("setting shortcut ownership: {e}")))?;
        }
        let persist: IPersistFile = link
            .cast()
            .map_err(|e| CoreError::Launch(format!("saving Windows shortcut: {e}")))?;
        let p = wide(path.as_os_str());
        unsafe { persist.Save(PCWSTR(p.as_ptr()), true) }
            .map_err(|e| CoreError::Launch(format!("saving Windows shortcut: {e}")))?;
        drop(com);
        Ok(path)
    }

    pub fn remove_if_owned(path: &Path, app_id: &str) -> Result<bool> {
        if !path.exists() {
            return Ok(false);
        }
        let desktop = desktop_dir()?;
        let same = path.parent().is_some_and(|parent| {
            parent
                .to_string_lossy()
                .eq_ignore_ascii_case(&desktop.to_string_lossy())
        });
        if !same {
            return Err(CoreError::PathSafety(
                "the recorded shortcut is outside the Windows Desktop folder".into(),
            ));
        }
        if !is_owned(path, app_id) {
            return Err(CoreError::PathSafety(
                "the desktop shortcut is not managed by CraftHub".into(),
            ));
        }
        std::fs::remove_file(path).map_err(CoreError::io("removing desktop shortcut"))?;
        Ok(true)
    }
}

#[cfg(windows)]
pub fn create_or_update_shortcut(
    app_name: &str,
    app_id: &str,
    exe: &Path,
    preferred: Option<&Path>,
) -> Result<PathBuf> {
    shortcuts::create_or_update(app_name, app_id, exe, preferred)
}
#[cfg(windows)]
pub fn desktop_dir() -> Result<PathBuf> {
    shortcuts::desktop_dir()
}
#[cfg(windows)]
pub fn is_owned_shortcut(path: &Path, app_id: &str) -> bool {
    shortcuts::is_owned(path, app_id)
}
#[cfg(windows)]
pub fn shortcut_target(path: &Path) -> Result<PathBuf> {
    shortcuts::target(path)
}
#[cfg(windows)]
pub fn remove_owned_shortcut(path: &Path, app_id: &str) -> Result<bool> {
    shortcuts::remove_if_owned(path, app_id)
}

#[cfg(not(windows))]
pub fn desktop_dir() -> Result<PathBuf> {
    Err(crate::error::CoreError::Unsupported(
        "Desktop shortcuts are only implemented on Windows.".into(),
    ))
}
#[cfg(not(windows))]
pub fn create_or_update_shortcut(
    _app_name: &str,
    _app_id: &str,
    _exe: &Path,
    _preferred: Option<&Path>,
) -> Result<PathBuf> {
    desktop_dir()
}
#[cfg(not(windows))]
pub fn is_owned_shortcut(_path: &Path, _app_id: &str) -> bool {
    false
}
#[cfg(not(windows))]
pub fn shortcut_target(_path: &Path) -> Result<PathBuf> {
    desktop_dir()
}
#[cfg(not(windows))]
pub fn remove_owned_shortcut(_path: &Path, _app_id: &str) -> Result<bool> {
    desktop_dir().map(|_| false)
}

/// Canonical, case-folded string form of a path for prefix comparison.
pub fn normalize_for_compare(p: &Path) -> String {
    let canon = dunce_canonicalize(p).unwrap_or_else(|| p.to_path_buf());
    let s = canon.to_string_lossy().into_owned();
    if cfg!(windows) { s.to_lowercase() } else { s }
}

/// `std::fs::canonicalize` without the Windows `\\?\` verbatim prefix for plain drive paths.
pub fn dunce_canonicalize(p: &Path) -> Option<PathBuf> {
    let c = std::fs::canonicalize(p).ok()?;
    #[cfg(windows)]
    {
        let s = c.to_string_lossy();
        if let Some(rest) = s.strip_prefix(r"\\?\")
            && rest.as_bytes().get(1) == Some(&b':')
        {
            return Some(PathBuf::from(rest));
        }
    }
    Some(c)
}
