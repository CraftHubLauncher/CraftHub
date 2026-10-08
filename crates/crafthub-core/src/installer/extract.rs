//! Hardened ZIP extraction for portable app archives.
//!
//! The archive is fully validated before a single byte is written: entry count, names
//! (traversal, absolute/drive/UNC paths, Windows reserved names, ADS, control chars),
//! symlinks, encryption, compression method, case-insensitive collisions, file/dir
//! conflicts, declared sizes and compression ratios. During extraction each entry is
//! capped at its declared size and CRC-checked by the zip reader.

use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use zip::CompressionMethod;
use zip::ZipArchive;

use crate::error::{CoreError, Result};
use crate::validate::is_safe_relative_path;

#[derive(Debug, Clone, Copy)]
pub struct ExtractLimits {
    pub max_entries: usize,
    pub max_total_bytes: u64,
    pub max_entry_bytes: u64,
    /// Maximum uncompressed/compressed ratio for entries larger than 1 MiB.
    pub max_ratio: u64,
}

impl Default for ExtractLimits {
    fn default() -> Self {
        Self {
            max_entries: 20_000,
            max_total_bytes: 4 * 1024 * 1024 * 1024,
            max_entry_bytes: 2 * 1024 * 1024 * 1024,
            max_ratio: 200,
        }
    }
}

struct PlannedEntry {
    index: usize,
    rel: String,
    is_dir: bool,
    size: u64,
}

fn unsafe_archive(msg: impl Into<String>) -> CoreError {
    CoreError::UnsafeArchive(msg.into())
}

/// Validates the archive and returns the total uncompressed bytes of files under `root`.
pub fn inspect(zip_path: &Path, root: &str, limits: &ExtractLimits) -> Result<u64> {
    let mut archive = open(zip_path)?;
    Ok(plan(&mut archive, root, limits)?
        .iter()
        .map(|e| e.size)
        .sum())
}

