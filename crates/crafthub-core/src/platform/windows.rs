use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, GetDiskFreeSpaceExW, GetDriveTypeW,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    CREATE_NEW_PROCESS_GROUP, CREATE_UNICODE_ENVIRONMENT, CreateProcessW, OpenProcess,
    PROCESS_INFORMATION, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    QueryFullProcessImageNameW, STARTUPINFOW,
};

// GetDriveTypeW return values (winbase.h).
const DRIVE_REMOVABLE: u32 = 2;
const DRIVE_FIXED: u32 = 3;
const DRIVE_REMOTE: u32 = 4;
const DRIVE_RAMDISK: u32 = 6;

use super::RunningProcess;
use crate::error::{CoreError, Result};

pub fn process_images() -> Result<Vec<RunningProcess>> {
    let mut out = Vec::new();
    let mut buf = vec![0u16; 32_768];
    // SAFETY: standard ToolHelp enumeration; every handle opened here is closed, buffers
    // are sized as declared, and PROCESSENTRY32W is initialised with its dwSize.
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return Err(CoreError::io("listing processes")(
                std::io::Error::last_os_error(),
            ));
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(snap, &mut entry);
        while ok != 0 {
            let pid = entry.th32ProcessID;
            if pid != 0 {
                // Processes we cannot query (other users / elevated) are skipped; apps the
                // user launched from CraftHub run as the same user and are always visible.
                let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
                if !h.is_null() {
                    let mut len = buf.len() as u32;
                    if QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len)
                        != 0
                    {
                        out.push(RunningProcess {
                            pid,
                            image: PathBuf::from(OsString::from_wide(&buf[..len as usize])),
                        });
                    }
                    CloseHandle(h);
                }
            }
            ok = Process32NextW(snap, &mut entry);
        }
        CloseHandle(snap);
    }
    Ok(out)
}

fn wide(p: &Path) -> Vec<u16> {
    p.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

pub fn available_space(path: &Path) -> Result<u64> {
    let w = wide(path);
    let mut avail: u64 = 0;
    // SAFETY: `w` is NUL-terminated and outlives the call; unused out-params are null.
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            w.as_ptr(),
            &mut avail,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(CoreError::io("checking free disk space")(
            std::io::Error::last_os_error(),
        ));
    }
    Ok(avail)
}

pub fn check_local_drive(path: &Path) -> Result<()> {
    use std::path::{Component, Prefix};
    let Some(Component::Prefix(p)) = path.components().next() else {
        return Err(CoreError::InvalidInput(
            "Choose a folder on a local drive.".into(),
        ));
    };
    let Prefix::Disk(letter) = p.kind() else {
        return Err(CoreError::InvalidInput(
            "Choose a folder on a local drive.".into(),
        ));
    };
    let root: Vec<u16> = format!("{}:\\", letter as char)
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: `root` is a NUL-terminated wide string that outlives the call.
    let kind = unsafe { GetDriveTypeW(root.as_ptr()) };
    match kind {
        DRIVE_FIXED | DRIVE_REMOVABLE | DRIVE_RAMDISK => Ok(()),
        DRIVE_REMOTE => Err(CoreError::InvalidInput(
            "Network drives are not supported for installing apps. Choose a local drive.".into(),
        )),
        _ => Err(CoreError::InvalidInput(
            "This drive can't hold installed apps. Choose a local fixed or removable drive.".into(),
        )),
    }
}

pub fn is_reparse_point(meta: &std::fs::Metadata) -> bool {
    meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

/// Starts `exe` with no arguments, no shell and — unlike `std::process::Command` —
/// **without inheriting any handles** from CraftHub (pipes, log files, the database).
pub fn spawn_detached(exe: &Path, cwd: &Path) -> Result<u32> {
    let exe_s = exe.as_os_str().to_string_lossy();
    if exe_s.contains('"') {
        return Err(CoreError::Launch("invalid executable path".into()));
    }
    let app = wide(exe);
    // The command line is just the quoted program path (argv[0]); no arguments.
    let mut cmdline: Vec<u16> = format!("\"{exe_s}\"")
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let dir = wide(cwd);
    // SAFETY: zero-initialised Win32 structs with `cb` set; all strings are NUL-terminated
    // and outlive the call; both returned handles are closed.
    unsafe {
        let mut si: STARTUPINFOW = std::mem::zeroed();
        si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
        let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
        let ok = CreateProcessW(
            app.as_ptr(),
            cmdline.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0, // bInheritHandles = FALSE
            CREATE_NEW_PROCESS_GROUP | CREATE_UNICODE_ENVIRONMENT,
            std::ptr::null(),
            dir.as_ptr(),
            &si,
            &mut pi,
        );
        if ok == 0 {
            return Err(CoreError::Launch(
                std::io::Error::last_os_error().to_string(),
            ));
        }
        CloseHandle(pi.hThread);
        CloseHandle(pi.hProcess);
        Ok(pi.dwProcessId)
    }
}

pub fn open_folder(dir: &Path) -> Result<()> {
    // explorer.exe receives the path as a single argument; no shell parsing is involved.
    let windir = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    Command::new(Path::new(&windir).join("explorer.exe"))
        .arg(dir)
        .spawn()
        .map(|_| ())
        .map_err(|e| CoreError::Launch(e.to_string()))
}
