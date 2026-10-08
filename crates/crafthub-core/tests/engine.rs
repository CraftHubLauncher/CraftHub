#![cfg(all(windows, target_arch = "x86_64"))]
//! End-to-end engine tests against a local mock GitHub (no network).

mod common;

use std::time::Duration;

use common::*;
use crafthub_core::CoreError;
use crafthub_core::engine::{AppStatus, Engine, FailPoint, Phase, null_sink};
use crafthub_core::releases::FetchSource;
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

fn view(e: &Engine, id: &str) -> crafthub_core::AppView {
    e.app_view(id).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn install_update_rollback_uninstall_lifecycle() {
    let h = harness(&[FixtureRelease::good("1.0.0")]).await;
    let e = &h.engine;

    // Before any check: listed, not installed, no fabricated version.
    let v = view(e, "testapp");
    assert_eq!(v.status, AppStatus::NotInstalled);
    assert!(v.latest.is_none());

    e.check_for_updates(None).await.unwrap();
    let v = view(e, "testapp");
    assert_eq!(v.latest.as_ref().unwrap().version, "1.0.0");
    assert_eq!(v.check.source, Some(FetchSource::Network));
    let nr = view(e, "noreleases");
    assert_eq!(nr.status, AppStatus::Unavailable);

    let (sink, events) = collecting_sink();
    let out = e.install("testapp", None, sink).await.unwrap();
    assert_eq!(out.version, "1.0.0");
    assert_eq!(out.verification, "github-digest+sha256sums");
    let phases: Vec<Phase> = events.lock().unwrap().iter().map(|e| e.phase).collect();
    for p in [
        Phase::Resolving,
        Phase::Downloading,
        Phase::Verifying,
        Phase::Extracting,
        Phase::Activating,
        Phase::Completed,
    ] {
        assert!(phases.contains(&p), "missing phase {p:?}");
    }
    let installed = std::path::PathBuf::from(&out.path);
    assert!(installed.join("testapp.exe").exists());
    assert!(installed.join("data").join("fonts").join("a.txt").exists());
    assert!(
        !installed.join("portable.txt").exists(),
        "audited stripFiles are removed"
    );
    h.assert_temp_clean();
    assert_eq!(view(e, "testapp").status, AppStatus::Installed);

    // Same version again is refused honestly.
    assert!(matches!(
        e.install("testapp", Some("1.0.0"), null_sink()).await,
        Err(CoreError::AlreadyInstalled(_))
    ));

    // New release → update available → update keeps the old version for rollback.
    h.set_releases(&[FixtureRelease::good("1.1.0"), FixtureRelease::good("1.0.0")])
        .await;
    e.check_for_updates(Some("testapp")).await.unwrap();
    let v = view(e, "testapp");
    assert_eq!(v.status, AppStatus::UpdateAvailable);
    assert!(v.update_available);
    let up = e.update("testapp", null_sink()).await.unwrap();
    assert_eq!(up.previous_version.as_deref(), Some("1.0.0"));
    assert_eq!(h.version_dirs().len(), 2);
    let v = view(e, "testapp");
    assert_eq!(v.installed.as_ref().unwrap().version, "1.1.0");
    assert_eq!(
        v.installed.as_ref().unwrap().previous_version.as_deref(),
        Some("1.0.0")
    );

    // Rollback switches to the retained version and back.
    assert_eq!(e.rollback("testapp").unwrap().version, "1.0.0");
    assert_eq!(e.rollback("testapp").unwrap().version, "1.1.0");

    // A third version evicts the oldest (retention: one previous).
    h.set_releases(&[FixtureRelease::good("1.2.0")]).await;
    e.update("testapp", null_sink()).await.unwrap();
    assert_eq!(h.version_dirs().len(), 2);

    // User data written inside the install folder survives uninstall.
    let active = std::path::PathBuf::from(view(e, "testapp").installed.unwrap().path);
    std::fs::create_dir_all(active.join("TestAppData")).unwrap();
    std::fs::write(active.join("TestAppData").join("prefs.json"), b"{}").unwrap();
    let un = e.uninstall("testapp").unwrap();
    assert!(un.removed_files > 0);
    assert_eq!(un.kept_entries, vec!["TestAppData".to_string()]);
    assert!(active.join("TestAppData").join("prefs.json").exists());
    assert!(!active.join("testapp.exe").exists());
    assert_eq!(view(e, "testapp").status, AppStatus::NotInstalled);

    let hist = e.events(50).unwrap();
    assert!(
        hist.iter()
            .any(|ev| ev.kind == "uninstall" && ev.outcome == "success")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn checksum_mismatch_prevents_activation_and_preserves_existing() {
    let h = harness(&[FixtureRelease::good("1.0.0")]).await;
    h.engine
        .install("testapp", None, null_sink())
        .await
        .unwrap();

    let mut bad = FixtureRelease::good("1.1.0");
    bad.digest = Some(format!("sha256:{}", "0".repeat(64)));
    h.set_releases(&[bad]).await;
    let err = h.engine.update("testapp", null_sink()).await.unwrap_err();
    assert!(matches!(err, CoreError::Integrity(_)), "{err:?}");
    let v = view(&h.engine, "testapp");
    assert_eq!(v.installed.unwrap().version, "1.0.0");
    assert_eq!(h.version_dirs().len(), 1);
    h.assert_temp_clean();
}

#[tokio::test(flavor = "multi_thread")]
async fn checksum_file_disagreement_blocks_install() {
    let mut r = FixtureRelease::good("1.0.0");
    r.sums = Some(format!("{}  {}\n", "a".repeat(64), r.asset_name()));
    let h = harness(&[r]).await;
    let err = h
        .engine
        .install("testapp", None, null_sink())
        .await
        .unwrap_err();
    assert!(matches!(err, CoreError::Integrity(_)), "{err:?}");
    assert!(h.version_dirs().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn missing_digest_is_not_installable() {
    let mut r = FixtureRelease::good("1.0.0");
    r.digest = None;
    let h = harness(&[r]).await;
    h.engine.check_for_updates(None).await.unwrap();
    let v = view(&h.engine, "testapp");
    assert_eq!(v.status, AppStatus::Unavailable);
    assert!(v.unsupported_reason.unwrap().contains("SHA-256"));
    assert!(
        h.engine
            .install("testapp", None, null_sink())
            .await
            .is_err()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn zip_slip_archive_is_rejected_without_writing_outside() {
    let root = "testapp-1.0.0-windows-x64-portable";
    let exe = fake_x64_exe("x");
    let zip = zip_of(&[
        (&format!("{root}/testapp.exe"), &exe),
        (&format!("{root}/../../../../escaped.txt"), b"pwned"),
    ]);
    let h = harness(&[FixtureRelease::with_zip("1.0.0", zip)]).await;
    let err = h
        .engine
        .install("testapp", None, null_sink())
        .await
        .unwrap_err();
    assert!(matches!(err, CoreError::UnsafeArchive(_)), "{err:?}");
    for p in walk(h.dir.path()) {
        assert!(
            !p.ends_with("escaped.txt"),
            "file escaped to {}",
            p.display()
        );
    }
    assert!(h.version_dirs().is_empty());
    h.assert_temp_clean();
}

#[tokio::test(flavor = "multi_thread")]
async fn wrong_layout_and_wrong_architecture_are_rejected() {
    // Executable missing from the audited location.
    let root = "testapp-1.0.0-windows-x64-portable";
    let zip = zip_of(&[(&format!("{root}/other.exe"), &fake_x64_exe("x"))]);
    let h = harness(&[FixtureRelease::with_zip("1.0.0", zip)]).await;
    assert!(matches!(
        h.engine.install("testapp", None, null_sink()).await,
        Err(CoreError::Layout(_))
    ));

    // Flat archive without the expected root folder.
    let zip = zip_of(&[("testapp.exe", &fake_x64_exe("x"))]);
    h.set_releases(&[FixtureRelease::with_zip("1.0.0", zip)])
        .await;
    assert!(matches!(
        h.engine.install("testapp", None, null_sink()).await,
        Err(CoreError::Layout(_))
    ));

    // ARM64 binary in an x64 package.
    let mut arm = fake_x64_exe("x");
    arm[0x44..0x46].copy_from_slice(&0xAA64u16.to_le_bytes());
    let zip = zip_of(&[(&format!("{root}/testapp.exe"), &arm)]);
    h.set_releases(&[FixtureRelease::with_zip("1.0.0", zip)])
        .await;
    assert!(matches!(
        h.engine.install("testapp", None, null_sink()).await,
        Err(CoreError::Layout(_))
    ));
    assert!(h.version_dirs().is_empty());
    h.assert_temp_clean();
}

#[tokio::test(flavor = "multi_thread")]
async fn truncated_download_is_rejected() {
    // The API advertises the full size; the download serves only part of the file.
    let r = FixtureRelease::good("1.0.0");
    let h = harness(&[FixtureRelease::good("1.0.0")]).await;
    let dl = format!("/{REPO}/releases/download/v1.0.0/{}", r.asset_name());
    Mock::given(method("GET"))
        .and(path(dl))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(r.zip[..r.zip.len() / 2].to_vec()))
        .with_priority(1)
        .mount(&h.server)
        .await;
    let err = h
        .engine
        .install("testapp", None, null_sink())
        .await
        .unwrap_err();
    assert!(matches!(err, CoreError::Integrity(_)), "{err:?}");
    h.assert_temp_clean();
}

#[tokio::test(flavor = "multi_thread")]
async fn redirect_to_untrusted_origin_is_blocked() {
    let h = harness(&[FixtureRelease::good("1.0.0")]).await;
    let dl = format!("/{REPO}/releases/download/v1.0.0/testapp-1.0.0-windows-x64-portable.zip");
    // localhost (not 127.0.0.1) is outside the test allowlist.
    let evil = format!(
        "{}{}",
        h.server.uri().replace("127.0.0.1", "localhost"),
        "/evil.zip"
    );
    Mock::given(method("GET"))
        .and(path(dl))
        .respond_with(ResponseTemplate::new(302).insert_header("location", evil.as_str()))
        .with_priority(1)
        .mount(&h.server)
        .await;
    let err = h
        .engine
        .install("testapp", None, null_sink())
        .await
        .unwrap_err();
    assert!(matches!(err, CoreError::UntrustedUrl(_)), "{err:?}");
    h.assert_temp_clean();
}

#[tokio::test(flavor = "multi_thread")]
async fn failure_before_commit_keeps_previous_version() {
    let h = harness(&[FixtureRelease::good("1.0.0")]).await;
    h.engine
        .install("testapp", None, null_sink())
        .await
        .unwrap();
    h.set_releases(&[FixtureRelease::good("1.1.0")]).await;
    h.engine.set_fail_point(Some(FailPoint::ErrorBeforeCommit));
    assert!(h.engine.update("testapp", null_sink()).await.is_err());
    h.engine.set_fail_point(None);
    assert_eq!(
        view(&h.engine, "testapp").installed.unwrap().version,
        "1.0.0"
    );
    assert_eq!(h.version_dirs().len(), 1, "half-activated version removed");
    h.assert_temp_clean();
}

#[tokio::test(flavor = "multi_thread")]
async fn crash_mid_activation_is_recovered_on_restart() {
    let h = harness(&[FixtureRelease::good("1.0.0")]).await;
    h.engine
        .install("testapp", None, null_sink())
        .await
        .unwrap();
    h.set_releases(&[FixtureRelease::good("1.1.0")]).await;
    h.engine.set_fail_point(Some(FailPoint::CrashBeforeCommit));
    assert!(h.engine.update("testapp", null_sink()).await.is_err());
    // "Power loss": the orphaned new version and temp files are still on disk.
    assert_eq!(h.version_dirs().len(), 2);

    // Restart on the same database and folders.
    let restarted = Engine::new(engine_config(&h.server, h.dir.path())).unwrap();
    let v = restarted.app_view("testapp").unwrap();
    assert_eq!(v.installed.unwrap().version, "1.0.0");
    assert_eq!(h.version_dirs().len(), 1);
    h.assert_temp_clean();
    let hist = restarted.events(10).unwrap();
    assert!(hist.iter().any(|e| e.outcome == "interrupted"));
}

#[tokio::test(flavor = "multi_thread")]
async fn concurrent_operations_on_one_app_are_rejected() {
    let h = harness(&[FixtureRelease::good("1.0.0")]).await;
    let r = FixtureRelease::good("1.0.0");
    let dl = format!("/{REPO}/releases/download/v1.0.0/{}", r.asset_name());
    Mock::given(method("GET"))
        .and(path(dl))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(r.zip.clone())
                .set_delay(Duration::from_millis(800)),
        )
        .with_priority(1)
        .mount(&h.server)
        .await;
    let (a, b) = tokio::join!(h.engine.install("testapp", None, null_sink()), async {
        tokio::time::sleep(Duration::from_millis(200)).await;
        h.engine.install("testapp", None, null_sink()).await
    });
    assert!(a.is_ok());
    assert!(matches!(b, Err(CoreError::Busy(_))), "{b:?}");
    assert_eq!(h.version_dirs().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn cancellation_leaves_nothing_behind() {
    let h = harness(&[FixtureRelease::good("1.0.0")]).await;
    let r = FixtureRelease::good("1.0.0");
    let dl = format!("/{REPO}/releases/download/v1.0.0/{}", r.asset_name());
    Mock::given(method("GET"))
        .and(path(dl))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(r.zip.clone())
                .set_delay(Duration::from_secs(5)),
        )
        .with_priority(1)
        .mount(&h.server)
        .await;
    let (sink, events) = collecting_sink();
    let engine = h.engine.clone();
    let task = tokio::spawn(async move { engine.install("testapp", None, sink).await });
    let op_id = loop {
        tokio::time::sleep(Duration::from_millis(20)).await;
        if let Some(e) = events.lock().unwrap().first() {
            break e.op_id.clone();
        }
    };
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(h.engine.cancel(&op_id));
    let res = task.await.unwrap();
    assert!(matches!(res, Err(CoreError::Cancelled)), "{res:?}");
    assert!(
        events
            .lock()
            .unwrap()
            .iter()
            .any(|e| e.phase == Phase::Cancelled)
    );
    assert!(h.version_dirs().is_empty());
    h.assert_temp_clean();
}

#[tokio::test(flavor = "multi_thread")]
async fn rate_limit_and_offline_use_cache_honestly() {
    let h = harness(&[FixtureRelease::good("1.0.0")]).await;
    h.engine.check_for_updates(None).await.unwrap();
    h.engine
        .install("testapp", None, null_sink())
        .await
        .unwrap();

    // ETag revalidation: GitHub answers 304 and cached data is reused.
    h.server.reset().await;
    Mock::given(method("GET"))
        .and(path(format!("/repos/{REPO}/releases")))
        .respond_with(ResponseTemplate::new(304))
        .mount(&h.server)
        .await;
    h.engine.check_for_updates(Some("testapp")).await.unwrap();
    let v = view(&h.engine, "testapp");
    assert_eq!(v.check.source, Some(FetchSource::NotModified));
    assert_eq!(v.latest.unwrap().version, "1.0.0");

    // Rate limited: stale cache is shown with a warning, not as fresh data.
    h.server.reset().await;
    Mock::given(method("GET"))
        .and(path(format!("/repos/{REPO}/releases")))
        .respond_with(
            ResponseTemplate::new(403)
                .insert_header("x-ratelimit-remaining", "0")
                .insert_header("x-ratelimit-reset", "0"),
        )
        .mount(&h.server)
        .await;
    h.engine.check_for_updates(Some("testapp")).await.unwrap();
    let v = view(&h.engine, "testapp");
    assert_eq!(v.check.source, Some(FetchSource::StaleCache));
    assert!(v.check.warning.unwrap().contains("Try again"));

    // Offline: server gone. Installed app view still works from the registry/cache.
    drop(h.server);
    let v = h.engine.check_for_updates(Some("testapp")).await.unwrap();
    let t = v.iter().find(|a| a.id == "testapp").unwrap();
    assert_eq!(t.status, AppStatus::Installed);
    assert!(t.check.warning.is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn rate_limit_without_cache_is_reported_as_error() {
    let h = harness(&[]).await;
    h.server.reset().await;
    Mock::given(method("GET"))
        .and(path(format!("/repos/{REPO}/releases")))
        .respond_with(ResponseTemplate::new(403).insert_header("x-ratelimit-remaining", "0"))
        .mount(&h.server)
        .await;
    h.engine.check_for_updates(Some("testapp")).await.unwrap();
    let v = view(&h.engine, "testapp");
    assert!(v.check.error.unwrap().contains("rate limit"));
    assert!(v.latest.is_none(), "no release data is invented");
    assert!(matches!(
        h.engine.install("testapp", None, null_sink()).await,
        Err(CoreError::RateLimited(_))
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn update_all_reports_results() {
    let h = harness(&[FixtureRelease::good("1.0.0")]).await;
    h.engine
        .install("testapp", None, null_sink())
        .await
        .unwrap();
    h.set_releases(&[FixtureRelease::good("2.0.0")]).await;
    let s = h.engine.update_all(null_sink()).await.unwrap();
    assert_eq!(s.updated.len(), 1);
    assert_eq!(s.updated[0].to.as_deref(), Some("2.0.0"));
    assert!(s.failed.is_empty());
    // Nothing left to do: up-to-date apps are not listed.
    let s = h.engine.update_all(null_sink()).await.unwrap();
    assert!(s.updated.is_empty() && s.skipped.is_empty() && s.failed.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn stable_channel_ignores_prereleases() {
    let mut beta = FixtureRelease::good("2.0.0-rc.1");
    beta.prerelease = false; // upstream flag can be wrong; semver decides
    let h = harness(&[beta, FixtureRelease::good("1.0.0")]).await;
    h.engine.check_for_updates(None).await.unwrap();
    assert_eq!(view(&h.engine, "testapp").latest.unwrap().version, "1.0.0");
    let mut s = h.engine.settings().unwrap();
    s.channel = crafthub_core::releases::Channel::Beta;
    h.engine.save_settings(&s).unwrap();
    assert_eq!(
        view(&h.engine, "testapp").latest.unwrap().version,
        "2.0.0-rc.1"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn invalid_ids_are_rejected() {
    let h = harness(&[]).await;
    for bad in ["../x", "TestApp", "", "photocraft\\..", "a;rm"] {
        assert!(h.engine.install(bad, None, null_sink()).await.is_err());
        assert!(h.engine.uninstall(bad).is_err());
        assert!(h.engine.launch(bad).is_err());
    }
    assert!(matches!(
        h.engine.install("noreleases", None, null_sink()).await,
        Err(CoreError::Unsupported(_))
    ));
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(walk(&p));
            }
            out.push(p);
        }
    }
    out
}

// ---------------------------------------------------------------- phase 2

/// A second engine on the same database and folders stands in for the CLI running next
/// to the GUI (separate process in real life; the file locks behave identically).
#[tokio::test(flavor = "multi_thread")]
async fn second_process_cannot_interfere_with_running_install() {
    let h = harness(&[FixtureRelease::good("1.0.0")]).await;
    let r = FixtureRelease::good("1.0.0");
    let dl = format!("/{REPO}/releases/download/v1.0.0/{}", r.asset_name());
    Mock::given(method("GET"))
        .and(path(dl))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(r.zip.clone())
                .set_delay(Duration::from_millis(1500)),
        )
        .with_priority(1)
        .mount(&h.server)
        .await;
    let (sink, events) = collecting_sink();
    let engine = h.engine.clone();
    let task = tokio::spawn(async move { engine.install("testapp", None, sink).await });
    // Wait until the first engine is downloading (temp file exists).
    loop {
        tokio::time::sleep(Duration::from_millis(20)).await;
        if events
            .lock()
            .unwrap()
            .iter()
            .any(|e| e.phase == Phase::Downloading)
        {
            break;
        }
    }
    // "CLI" starts: its crash recovery must not touch the live operation.
    let other = Engine::new(engine_config(&h.server, h.dir.path())).unwrap();
    assert!(
        other.app_view("testapp").unwrap().busy,
        "lock visible across engines"
    );
    let err = other
        .install("testapp", None, null_sink())
        .await
        .unwrap_err();
    assert!(matches!(err, CoreError::Busy(_)), "{err:?}");
    assert!(matches!(
        other.uninstall("testapp"),
        Err(CoreError::Busy(_)) | Err(CoreError::NotInstalled(_))
    ));

    let out = task
        .await
        .unwrap()
        .expect("first install completes despite the second engine");
    assert_eq!(out.version, "1.0.0");
    assert!(!other.app_view("testapp").unwrap().busy);
    assert_eq!(
        other.app_view("testapp").unwrap().status,
        AppStatus::Installed
    );
    h.assert_temp_clean();
}

#[tokio::test(flavor = "multi_thread")]
async fn custom_install_root_applies_to_new_installs_only() {
    let h = harness(&[FixtureRelease::good("1.0.0")]).await;
    h.engine
        .install("testapp", None, null_sink())
        .await
        .unwrap();
    let default_dir = h.apps_dir();
    assert_eq!(h.version_dirs().len(), 1);

    let custom = h.dir.path().join("MyApps");
    let root = h.engine.set_install_root(Some(&custom)).unwrap();
    assert!(root.ends_with("MyApps"));
    assert!(custom.join(".crafthub-library").is_file());

    // Updating an existing install keeps it in its original library.
    h.set_releases(&[FixtureRelease::good("1.1.0")]).await;
    let up = h.engine.update("testapp", null_sink()).await.unwrap();
    assert!(
        up.path.starts_with(default_dir.to_string_lossy().as_ref()),
        "{}",
        up.path
    );
    assert_eq!(h.version_dirs().len(), 2);

    // A fresh install (after uninstall) goes to the new root, and is fully manageable there.
    h.engine.uninstall("testapp").unwrap();
    let fresh = h
        .engine
        .install("testapp", None, null_sink())
        .await
        .unwrap();
    assert!(
        std::path::Path::new(&fresh.path).starts_with(&custom),
        "{}",
        fresh.path
    );
    assert!(
        std::path::Path::new(&fresh.path)
            .join("testapp.exe")
            .is_file()
    );
    assert_eq!(
        h.engine.app_view("testapp").unwrap().status,
        AppStatus::Installed
    );
    h.engine.uninstall("testapp").unwrap();
    assert!(!custom.join("Apps").join("testapp").exists());

    // Unsafe choices are refused; the setting is unchanged.
    let docs = h.dir.path().join("Docs");
    std::fs::create_dir_all(&docs).unwrap();
    std::fs::write(docs.join("keep.txt"), b"x").unwrap();
    assert!(h.engine.set_install_root(Some(&docs)).is_err());
    let inside_apps = h.dir.path().join("CraftHub").join("Apps").join("x");
    assert!(h.engine.set_install_root(Some(&inside_apps)).is_err());
    assert!(h.engine.install_root().unwrap().ends_with("MyApps"));

    // Resetting to the default.
    let back = h.engine.set_install_root(None).unwrap();
    assert!(back.ends_with("CraftHub"));
    // save_settings from the UI can never change the root.
    let mut s = h.engine.settings().unwrap();
    s.install_root = Some(r"C:\Windows".into());
    h.engine.save_settings(&s).unwrap();
    assert!(h.engine.settings().unwrap().install_root.is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn update_notices_are_not_repeated() {
    let h = harness(&[FixtureRelease::good("1.0.0")]).await;
    h.engine
        .install("testapp", None, null_sink())
        .await
        .unwrap();
    assert!(h.engine.take_new_update_notices().unwrap().is_empty());

    h.set_releases(&[FixtureRelease::good("1.1.0")]).await;
    h.engine.check_for_updates(None).await.unwrap();
    let n = h.engine.take_new_update_notices().unwrap();
    assert_eq!(n.len(), 1);
    assert_eq!(
        (n[0].app_id.as_str(), n[0].version.as_str()),
        ("testapp", "1.1.0")
    );
    assert!(
        h.engine.take_new_update_notices().unwrap().is_empty(),
        "same version: no repeat"
    );

    h.set_releases(&[FixtureRelease::good("1.2.0")]).await;
    h.engine.check_for_updates(None).await.unwrap();
    assert_eq!(
        h.engine.take_new_update_notices().unwrap()[0].version,
        "1.2.0"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn cancel_all_rolls_back_active_operations() {
    let h = harness(&[FixtureRelease::good("1.0.0")]).await;
    let r = FixtureRelease::good("1.0.0");
    let dl = format!("/{REPO}/releases/download/v1.0.0/{}", r.asset_name());
    Mock::given(method("GET"))
        .and(path(dl))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(r.zip.clone())
                .set_delay(Duration::from_secs(5)),
        )
        .with_priority(1)
        .mount(&h.server)
        .await;
    let engine = h.engine.clone();
    let task = tokio::spawn(async move { engine.install("testapp", None, null_sink()).await });
    while h.engine.active_operations().is_empty() {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(h.engine.active_operations()[0].app_id, "testapp");
    assert_eq!(h.engine.cancel_all(), 1);
    assert!(matches!(task.await.unwrap(), Err(CoreError::Cancelled)));
    assert!(h.engine.active_operations().is_empty());
    assert!(h.version_dirs().is_empty());
    h.assert_temp_clean();
}

/// Automatic-update rule with a *real* running process: an app whose executable image lives
/// in its install folder must be skipped (never closed), and must keep its current version.
#[tokio::test(flavor = "multi_thread")]
async fn update_all_skips_app_with_real_running_process() {
    let h = harness(&[FixtureRelease::good("1.0.0")]).await;
    let out = h
        .engine
        .install("testapp", None, null_sink())
        .await
        .unwrap();
    // Put a real, harmless Windows program inside the install folder and run it from there.
    let sys = std::env::var_os("SystemRoot").expect("SystemRoot");
    let ping = std::path::Path::new(&sys).join("System32").join("PING.EXE");
    let runner = std::path::Path::new(&out.path).join("helper-running.exe");
    std::fs::copy(&ping, &runner).unwrap();
    let mut child = std::process::Command::new(&runner)
        .args(["-n", "60", "127.0.0.1"])
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;

    h.set_releases(&[FixtureRelease::good("2.0.0")]).await;
    let s = h.engine.update_all(null_sink()).await.unwrap();
    assert!(s.updated.is_empty(), "{s:?}");
    assert_eq!(s.skipped.len(), 1);
    assert!(s.skipped[0].reason.as_deref().unwrap().contains("running"));
    assert!(h.engine.app_view("testapp").unwrap().running);
    assert_eq!(
        h.engine
            .app_view("testapp")
            .unwrap()
            .installed
            .unwrap()
            .version,
        "1.0.0"
    );

    // Once it exits, the next automatic cycle updates it and keeps 1.0.0 for rollback.
    child.kill().unwrap();
    child.wait().unwrap();
    let s = h.engine.update_all(null_sink()).await.unwrap();
    assert_eq!(s.updated.len(), 1);
    let v = h.engine.app_view("testapp").unwrap().installed.unwrap();
    assert_eq!(
        (v.version.as_str(), v.previous_version.as_deref()),
        ("2.0.0", Some("1.0.0"))
    );
    h.assert_temp_clean();
}
