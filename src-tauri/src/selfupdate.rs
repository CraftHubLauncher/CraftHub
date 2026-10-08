//! CraftHub self-update via the Tauri updater plugin.
//!
//! Authenticity comes from a minisign signature over the update package, verified with a
//! public key compiled into the binary — not from GitHub's SHA-256 digest. Self-update is
//! **off** unless one gate ([`gate`]) passes for this build:
//!
//! 1. [`PRODUCTION_REPOSITORY`] names the real repository (`owner/repo`, set in source at
//!    publish time and reviewed like any other code change);
//! 2. the bundle identifier is final and names the same owner (`io.github.<owner>.crafthub`);
//! 3. `CRAFTHUB_UPDATER_PUBKEY` (compile time) is a minisign *public* key;
//! 4. `CRAFTHUB_UPDATE_ENDPOINT` (compile time) is exactly
//!    `https://github.com/<owner>/<repo>/releases/latest/download/latest.json` for that
//!    repository — same owner **and** same repository name.
//!
//! Development, local and unsigned builds report why and never download anything; the updater
//! plugin is not even registered for them.

use base64::Engine as _;
use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_updater::UpdaterExt;

const PUBKEY: Option<&str> = option_env!("CRAFTHUB_UPDATER_PUBKEY");
const ENDPOINT: Option<&str> = option_env!("CRAFTHUB_UPDATE_ENDPOINT");

/// Identifiers used before the public repository identity was finalized. Keep these blocked
/// even if a stale build is supplied with otherwise plausible update settings.
const PROVISIONAL_IDENTIFIERS: &[&str] = &["io.github.crafthub-community.crafthub"];

