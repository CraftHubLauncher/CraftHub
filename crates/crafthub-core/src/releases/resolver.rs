//! Strict artifact resolver: an asset is selected only when its name is *exactly* the
//! audited pattern for this app and version, its download URL is the canonical GitHub
//! release-download URL for this repo and tag, and its metadata is sane.

use semver::Version;
use serde::Serialize;

use super::github::{GhAsset, GhRelease, parse_sha256_digest};
use super::version::{Channel, channel_allows, is_prerelease, parse_tag};
use crate::catalog::{CatalogApp, WindowsAdapter};

/// Hard upper bound for a single application archive.
pub const MAX_ASSET_BYTES: u64 = 2 * 1024 * 1024 * 1024;
pub const MAX_CHECKSUM_FILE_BYTES: u64 = 64 * 1024;
const CHECKSUM_FILE_NAMES: &[&str] = &["SHA256SUMS.txt", "SHA256SUMS"];

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedAsset {
    pub name: String,
    pub url: String,
    pub size: u64,
    /// Lowercase hex SHA-256 from GitHub's asset `digest` metadata.
    pub sha256: Option<String>,
    #[serde(skip)]
    pub checksums_url: Option<String>,
    pub has_checksum_file: bool,
    /// Executable and archive root of the audited variant this asset matched.
    #[serde(skip)]
    pub executable: String,
    #[serde(skip)]
    pub archive_root: String,
}

#[derive(Debug, Clone)]
pub struct ResolvedRelease {
    pub tag: String,
    pub version_text: String,
    pub version: Version,
    pub prerelease: bool,
    pub name: Option<String>,
    pub notes: Option<String>,
    pub published_at: Option<String>,
    pub html_url: String,
    pub asset: Result<ResolvedAsset, String>,
}

impl ResolvedRelease {
    pub fn installable(&self) -> bool {
        self.asset.as_ref().is_ok_and(|a| a.sha256.is_some())
    }
}

/// All usable releases for one app, newest first.
#[derive(Debug, Clone, Default)]
pub struct ReleaseSummary {
    pub releases: Vec<ResolvedRelease>,
    /// Tags that were skipped (unparseable versions), for diagnostics.
    pub ignored_tags: Vec<String>,
}

impl ReleaseSummary {
    pub fn build(
        app: &CatalogApp,
        adapter: Option<&WindowsAdapter>,
        releases: &[GhRelease],
        download_base: &str,
    ) -> Self {
        let mut out = ReleaseSummary::default();
        for rel in releases.iter().filter(|r| !r.draft) {
            let Some((version_text, version)) = parse_tag(&rel.tag_name, &app.tag_prefix) else {
                out.ignored_tags.push(rel.tag_name.clone());
                continue;
            };
            let asset = match adapter {
                Some(a) => resolve_asset(app, a, rel, &version_text, download_base),
                None => Err("No audited Windows x64 portable package for this app.".into()),
            };
            out.releases.push(ResolvedRelease {
                tag: rel.tag_name.clone(),
                prerelease: is_prerelease(rel.prerelease, &version),
                version_text,
                version,
                name: rel.name.clone(),
                notes: rel.body.clone(),
                published_at: rel.published_at.clone(),
                html_url: rel.html_url.clone(),
                asset,
            });
        }
        out.releases.sort_by(|a, b| b.version.cmp(&a.version));
        out.releases.dedup_by(|a, b| a.version == b.version);
        out
    }

    /// Newest release in `channel`, regardless of Windows support.
    pub fn newest(&self, channel: Channel) -> Option<&ResolvedRelease> {
        self.releases
            .iter()
            .find(|r| channel_allows(channel, r.prerelease))
    }

    /// Newest release in `channel` with an installable Windows x64 asset.
    pub fn latest_installable(&self, channel: Channel) -> Option<&ResolvedRelease> {
        self.releases
            .iter()
            .find(|r| channel_allows(channel, r.prerelease) && r.installable())
    }

    pub fn find_version(&self, version_text: &str) -> Option<&ResolvedRelease> {
        self.releases
            .iter()
            .find(|r| r.version_text == version_text)
    }
}

