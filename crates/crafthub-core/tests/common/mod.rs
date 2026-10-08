#![allow(dead_code)]
//! Deterministic fixtures: a local mock of the GitHub REST API + asset downloads, and
//! in-memory ZIP builders. No test touches the real network.

use std::io::Write;
use std::sync::Arc;

use crafthub_core::catalog::Catalog;
use crafthub_core::engine::{Engine, EngineConfig, ProgressEvent};
use crafthub_core::installer::extract::ExtractLimits;
use crafthub_core::net::HttpPolicy;
use crafthub_core::paths::ManagedPaths;
use crafthub_core::releases::GithubConfig;
use serde_json::json;
use sha2::{Digest, Sha256};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use zip::write::SimpleFileOptions;

pub const REPO: &str = "storytold/testapp";

pub fn catalog() -> Catalog {
    Catalog::parse(
        r#"{"schemaVersion":3,"source":"test","updated":"test","applications":[
        {"id":"testapp","name":"TestApp","description":"fixture","category":"craft-suite","repo":"storytold/testapp","tagPrefix":"v",
         "windowsX64":{"format":"portable-zip","variants":[{"assetStem":"testapp","executable":"testapp.exe"}],"stripFiles":["portable.txt"],"verified":true,
         "audit":{"date":"test","version":"v1.0.0","reference":"test"}},"notes":null},
        {"id":"noreleases","name":"NoReleases","description":"fixture","category":"craft-suite","repo":"storytold/noreleases","tagPrefix":"v","windowsX64":null,"notes":"none"}
        ]}"#,
    )
    .unwrap()
}

/// Minimal x86-64 PE header (not runnable; only needs to pass header validation).
pub fn fake_x64_exe(marker: &str) -> Vec<u8> {
    let mut v = vec![0u8; 0x200];
    v[0] = b'M';
    v[1] = b'Z';
    v[0x3c] = 0x40;
    v[0x40..0x44].copy_from_slice(b"PE\0\0");
    v[0x44..0x46].copy_from_slice(&0x8664u16.to_le_bytes());
    v.extend_from_slice(marker.as_bytes());
    v
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

pub fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, data) in entries {
        if name.ends_with('/') {
            w.add_directory(name.trim_end_matches('/'), opts).unwrap();
        } else {
            w.start_file(*name, opts).unwrap();
            w.write_all(data).unwrap();
        }
    }
    w.finish().unwrap().into_inner()
}

/// A well-formed portable archive for `version` matching the audited layout.
pub fn good_zip(version: &str) -> Vec<u8> {
    let root = format!("testapp-{version}-windows-x64-portable");
    let exe = fake_x64_exe(version);
    zip_of(&[
        (&format!("{root}/"), b""),
        (&format!("{root}/testapp.exe"), &exe),
        (&format!("{root}/testapp-cli.exe"), &exe),
        (&format!("{root}/README.md"), b"readme"),
        (&format!("{root}/portable.txt"), b"portable mode"),
        (&format!("{root}/data/fonts/a.txt"), b"font"),
    ])
}

pub struct FixtureRelease {
    pub version: String,
    pub prerelease: bool,
    pub zip: Vec<u8>,
    /// Digest advertised in the API (defaults to the real hash).
    pub digest: Option<String>,
    /// Content of SHA256SUMS.txt; `None` omits the file.
    pub sums: Option<String>,
}

impl FixtureRelease {
    pub fn good(version: &str) -> Self {
        let zip = good_zip(version);
        let name = format!("testapp-{version}-windows-x64-portable.zip");
        let h = sha256_hex(&zip);
        Self {
            version: version.into(),
            prerelease: false,
            sums: Some(format!("{h}  {name}\n")),
            digest: Some(format!("sha256:{h}")),
            zip,
        }
    }

    pub fn with_zip(version: &str, zip: Vec<u8>) -> Self {
        let h = sha256_hex(&zip);
        Self {
            version: version.into(),
            prerelease: false,
            digest: Some(format!("sha256:{h}")),
            sums: None,
            zip,
        }
    }

    pub fn asset_name(&self) -> String {
        format!("testapp-{}-windows-x64-portable.zip", self.version)
    }
}