/// The one repository CraftHub may update itself from (`"owner/repo"`). `None` until the
/// signed production release is configured — which keeps self-update off in every build,
/// whatever keys or endpoints are supplied at compile time. The release workflow checks this
/// against the repository it runs in.
pub const PRODUCTION_REPOSITORY: Option<&str> = None;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfUpdateStatus {
    pub configured: bool,
    pub current_version: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfUpdateInfo {
    pub version: String,
    pub notes: Option<String>,
    pub date: Option<String>,
}

/// Decides whether self-update may be enabled. Pure function so every rule is unit-tested.
pub fn gate(
    production_repository: Option<&str>,
    identifier: &str,
    pubkey: Option<&str>,
    endpoint: Option<&str>,
) -> Result<(String, url::Url), String> {
    let Some((repo_owner, repo_name)) = production_repository.and_then(|r| r.split_once('/'))
    else {
        return Err(
            "Self-update is turned off until CraftHub's public repository is configured. \
             Download new versions from the CraftHub releases page."
                .into(),
        );
    };
    let valid_part = |p: &str| {
        !p.is_empty()
            && p.len() <= 100
            && p.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    };
    if !valid_part(repo_owner) || !valid_part(repo_name) {
        return Err("The configured update repository is not a valid owner/repo name.".into());
    }
    if PROVISIONAL_IDENTIFIERS.contains(&identifier) {
        return Err(
            "Self-update is turned off until CraftHub's public repository identity is final. \
             Download new versions from the CraftHub releases page."
                .into(),
        );
    }
    let owner = identifier
        .strip_prefix("io.github.")
        .and_then(|rest| rest.strip_suffix(".crafthub"))
        .filter(|o| !o.is_empty() && !o.contains('.'))
        .ok_or("The app identifier does not name a GitHub owner (io.github.<owner>.crafthub).")?;

    let key = pubkey.map(str::trim).filter(|k| !k.is_empty()).ok_or(
        "This build has no update-signing key, so CraftHub cannot verify updates to itself. \
         Download new versions from the CraftHub releases page.",
    )?;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(key)
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
        .ok_or("The embedded update key is not a valid minisign public key.")?;
    let first = decoded
        .lines()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !first.starts_with("untrusted comment:")
        || !first.contains("public key")
        || first.contains("secret")
    {
        return Err("The embedded update key is not a minisign public key.".into());
    }

    let raw = endpoint
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .ok_or("This build has no update address configured.")?;
    let u = url::Url::parse(raw).map_err(|_| "The update address is not a valid URL.")?;
    let segs: Vec<&str> = u.path_segments().map(|s| s.collect()).unwrap_or_default();
    let shape_ok = u.scheme() == "https"
        && u.host_str() == Some("github.com")
        && u.port().is_none()
        && u.query().is_none()
        && u.username().is_empty()
        && segs.len() == 6
        && segs[2] == "releases"
        && segs[3] == "latest"
        && segs[4] == "download"
        && segs[5] == "latest.json";
    if !shape_ok {
        return Err("The update address is not a GitHub latest.json release URL.".into());
    }
    if !owner.eq_ignore_ascii_case(repo_owner) {
        return Err(
            "The app identifier names a different owner than the update repository.".into(),
        );
    }
    // Exact repository match: same owner and same repository name, not just the owner.
    if segs[0] != repo_owner || segs[1] != repo_name {
        return Err("The update address does not point to CraftHub's own repository.".into());
    }
    Ok((key.to_string(), u))
}

fn this_build(identifier: &str) -> Result<(String, url::Url), String> {
    gate(PRODUCTION_REPOSITORY, identifier, PUBKEY, ENDPOINT)
}

/// Public key to register the updater plugin with — only when the whole gate passes.
pub fn pubkey_for(identifier: &str) -> Option<String> {
    this_build(identifier).ok().map(|(k, _)| k)
}

pub fn status(app: &AppHandle) -> SelfUpdateStatus {
    let reason = this_build(&app.config().identifier).err();
    SelfUpdateStatus {
        configured: reason.is_none(),
        current_version: app.package_info().version.to_string(),
        reason,
    }
}

fn updater(app: &AppHandle) -> Result<tauri_plugin_updater::Updater, String> {
    let (_, url) = this_build(&app.config().identifier)?;
    app.updater_builder()
        .endpoints(vec![url])
        .map_err(|e| e.to_string())?
        .build()
        .map_err(|e| e.to_string())
}

pub async fn check(app: &AppHandle) -> Result<Option<SelfUpdateInfo>, String> {
    let update = updater(app)?.check().await.map_err(|e| e.to_string())?;
    Ok(update.map(|u| SelfUpdateInfo {
        version: u.version.clone(),
        notes: u.body.clone(),
        date: u.date.map(|d| d.to_string()),
    }))
}

/// Downloads, verifies the signature, and runs CraftHub's own signed installer, then
/// restarts. Only called after the user approves in the UI.
pub async fn install(app: &AppHandle) -> Result<(), String> {
    let update = updater(app)?
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or("CraftHub is already up to date.")?;
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| e.to_string())?;
    app.restart();
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPO: Option<&str> = Some("example-owner/crafthub");
    const ID: &str = "io.github.example-owner.crafthub";
    const EP: &str =
        "https://github.com/example-owner/crafthub/releases/latest/download/latest.json";

    fn b64(s: &str) -> String {
        base64::engine::general_purpose::STANDARD.encode(s)
    }

    fn pubkey() -> String {
        b64(
            "untrusted comment: minisign public key: 0123456789ABCDEF\nRWQBI0VniavN7xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\n",
        )
    }

    #[test]
    fn shipped_source_and_config_keep_self_update_off() {
        assert_eq!(
            PRODUCTION_REPOSITORY, None,
            "set only when the public repo exists"
        );
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let id = conf["identifier"].as_str().unwrap();
        // Even with a valid key and endpoint, this build cannot enable self-update.
        assert!(gate(PRODUCTION_REPOSITORY, id, Some(&pubkey()), Some(EP)).is_err());
        assert!(
            gate(REPO, id, Some(&pubkey()), Some(EP)).is_err(),
            "provisional identifier"
        );
        assert!(pubkey_for(id).is_none());
        assert!(conf["plugins"]["updater"].is_null());
        assert_ne!(
            conf["bundle"]["createUpdaterArtifacts"],
            serde_json::json!(true)
        );
    }

    #[test]
    fn requires_exact_repository_key_and_matching_identity() {
        assert!(gate(REPO, ID, Some(&pubkey()), Some(EP)).is_ok());
        assert!(gate(None, ID, Some(&pubkey()), Some(EP)).is_err());
        assert!(gate(Some("no-slash"), ID, Some(&pubkey()), Some(EP)).is_err());
        assert!(
            gate(
                Some("example-owner/cra fthub"),
                ID,
                Some(&pubkey()),
                Some(EP)
            )
            .is_err()
        );
        assert!(gate(REPO, ID, None, Some(EP)).is_err());
        assert!(gate(REPO, ID, Some(""), Some(EP)).is_err());
        assert!(gate(REPO, ID, Some(&pubkey()), None).is_err());
        // Same owner, different repository: rejected (exact repository, not just owner).
        let sibling =
            "https://github.com/example-owner/other-repo/releases/latest/download/latest.json";
        assert!(gate(REPO, ID, Some(&pubkey()), Some(sibling)).is_err());
        // Different owner.
        let other = "https://github.com/someone-else/crafthub/releases/latest/download/latest.json";
        assert!(gate(REPO, ID, Some(&pubkey()), Some(other)).is_err());
        // Identifier owner must match the configured repository owner.
        assert!(
            gate(
                REPO,
                "io.github.someone-else.crafthub",
                Some(&pubkey()),
                Some(EP)
            )
            .is_err()
        );
        assert!(gate(REPO, "com.example.crafthub", Some(&pubkey()), Some(EP)).is_err());
    }

    #[test]
    fn rejects_bad_endpoints() {
        for bad in [
            "http://github.com/example-owner/crafthub/releases/latest/download/latest.json",
            "https://evil.example/example-owner/crafthub/releases/latest/download/latest.json",
            "https://github.com/example-owner/crafthub/releases/download/v1/latest.json",
            "https://github.com/example-owner/crafthub/releases/latest/download/latest.json?x=1",
            "https://github.com:8443/example-owner/crafthub/releases/latest/download/latest.json",
            "https://github.com/example-owner/crafthub/releases/latest/download/other.json",
            "not a url",
        ] {
            assert!(gate(REPO, ID, Some(&pubkey()), Some(bad)).is_err(), "{bad}");
        }
    }

    #[test]
    fn rejects_secret_or_garbage_keys() {
        // Built at runtime so the repository privacy scan never sees a literal key header.
        let secret = b64(&format!(
            "untrusted comment: rsign encrypted {}\nRWRTY0Iy\n",
            "secret key"
        ));
        assert!(
            gate(REPO, ID, Some(&secret), Some(EP)).is_err(),
            "never accept a private key"
        );
        assert!(gate(REPO, ID, Some("not-base64!!"), Some(EP)).is_err());
        assert!(gate(REPO, ID, Some(&b64("hello world")), Some(EP)).is_err());
    }
}
