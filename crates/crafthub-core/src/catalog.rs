//! Validated application catalog. The catalog is a discovery list; installability is
//! decided by the audited `windowsX64` adapter *and* by what the live release contains.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use crate::error::{CoreError, Result};
use crate::validate::{is_safe_component, is_valid_app_id, is_valid_exe_name, is_valid_slug};

/// Only repositories under these owners may ever be contacted.
pub const ALLOWED_REPO_OWNERS: &[&str] = &["storytold"];
pub const SUPPORTED_SCHEMA_VERSION: u32 = 3;

/// The catalog compiled into the binary.
pub const EMBEDDED_CATALOG: &str = include_str!("../../../catalog/apps.json");

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Catalog {
    pub schema_version: u32,
    pub source: String,
    pub updated: String,
    pub applications: Vec<CatalogApp>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AppCategory {
    CraftSuite,
    AiStudio,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CatalogApp {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: AppCategory,
    /// `owner/name`; owner must be in [`ALLOWED_REPO_OWNERS`].
    pub repo: String,
    /// Literal prefix stripped from tag names before semver parsing (`v`, `artcraft-v`).
    pub tag_prefix: String,
    pub windows_x64: Option<WindowsAdapter>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PackageFormat {
    PortableZip,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WindowsAdapter {
    pub format: PackageFormat,
    /// Audited asset naming schemes. Upstream has renamed assets between releases
    /// (PDFCraft: `printcraft-*` up to 0.2.1, `pdfcraft-*` from 0.4.0), so each release must
    /// match exactly one variant; zero or several matches make it not installable.
    pub variants: Vec<AssetVariant>,
    /// Top-level files removed from the staged copy before activation (see RELEASE_AUDIT.md).
    pub strip_files: Vec<String>,
    /// Must be `true` (audited) for installation to be offered.
    pub verified: bool,
    pub audit: AuditInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuditInfo {
    pub date: String,
    pub version: String,
    pub reference: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssetVariant {
    /// Asset filename stem; the exact asset name is `{stem}-{version}-windows-x64-portable.zip`.
    pub asset_stem: String,
    /// GUI executable, relative to the archive's root folder.
    pub executable: String,
}

impl AssetVariant {
    pub fn expected_asset_name(&self, version_text: &str) -> String {
        format!(
            "{}-{}-windows-x64-portable.zip",
            self.asset_stem, version_text
        )
    }

    pub fn expected_archive_root(&self, version_text: &str) -> String {
        format!("{}-{}-windows-x64-portable", self.asset_stem, version_text)
    }
}

impl CatalogApp {
    pub fn repo_url(&self) -> String {
        format!("https://github.com/{}", self.repo)
    }

    pub fn releases_url(&self) -> String {
        format!("https://github.com/{}/releases", self.repo)
    }

    /// The installable Windows adapter, if audited.
    pub fn installable_adapter(&self) -> Option<&WindowsAdapter> {
        self.windows_x64.as_ref().filter(|a| a.verified)
    }

    /// Human-readable reason this app cannot be installed on this platform, if any.
    pub fn unsupported_reason(&self) -> Option<String> {
        if !cfg!(all(windows, target_arch = "x86_64")) {
            return Some("CraftHub currently installs apps on Windows x64 only.".into());
        }
        match &self.windows_x64 {
            None => {
                Some(self.notes.clone().unwrap_or_else(|| {
                    "No audited Windows x64 portable release is available.".into()
                }))
            }
            Some(a) if !a.verified => Some(
                "The Windows release layout has not been audited yet, so installing is disabled."
                    .into(),
            ),
            Some(_) => None,
        }
    }
}

impl Catalog {
    pub fn load_embedded() -> Result<Catalog> {
        Self::parse(EMBEDDED_CATALOG)
    }

    pub fn parse(json: &str) -> Result<Catalog> {
        let catalog: Catalog =
            serde_json::from_str(json).map_err(|e| CoreError::Catalog(e.to_string()))?;
        catalog.validate()?;
        Ok(catalog)
    }

    pub fn get(&self, id: &str) -> Result<&CatalogApp> {
        self.applications
            .iter()
            .find(|a| a.id == id)
            .ok_or_else(|| CoreError::UnknownApp(id.to_string()))
    }

    fn validate(&self) -> Result<()> {
        let bad = |m: String| Err(CoreError::Catalog(m));
        if self.schema_version != SUPPORTED_SCHEMA_VERSION {
            return bad(format!("unsupported schemaVersion {}", self.schema_version));
        }
        let mut ids = HashSet::new();
        for app in &self.applications {
            if !is_valid_app_id(&app.id) {
                return bad(format!("invalid app id '{}'", app.id));
            }
            if !ids.insert(app.id.as_str()) {
                return bad(format!("duplicate app id '{}'", app.id));
            }
            if app.name.trim().is_empty() || app.name.len() > 64 {
                return bad(format!("{}: invalid name", app.id));
            }
            let Some((owner, name)) = app.repo.split_once('/') else {
                return bad(format!("{}: repo must be owner/name", app.id));
            };
            if !ALLOWED_REPO_OWNERS.contains(&owner) || !is_valid_slug(name) {
                return bad(format!(
                    "{}: repository '{}' is not allowlisted",
                    app.id, app.repo
                ));
            }
            if app.tag_prefix.len() > 32
                || !app
                    .tag_prefix
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b == b'-')
            {
                return bad(format!("{}: invalid tagPrefix", app.id));
            }
            if let Some(a) = &app.windows_x64 {
                if a.variants.is_empty() || a.variants.len() > 4 {
                    return bad(format!("{}: windowsX64 needs 1-4 variants", app.id));
                }
                let mut stems = HashSet::new();
                for v in &a.variants {
                    if !is_valid_slug(&v.asset_stem) || !stems.insert(v.asset_stem.as_str()) {
                        return bad(format!("{}: invalid or duplicate assetStem", app.id));
                    }
                    if !is_valid_exe_name(&v.executable) {
                        return bad(format!("{}: invalid executable name", app.id));
                    }
                }
                if a.strip_files.iter().any(|f| {
                    !is_safe_component(f)
                        || a.variants
                            .iter()
                            .any(|v| f.eq_ignore_ascii_case(&v.executable))
                }) {
                    return bad(format!("{}: invalid stripFiles entry", app.id));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_catalog_is_valid_and_complete() {
        let c = Catalog::load_embedded().unwrap();
        let suite: Vec<_> = c
            .applications
            .iter()
            .filter(|a| a.category == AppCategory::CraftSuite)
            .map(|a| a.id.as_str())
            .collect();
        assert_eq!(
            suite,
            [
                "photocraft",
                "vectorcraft",
                "filmcraft",
                "lightcraft",
                "pdfcraft",
                "effectcraft",
                "designcraft",
                "soundcraft",
                "wordcraft",
                "gridcraft",
                "deckcraft",
                "cadcraft"
            ]
        );
        let art = c.get("artcraft").unwrap();
        assert_eq!(art.category, AppCategory::AiStudio);
        assert!(art.windows_x64.is_none(), "ArtCraft has no portable ZIP");
        let sound = c.get("soundcraft").unwrap();
        assert!(
            sound.installable_adapter().is_some(),
            "SoundCraft audited 2026-10-08 (v0.3.0)"
        );
        let pdf = c.get("pdfcraft").unwrap().windows_x64.as_ref().unwrap();
        let stems: Vec<_> = pdf.variants.iter().map(|v| v.asset_stem.as_str()).collect();
        assert_eq!(stems, ["pdfcraft", "printcraft"]);
        assert_eq!(
            pdf.variants[1].expected_asset_name("0.2.1"),
            "printcraft-0.2.1-windows-x64-portable.zip"
        );
        let photo = c.get("photocraft").unwrap().windows_x64.as_ref().unwrap();
        assert_eq!(photo.strip_files, ["portable.txt"]);
    }

    fn with_app(app_json: &str) -> String {
        format!(r#"{{"schemaVersion":3,"source":"t","updated":"t","applications":[{app_json}]}}"#)
    }

    #[test]
    fn rejects_non_allowlisted_repo() {
        let j = with_app(
            r#"{"id":"evil","name":"Evil","description":"","category":"craft-suite","repo":"attacker/evil","tagPrefix":"v","windowsX64":null,"notes":null}"#,
        );
        assert!(Catalog::parse(&j).is_err());
    }

    #[test]
    fn rejects_traversal_in_executable() {
        let j = with_app(
            r#"{"id":"evil","name":"Evil","description":"","category":"craft-suite","repo":"storytold/evil","tagPrefix":"v","windowsX64":{"format":"portable-zip","variants":[{"assetStem":"evil","executable":"..\\x.exe"}],"stripFiles":[],"verified":true,"audit":{"date":"","version":"","reference":""}},"notes":null}"#,
        );
        assert!(Catalog::parse(&j).is_err());
    }

    #[test]
    fn accepts_valid_variants_and_rejects_empty_or_duplicate() {
        let ok = with_app(
            r#"{"id":"ok","name":"Ok","description":"","category":"craft-suite","repo":"storytold/ok","tagPrefix":"v","windowsX64":{"format":"portable-zip","variants":[{"assetStem":"ok","executable":"ok.exe"},{"assetStem":"old","executable":"old.exe"}],"stripFiles":[],"verified":true,"audit":{"date":"","version":"","reference":""}},"notes":null}"#,
        );
        assert!(Catalog::parse(&ok).is_ok());
        let empty = ok.replace(
            r#"[{"assetStem":"ok","executable":"ok.exe"},{"assetStem":"old","executable":"old.exe"}]"#,
            "[]",
        );
        assert!(Catalog::parse(&empty).is_err());
        let dup = ok.replace(r#""assetStem":"old""#, r#""assetStem":"ok""#);
        assert!(Catalog::parse(&dup).is_err());
        let strip_exe = ok.replace(r#""stripFiles":[]"#, r#""stripFiles":["old.exe"]"#);
        assert!(Catalog::parse(&strip_exe).is_err());
    }

    #[test]
    fn rejects_unknown_fields_and_bad_ids() {
        let j = with_app(
            r#"{"id":"Bad Id","name":"x","description":"","category":"craft-suite","repo":"storytold/x","tagPrefix":"v","windowsX64":null,"notes":null}"#,
        );
        assert!(Catalog::parse(&j).is_err());
        let j = with_app(
            r#"{"id":"ok","name":"x","description":"","category":"craft-suite","repo":"storytold/x","tagPrefix":"v","windowsX64":null,"notes":null,"installer":"run.ps1"}"#,
        );
        assert!(Catalog::parse(&j).is_err());
    }
}