/// Extracts the contents of `<root>/` from the archive into the empty directory `dest`,
/// stripping the root folder. Returns `(relative path, size)` for every file written.
pub fn extract_portable_zip(
    zip_path: &Path,
    dest: &Path,
    root: &str,
    limits: &ExtractLimits,
    is_cancelled: &dyn Fn() -> bool,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<Vec<(String, u64)>> {
    let mut archive = open(zip_path)?;
    let entries = plan(&mut archive, root, limits)?;
    let total: u64 = entries.iter().map(|e| e.size).sum();

    std::fs::create_dir(dest).map_err(CoreError::io("creating staging folder"))?;
    let mut written_total = 0u64;
    let mut files = Vec::new();
    let mut buf = vec![0u8; 256 * 1024];
    progress(0, total);

    for e in &entries {
        if is_cancelled() {
            return Err(CoreError::Cancelled);
        }
        let target = join_rel(dest, &e.rel);
        if e.is_dir {
            std::fs::create_dir_all(&target).map_err(CoreError::io("creating folder"))?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(CoreError::io("creating folder"))?;
        }
        let mut out = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .map_err(CoreError::io(format!("creating {}", e.rel)))?;
        let entry = archive
            .by_index(e.index)
            .map_err(|err| unsafe_archive(format!("cannot read {}: {err}", e.rel)))?;
        // Read at most one byte more than declared so overruns are detected.
        let mut reader = entry.take(e.size + 1);
        let mut written = 0u64;
        loop {
            let n = reader
                .read(&mut buf)
                .map_err(|err| unsafe_archive(format!("corrupt data in {}: {err}", e.rel)))?;
            if n == 0 {
                break;
            }
            written += n as u64;
            if written > e.size {
                return Err(unsafe_archive(format!("{} is larger than declared", e.rel)));
            }
            out.write_all(&buf[..n])
                .map_err(CoreError::io(format!("writing {}", e.rel)))?;
            written_total += n as u64;
            if is_cancelled() {
                return Err(CoreError::Cancelled);
            }
        }
        if written != e.size {
            return Err(unsafe_archive(format!("{} is truncated", e.rel)));
        }
        out.sync_all()
            .map_err(CoreError::io(format!("writing {}", e.rel)))?;
        progress(written_total, total);
        files.push((e.rel.clone(), e.size));
    }
    Ok(files)
}

fn open(zip_path: &Path) -> Result<ZipArchive<File>> {
    let f = File::open(zip_path).map_err(CoreError::io("opening archive"))?;
    ZipArchive::new(f).map_err(|e| unsafe_archive(format!("not a valid ZIP archive: {e}")))
}

fn join_rel(base: &Path, rel: &str) -> PathBuf {
    rel.split('/').fold(base.to_path_buf(), |p, c| p.join(c))
}

fn plan(
    archive: &mut ZipArchive<File>,
    root: &str,
    limits: &ExtractLimits,
) -> Result<Vec<PlannedEntry>> {
    if archive.len() > limits.max_entries {
        return Err(unsafe_archive(format!(
            "too many entries ({})",
            archive.len()
        )));
    }
    let root_prefix = format!("{root}/");
    let mut seen = HashSet::new();
    let mut file_keys = HashSet::new();
    let mut dir_keys = HashSet::new();
    let mut total = 0u64;
    let mut out = Vec::new();

    for i in 0..archive.len() {
        let entry = archive
            .by_index_raw(i)
            .map_err(|e| unsafe_archive(format!("unreadable entry #{i}: {e}")))?;
        let name = entry.name().to_string();
        if entry.encrypted() {
            return Err(unsafe_archive(format!("{name} is encrypted")));
        }
        if entry.is_symlink() {
            return Err(unsafe_archive(format!("{name} is a symbolic link")));
        }
        if let Some(mode) = entry.unix_mode() {
            let kind = mode & 0o170000;
            if kind != 0 && kind != 0o100000 && kind != 0o040000 {
                return Err(unsafe_archive(format!("{name} is a special file")));
            }
        }
        if !matches!(
            entry.compression(),
            CompressionMethod::Stored | CompressionMethod::Deflated
        ) {
            return Err(unsafe_archive(format!(
                "{name} uses an unsupported compression method"
            )));
        }
        let is_dir = entry.is_dir();
        let trimmed = name.strip_suffix('/').unwrap_or(&name);
        if name.contains('\\') || !is_safe_relative_path(trimmed) {
            return Err(unsafe_archive(format!("unsafe entry name {name:?}")));
        }
        if name == root_prefix {
            continue;
        }
        let Some(rel) = trimmed.strip_prefix(&root_prefix) else {
            return Err(CoreError::Layout(format!(
                "entry {name:?} is outside the expected folder {root_prefix}"
            )));
        };
        let key = rel.to_lowercase();
        if !seen.insert(key.clone()) {
            return Err(unsafe_archive(format!("duplicate entry {name:?}")));
        }
        // Record every ancestor as a directory to catch file/dir conflicts.
        let mut acc = String::new();
        let parts: Vec<&str> = key.split('/').collect();
        for p in &parts[..parts.len() - 1] {
            if !acc.is_empty() {
                acc.push('/');
            }
            acc.push_str(p);
            dir_keys.insert(acc.clone());
        }
        let size = if is_dir { 0 } else { entry.size() };
        if is_dir {
            dir_keys.insert(key);
        } else {
            if size > limits.max_entry_bytes {
                return Err(unsafe_archive(format!("{name} is too large")));
            }
            let comp = entry.compressed_size();
            if size > 1024 * 1024 && (comp == 0 || size / comp.max(1) > limits.max_ratio) {
                return Err(unsafe_archive(format!(
                    "{name} has a suspicious compression ratio"
                )));
            }
            total = total
                .checked_add(size)
                .filter(|t| *t <= limits.max_total_bytes)
                .ok_or_else(|| unsafe_archive("archive expands beyond the allowed size"))?;
            file_keys.insert(key);
        }
        out.push(PlannedEntry {
            index: i,
            rel: rel.to_string(),
            is_dir,
            size,
        });
    }
    if let Some(k) = file_keys.iter().find(|k| dir_keys.contains(*k)) {
        return Err(unsafe_archive(format!("{k} is both a file and a folder")));
    }
    if out.iter().all(|e| e.is_dir) {
        return Err(CoreError::Layout(format!(
            "no files found under {root_prefix}"
        )));
    }
    Ok(out)
}

/// Confirms `path` is a Windows PE image for x86-64 (`MZ` header, `PE\0\0`, machine 0x8664).
pub fn verify_x64_pe(path: &Path) -> Result<()> {
    let bad = |m: &str| CoreError::Layout(format!("{}: {m}", path.display()));
    let mut f = File::open(path).map_err(|_| bad("executable is missing"))?;
    let mut head = [0u8; 4096];
    let n = f
        .read(&mut head)
        .map_err(CoreError::io("reading executable"))?;
    if n < 0x40 || &head[..2] != b"MZ" {
        return Err(bad("not a Windows executable"));
    }
    let pe = u32::from_le_bytes([head[0x3c], head[0x3d], head[0x3e], head[0x3f]]) as usize;
    if pe + 6 > n || &head[pe..pe + 4] != b"PE\0\0" {
        return Err(bad("invalid PE header"));
    }
    let machine = u16::from_le_bytes([head[pe + 4], head[pe + 5]]);
    if machine != 0x8664 {
        return Err(bad(&format!(
            "built for machine type 0x{machine:04x}, not x64"
        )));
    }
    Ok(())
}