pub struct Harness {
    pub server: MockServer,
    pub dir: tempfile::TempDir,
    pub engine: Arc<Engine>,
}

pub async fn mount_releases(server: &MockServer, releases: &[FixtureRelease]) {
    let base = server.uri();
    let mut api = Vec::new();
    for (i, r) in releases.iter().enumerate() {
        let tag = format!("v{}", r.version);
        let name = r.asset_name();
        let dl = format!("/{REPO}/releases/download/{tag}/{name}");
        Mock::given(method("GET"))
            .and(path(dl.clone()))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(r.zip.clone()))
            .mount(server)
            .await;
        let mut assets = vec![json!({
            "id": i * 10 + 1, "name": name, "size": r.zip.len(), "digest": r.digest,
            "browser_download_url": format!("{base}{dl}"), "state": "uploaded"
        })];
        if let Some(s) = &r.sums {
            let sp = format!("/{REPO}/releases/download/{tag}/SHA256SUMS.txt");
            Mock::given(method("GET"))
                .and(path(sp.clone()))
                .respond_with(ResponseTemplate::new(200).set_body_string(s.clone()))
                .mount(server)
                .await;
            assets.push(json!({"id": i * 10 + 2, "name": "SHA256SUMS.txt", "size": s.len(),
                "digest": null, "browser_download_url": format!("{base}{sp}"), "state": "uploaded"}));
        }
        api.push(json!({
            "id": i + 1, "tag_name": tag, "name": format!("TestApp {}", r.version), "body": "Release notes",
            "draft": false, "prerelease": r.prerelease, "published_at": "2026-10-01T00:00:00Z",
            "html_url": format!("https://github.com/{REPO}/releases/tag/{tag}"), "assets": assets
        }));
    }
    Mock::given(method("GET"))
        .and(path(format!("/repos/{REPO}/releases")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(api)
                .insert_header("etag", "\"v1\""),
        )
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/storytold/noreleases/releases"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(server)
        .await;
}

pub fn engine_config(server: &MockServer, dir: &std::path::Path) -> EngineConfig {
    EngineConfig {
        paths: ManagedPaths::new(dir.join("CraftHub")),
        db_path: Some({
            // Separate data folder, as in production (%LOCALAPPDATA%\<identifier>).
            let data = dir.join("Data");
            std::fs::create_dir_all(&data).unwrap();
            data.join("crafthub.db")
        }),
        github: GithubConfig {
            api_base: server.uri(),
            download_base: server.uri(),
            policy: HttpPolicy::loopback_for_tests(),
        },
        extract_limits: ExtractLimits::default(),
        catalog: catalog(),
    }
}

pub async fn harness(releases: &[FixtureRelease]) -> Harness {
    let server = MockServer::start().await;
    mount_releases(&server, releases).await;
    let dir = tempfile::tempdir().unwrap();
    let engine = Arc::new(Engine::new(engine_config(&server, dir.path())).unwrap());
    Harness {
        server,
        dir,
        engine,
    }
}

impl Harness {
    pub async fn set_releases(&self, releases: &[FixtureRelease]) {
        self.server.reset().await;
        mount_releases(&self.server, releases).await;
    }

    pub fn apps_dir(&self) -> std::path::PathBuf {
        self.dir
            .path()
            .join("CraftHub")
            .join("Apps")
            .join("testapp")
    }

    pub fn version_dirs(&self) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(self.apps_dir())
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        v.sort();
        v
    }

    /// Asserts nothing is left in Staging/Downloads.
    pub fn assert_temp_clean(&self) {
        for d in ["Staging", "Downloads"] {
            let p = self.dir.path().join("CraftHub").join(d);
            let n = std::fs::read_dir(&p).map(|r| r.count()).unwrap_or(0);
            assert_eq!(n, 0, "{d} is not empty");
        }
    }
}

pub fn collecting_sink() -> (
    crafthub_core::ProgressSink,
    Arc<std::sync::Mutex<Vec<ProgressEvent>>>,
) {
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let e2 = events.clone();
    (Arc::new(move |e| e2.lock().unwrap().push(e)), events)
}
