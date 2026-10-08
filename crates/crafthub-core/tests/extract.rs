//! Malicious and malformed archive handling in the extractor (no network).

mod common;

use std::io::Write;

use common::{fake_x64_exe, zip_of};
use crafthub_core::CoreError;
use crafthub_core::installer::extract::{
    ExtractLimits, extract_portable_zip, inspect, verify_x64_pe,
};
use zip::write::SimpleFileOptions;

const ROOT: &str = "app-1.0.0-windows-x64-portable";

fn run(
    zip: &[u8],
    limits: ExtractLimits,
) -> (tempfile::TempDir, Result<Vec<(String, u64)>, CoreError>) {
    let t = tempfile::tempdir().unwrap();
    let zp = t.path().join("a.zip");
    std::fs::write(&zp, zip).unwrap();
    let dest = t.path().join("out");
    let r = extract_portable_zip(&zp, &dest, ROOT, &limits, &|| false, &mut |_, _| {});
    (t, r)
}

fn rejects(zip: Vec<u8>) -> CoreError {
    let (t, r) = run(&zip, ExtractLimits::default());
    let err = r.expect_err("archive should be rejected");
    // Validation happens before any write: nothing was extracted.
    assert!(
        !t.path().join("out").exists(),
        "files were written before validation finished"
    );
    err
}

#[test]
fn extracts_valid_archive_and_strips_root() {
    let exe = fake_x64_exe("ok");
    let zip = zip_of(&[
        (&format!("{ROOT}/"), b""),
        (&format!("{ROOT}/app.exe"), &exe),
        (&format!("{ROOT}/sub/dir/file.txt"), b"hello"),
    ]);
    let (t, r) = run(&zip, ExtractLimits::default());
    let files = r.unwrap();
    assert_eq!(files.len(), 2);
    let out = t.path().join("out");
    assert!(out.join("app.exe").is_file());
    assert_eq!(
        std::fs::read(out.join("sub").join("dir").join("file.txt")).unwrap(),
        b"hello"
    );
    verify_x64_pe(&out.join("app.exe")).unwrap();
    assert!(verify_x64_pe(&out.join("sub").join("dir").join("file.txt")).is_err());
}

#[test]
fn rejects_traversal_and_absolute_paths() {
    for name in [
        format!("{ROOT}/../evil.txt"),
        format!("{ROOT}/a/../../evil.txt"),
        "/etc/passwd".to_string(),
        "C:/Windows/evil.dll".to_string(),
        format!("{ROOT}/C:evil"),
        format!("{ROOT}\\..\\evil.txt"),
        format!("{ROOT}/file.txt:stream"),
        format!("{ROOT}/CON"),
        format!("{ROOT}/nul.txt"),
        format!("{ROOT}/trailing."),
        "//server/share/evil".to_string(),
    ] {
        let zip = zip_of(&[(&format!("{ROOT}/app.exe"), b"MZ"), (&name, b"x")]);
        let err = rejects(zip);
        assert!(
            matches!(err, CoreError::UnsafeArchive(_) | CoreError::Layout(_)),
            "{name}: {err:?}"
        );
    }
}

#[test]
fn rejects_entries_outside_root() {
    let zip = zip_of(&[
        (&format!("{ROOT}/app.exe"), b"MZ"),
        ("other-root/x.txt", b"x"),
    ]);
    assert!(matches!(rejects(zip), CoreError::Layout(_)));
}

#[test]
fn rejects_symlinks() {
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let o = SimpleFileOptions::default();
    w.start_file(format!("{ROOT}/app.exe"), o).unwrap();
    w.write_all(b"MZ").unwrap();
    w.add_symlink(format!("{ROOT}/link"), "C:/Windows/System32", o)
        .unwrap();
    let zip = w.finish().unwrap().into_inner();
    assert!(matches!(rejects(zip), CoreError::UnsafeArchive(_)));
}

#[test]
fn rejects_case_insensitive_collisions_and_file_dir_conflicts() {
    let zip = zip_of(&[
        (&format!("{ROOT}/App.exe"), b"MZ"),
        (&format!("{ROOT}/app.exe"), b"MZ"),
    ]);
    assert!(matches!(rejects(zip), CoreError::UnsafeArchive(_)));
    let zip = zip_of(&[
        (&format!("{ROOT}/a"), b"file"),
        (&format!("{ROOT}/a/b.txt"), b"x"),
    ]);
    assert!(matches!(rejects(zip), CoreError::UnsafeArchive(_)));
}

#[test]
fn rejects_compression_bombs_and_oversize() {
    // 50 MiB of zeros compresses ~1000:1.
    let zeros = vec![0u8; 50 * 1024 * 1024];
    let zip = zip_of(&[(&format!("{ROOT}/bomb.bin"), &zeros)]);
    assert!(matches!(rejects(zip.clone()), CoreError::UnsafeArchive(_)));

    let small = zip_of(&[(&format!("{ROOT}/a.bin"), &[7u8; 4096])]);
    let limits = ExtractLimits {
        max_total_bytes: 1024,
        ..ExtractLimits::default()
    };
    let (_t, r) = run(&small, limits);
    assert!(matches!(r, Err(CoreError::UnsafeArchive(_))));
}

#[test]
fn rejects_too_many_entries() {
    let names: Vec<String> = (0..50).map(|i| format!("{ROOT}/f{i}.txt")).collect();
    let entries: Vec<(&str, &[u8])> = names.iter().map(|n| (n.as_str(), &b"x"[..])).collect();
    let zip = zip_of(&entries);
    let limits = ExtractLimits {
        max_entries: 10,
        ..ExtractLimits::default()
    };
    let (_t, r) = run(&zip, limits);
    assert!(matches!(r, Err(CoreError::UnsafeArchive(_))));
}

#[test]
fn rejects_corrupt_and_non_zip_input() {
    let (_t, r) = run(b"this is not a zip file", ExtractLimits::default());
    assert!(matches!(r, Err(CoreError::UnsafeArchive(_))));

    let mut zip = zip_of(&[(&format!("{ROOT}/data.bin"), &[1u8; 100_000])]);
    // Flip bytes in the compressed payload (after the local header) to break the CRC/stream.
    for b in &mut zip[200..260] {
        *b ^= 0xff;
    }
    let (_t, r) = run(&zip, ExtractLimits::default());
    assert!(r.is_err());
}

#[test]
fn inspect_reports_expanded_size() {
    let zip = zip_of(&[
        (&format!("{ROOT}/a.txt"), b"12345"),
        (&format!("{ROOT}/b.txt"), b"678"),
    ]);
    let t = tempfile::tempdir().unwrap();
    let zp = t.path().join("a.zip");
    std::fs::write(&zp, zip).unwrap();
    assert_eq!(inspect(&zp, ROOT, &ExtractLimits::default()).unwrap(), 8);
}

#[test]
fn cancellation_stops_extraction() {
    let zip = zip_of(&[
        (&format!("{ROOT}/a.txt"), b"1"),
        (&format!("{ROOT}/b.txt"), b"2"),
    ]);
    let t = tempfile::tempdir().unwrap();
    let zp = t.path().join("a.zip");
    std::fs::write(&zp, zip).unwrap();
    let r = extract_portable_zip(
        &zp,
        &t.path().join("out"),
        ROOT,
        &ExtractLimits::default(),
        &|| true,
        &mut |_, _| {},
    );
    assert!(matches!(r, Err(CoreError::Cancelled)));
}