pub fn resolve_asset(
    app: &CatalogApp,
    adapter: &WindowsAdapter,
    rel: &GhRelease,
    version_text: &str,
    download_base: &str,
) -> Result<ResolvedAsset, String> {
    // Every audited variant is tried; exactly one may match.
    let mut matches: Vec<(&GhAsset, &crate::catalog::AssetVariant)> = Vec::new();
    for v in &adapter.variants {
        let name = v.expected_asset_name(version_text);
        matches.extend(rel.assets.iter().filter(|a| a.name == name).map(|a| (a, v)));
    }
    let (asset, variant) = match matches.as_slice() {
        [] => {
            let names: Vec<String> = adapter
                .variants
                .iter()
                .map(|v| v.expected_asset_name(version_text))
                .collect();
            return Err(format!(
                "Release {} has no audited Windows x64 package ({}).",
                rel.tag_name,
                names.join(" or ")
            ));
        }
        [one] => *one,
        _ => {
            return Err(format!(
                "Release {} has more than one candidate Windows x64 package; refusing to guess.",
                rel.tag_name
            ));
        }
    };
    let expected = asset.name.clone();
    if asset.state.as_deref().is_some_and(|s| s != "uploaded") {
        return Err(format!("{expected} is not fully uploaded yet."));
    }
    if asset.size == 0 || asset.size > MAX_ASSET_BYTES {
        return Err(format!(
            "{expected} has an implausible size ({} bytes).",
            asset.size
        ));
    }
    let base = download_base.trim_end_matches('/');
    let expected_url = format!(
        "{base}/{}/releases/download/{}/{}",
        app.repo, rel.tag_name, expected
    );
    if asset.browser_download_url != expected_url {
        return Err(format!("{expected} has an unexpected download location."));
    }
    let sha256 = asset.digest.as_deref().and_then(parse_sha256_digest);

    let checksums = rel.assets.iter().find(|a| {
        CHECKSUM_FILE_NAMES.contains(&a.name.as_str())
            && a.size > 0
            && a.size <= MAX_CHECKSUM_FILE_BYTES
            && a.browser_download_url
                == format!(
                    "{base}/{}/releases/download/{}/{}",
                    app.repo, rel.tag_name, a.name
                )
    });

    Ok(ResolvedAsset {
        name: asset.name.clone(),
        url: asset.browser_download_url.clone(),
        size: asset.size,
        sha256,
        has_checksum_file: checksums.is_some(),
        checksums_url: checksums.map(|a| a.browser_download_url.clone()),
        executable: variant.executable.clone(),
        archive_root: variant.expected_archive_root(version_text),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Catalog;

    const BASE: &str = "https://github.com";
    const H: &str = "9997f7df3a6f0717b46bb0f503b3d920c3beedeb9e9c4e1ff2ef3b8671acd47e";

    fn asset(repo: &str, tag: &str, name: &str) -> GhAsset {
        GhAsset {
            id: 1,
            name: name.into(),
            size: 1000,
            digest: Some(format!("sha256:{H}")),
            browser_download_url: format!("{BASE}/{repo}/releases/download/{tag}/{name}"),
            state: Some("uploaded".into()),
        }
    }

    fn release(tag: &str, prerelease: bool, assets: Vec<GhAsset>) -> GhRelease {
        GhRelease {
            id: 1,
            tag_name: tag.into(),
            name: Some(tag.into()),
            body: Some("notes".into()),
            draft: false,
            prerelease,
            published_at: None,
            html_url: format!("{BASE}/storytold/photocraft/releases/tag/{tag}"),
            assets,
        }
    }

    fn photocraft() -> CatalogApp {
        Catalog::load_embedded()
            .unwrap()
            .get("photocraft")
            .unwrap()
            .clone()
    }

    fn full_release(tag: &str, pre: bool) -> GhRelease {
        let v = tag.trim_start_matches('v');
        let repo = "storytold/photocraft";
        release(
            tag,
            pre,
            vec![
                asset(
                    repo,
                    tag,
                    &format!("photocraft-{v}-windows-arm64-portable.zip"),
                ),
                asset(repo, tag, &format!("photocraft-{v}-windows-x64.msi")),
                asset(
                    repo,
                    tag,
                    &format!("photocraft-{v}-windows-x64-portable.zip"),
                ),
                asset(repo, tag, "SHA256SUMS.txt"),
            ],
        )
    }

    #[test]
    fn selects_exact_x64_portable_zip() {
        let app = photocraft();
        let s = ReleaseSummary::build(
            &app,
            app.installable_adapter(),
            &[full_release("v0.3.0", false)],
            BASE,
        );
        let r = s.latest_installable(Channel::Stable).unwrap();
        let a = r.asset.as_ref().unwrap();
        assert_eq!(a.name, "photocraft-0.3.0-windows-x64-portable.zip");
        assert_eq!(a.sha256.as_deref(), Some(H));
        assert!(a.has_checksum_file);
    }

    #[test]
    fn no_substring_matching() {
        let app = photocraft();
        let repo = "storytold/photocraft";
        let rel = release(
            "v0.3.0",
            false,
            vec![
                asset(
                    repo,
                    "v0.3.0",
                    "photocraft-0.3.0-windows-x64-portable.zip.sig",
                ),
                asset(
                    repo,
                    "v0.3.0",
                    "evil-photocraft-0.3.0-windows-x64-portable.zip",
                ),
                asset(repo, "v0.3.0", "photocraft-0.3.1-windows-x64-portable.zip"),
            ],
        );
        let s = ReleaseSummary::build(&app, app.installable_adapter(), &[rel], BASE);
        assert!(s.latest_installable(Channel::Stable).is_none());
        assert!(s.newest(Channel::Stable).unwrap().asset.is_err());
    }

    #[test]
    fn rejects_foreign_download_url_and_missing_digest() {
        let app = photocraft();
        let mut a = asset(
            "storytold/photocraft",
            "v0.3.0",
            "photocraft-0.3.0-windows-x64-portable.zip",
        );
        a.browser_download_url = "https://github.com/attacker/photocraft/releases/download/v0.3.0/photocraft-0.3.0-windows-x64-portable.zip".into();
        let s = ReleaseSummary::build(
            &app,
            app.installable_adapter(),
            &[release("v0.3.0", false, vec![a.clone()])],
            BASE,
        );
        assert!(s.newest(Channel::Stable).unwrap().asset.is_err());

        let mut b = asset(
            "storytold/photocraft",
            "v0.3.0",
            "photocraft-0.3.0-windows-x64-portable.zip",
        );
        b.digest = None;
        let s = ReleaseSummary::build(
            &app,
            app.installable_adapter(),
            &[release("v0.3.0", false, vec![b])],
            BASE,
        );
        let r = s.newest(Channel::Stable).unwrap();
        assert!(r.asset.is_ok());
        assert!(!r.installable(), "no digest => not installable");
    }

    #[test]
    fn channel_and_ordering() {
        let app = photocraft();
        let rels = vec![
            full_release("v0.1.1-rc.5", false), // API flag wrong upstream
            full_release("v0.10.0", true),
            full_release("v0.9.0", false),
            GhRelease {
                draft: true,
                ..full_release("v9.9.9", false)
            },
            release("nightly", false, vec![]),
        ];
        let s = ReleaseSummary::build(&app, app.installable_adapter(), &rels, BASE);
        assert_eq!(s.latest_installable(Channel::Stable).unwrap().tag, "v0.9.0");
        assert_eq!(s.latest_installable(Channel::Beta).unwrap().tag, "v0.10.0");
        assert!(s.find_version("0.1.1-rc.5").unwrap().prerelease);
        assert!(s.find_version("9.9.9").is_none(), "drafts are ignored");
        assert_eq!(s.ignored_tags, vec!["nightly".to_string()]);
    }

    #[test]
    fn handles_upstream_asset_rename_via_variants() {
        let app = Catalog::load_embedded()
            .unwrap()
            .get("pdfcraft")
            .unwrap()
            .clone();
        let repo = "storytold/pdfcraft";
        let new = release(
            "v0.4.0",
            false,
            vec![asset(
                repo,
                "v0.4.0",
                "pdfcraft-0.4.0-windows-x64-portable.zip",
            )],
        );
        let old = release(
            "v0.2.1",
            false,
            vec![asset(
                repo,
                "v0.2.1",
                "printcraft-0.2.1-windows-x64-portable.zip",
            )],
        );
        let both = release(
            "v0.5.0",
            false,
            vec![
                asset(repo, "v0.5.0", "pdfcraft-0.5.0-windows-x64-portable.zip"),
                asset(repo, "v0.5.0", "printcraft-0.5.0-windows-x64-portable.zip"),
            ],
        );
        let s = ReleaseSummary::build(&app, app.installable_adapter(), &[both, new, old], BASE);
        let r = s.find_version("0.4.0").unwrap().asset.as_ref().unwrap();
        assert_eq!(
            (r.executable.as_str(), r.archive_root.as_str()),
            ("pdfcraft.exe", "pdfcraft-0.4.0-windows-x64-portable")
        );
        let r = s.find_version("0.2.1").unwrap().asset.as_ref().unwrap();
        assert_eq!(r.executable, "printcraft.exe");
        assert!(
            s.find_version("0.5.0").unwrap().asset.is_err(),
            "ambiguous release refused"
        );
        assert_eq!(s.latest_installable(Channel::Stable).unwrap().tag, "v0.4.0");
    }

    #[test]
    fn apps_without_adapter_are_never_installable() {
        let c = Catalog::load_embedded().unwrap();
        let art = c.get("artcraft").unwrap();
        let rel = release(
            "artcraft-v0.41.0",
            false,
            vec![asset(
                "storytold/artcraft",
                "artcraft-v0.41.0",
                "ArtCraft_0.41.0_x64-setup.exe",
            )],
        );
        let s = ReleaseSummary::build(art, art.installable_adapter(), &[rel], BASE);
        assert_eq!(
            s.newest(Channel::Stable).unwrap().version,
            Version::new(0, 41, 0)
        );
        assert!(s.latest_installable(Channel::Stable).is_none());
    }
}
