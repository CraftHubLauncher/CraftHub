//! The CraftHub engine: coordinates catalog, release discovery, transactional
//! install/update/rollback/uninstall, launching and crash recovery.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use futures_util::future::join_all;
use serde::Serialize;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use crate::catalog::{AppCategory, AuditInfo, Catalog, CatalogApp, WindowsAdapter};
use crate::error::{CoreError, Result};
use crate::installer::download::download_to_file;
use crate::installer::extract::{self, ExtractLimits};
use crate::locks::{AppLock, LockDir};
use crate::paths::{
    ManagedPaths, RemovalReport, remove_app_dir_if_empty, remove_version_dir, validate_library_root,
};
use crate::platform::{self, RunningProcess};
use crate::registry::{EventRow, InstallRecord, Registry, Settings};
use crate::releases::github::find_in_checksums;
use crate::releases::resolver::MAX_CHECKSUM_FILE_BYTES;
use crate::releases::{
    FetchSource, GithubClient, GithubConfig, ReleaseSummary, ResolvedAsset, ResolvedRelease,
};
use crate::util::{constant_time_eq, now_unix};
use crate::validate::{is_valid_app_id, is_valid_version_label};

/// Extra free space kept beyond what an install needs.
const FREE_SPACE_MARGIN: u64 = 64 * 1024 * 1024;
const MAX_PARALLEL_DOWNLOADS: usize = 2;

pub struct EngineConfig {
    /// Default install root ("library"). Users may pick another root for new installs.
    pub paths: ManagedPaths,
    /// SQLite file; `None` keeps state in memory (tests).
    pub db_path: Option<PathBuf>,
    pub github: GithubConfig,
    pub extract_limits: ExtractLimits,
    pub catalog: Catalog,
}

// ---------------------------------------------------------------- progress

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum OpKind {
    Install,
    Update,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Resolving,
    Downloading,
    Verifying,
    Extracting,
    Activating,
    CleaningUp,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressEvent {
    pub op_id: String,
    pub app_id: String,
    pub kind: OpKind,
    pub phase: Phase,
    pub done: u64,
    pub total: u64,
    pub message: Option<String>,
}

pub type ProgressSink = Arc<dyn Fn(ProgressEvent) + Send + Sync>;

pub fn null_sink() -> ProgressSink {
    Arc::new(|_| {})
}

/// Deterministic fault injection used by integration tests.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailPoint {
    /// Return an error after the new version is moved into place but before the registry
    /// commit; normal error handling runs.
    ErrorBeforeCommit,
    /// Stop right after moving the new version into place *without* any cleanup, as if
    /// the process had been killed. Recovery happens on the next `Engine::new`.
    CrashBeforeCommit,
}

// ---------------------------------------------------------------- views

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AppStatus {
    Unavailable,
    NotInstalled,
    Installed,
    UpdateAvailable,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledView {
    pub version: String,
    pub tag: String,
    pub path: String,
    pub executable: String,
    pub installed_at: i64,
    pub sha256: String,
    pub verification: String,
    pub previous_version: Option<String>,
    pub shortcut_path: Option<String>,
    pub shortcut_exists: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseView {
    pub tag: String,
    pub version: String,
    pub name: Option<String>,
    pub published_at: Option<String>,
    pub notes: Option<String>,
    pub html_url: String,
    pub prerelease: bool,
    pub installable: bool,
    pub asset: Option<ResolvedAsset>,
    pub unavailable_reason: Option<String>,
}

impl From<&ResolvedRelease> for ReleaseView {
    fn from(r: &ResolvedRelease) -> Self {
        let unavailable_reason = match &r.asset {
            Err(e) => Some(e.clone()),
            Ok(a) if a.sha256.is_none() => Some(
                "GitHub did not publish a SHA-256 digest for this file, so CraftHub will not install it."
                    .into(),
            ),
            Ok(_) => None,
        };
        ReleaseView {
            tag: r.tag.clone(),
            version: r.version_text.clone(),
            name: r.name.clone(),
            published_at: r.published_at.clone(),
            notes: r.notes.clone(),
            html_url: r.html_url.clone(),
            prerelease: r.prerelease,
            installable: r.installable(),
            asset: r.asset.as_ref().ok().cloned(),
            unavailable_reason,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseCheck {
    pub checked_at: Option<i64>,
    pub source: Option<FetchSource>,
    pub warning: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppView {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: AppCategory,
    pub repo: String,
    pub repo_url: String,
    pub releases_url: String,
    pub supported: bool,
    pub unsupported_reason: Option<String>,
    pub audit: Option<AuditInfo>,
    pub status: AppStatus,
    pub installed: Option<InstalledView>,
    /// Newest installable release in the selected channel.
    pub latest: Option<ReleaseView>,
    /// Newest release in the channel when it is *not* installable (explains why).
    pub newest_unavailable: Option<ReleaseView>,
    pub update_available: bool,
    pub check: ReleaseCheck,
    pub running: bool,
    pub busy: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallOutcome {
    pub app_id: String,
    pub op_id: String,
    pub version: String,
    pub previous_version: Option<String>,
    pub verification: String,
    pub path: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct InstallOptions {
    /// Validated library root for a new app. Updates ignore this and use the recorded root.
    pub library: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutView {
    pub path: Option<String>,
    pub exists: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UninstallOutcome {
    pub app_id: String,
    pub removed_files: usize,
    pub kept_entries: Vec<String>,
    pub kept_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAllItem {
    pub app_id: String,
    pub name: String,
    pub from: Option<String>,
    pub to: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAllSummary {
    pub updated: Vec<UpdateAllItem>,
    pub skipped: Vec<UpdateAllItem>,
    pub failed: Vec<UpdateAllItem>,
}

/// An update the user has not been told about yet (for native notifications).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateNotice {
    pub app_id: String,
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ActiveOperation {
    pub op_id: String,
    pub app_id: String,
}

// ---------------------------------------------------------------- engine

#[derive(Clone)]
struct CheckState {
    summary: ReleaseSummary,
    checked_at: i64,
    source: FetchSource,
    warning: Option<String>,
}

pub struct Engine {
    catalog: Catalog,
    registry: Registry,
    /// Default library (`%LOCALAPPDATA%\Programs\CraftHub` in production).
    paths: ManagedPaths,
    data_dir: Option<PathBuf>,
    github: GithubClient,
    limits: ExtractLimits,
    busy: Mutex<HashSet<String>>,
    locks: LockDir,
    ops: Mutex<HashMap<String, (String, CancellationToken)>>,
    downloads: Semaphore,
    checks: Mutex<HashMap<String, CheckState>>,
    check_errors: Mutex<HashMap<String, String>>,
    fail_point: Mutex<Option<FailPoint>>,
}

/// In-process busy flag plus the cross-process file lock for one app.
struct BusyGuard<'a> {
    set: &'a Mutex<HashSet<String>>,
    id: String,
    _lock: AppLock,
}

impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut s) = self.set.lock() {
            s.remove(&self.id);
        }
    }
}

struct OpGuard<'a> {
    ops: &'a Mutex<HashMap<String, (String, CancellationToken)>>,
    id: String,
}

impl Drop for OpGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut m) = self.ops.lock() {
            m.remove(&self.id);
        }
    }
}

/// Mutable bookkeeping for one install transaction, used for cleanup on failure.
struct Txn {
    op_id: String,
    app_id: String,
    /// Library the new version goes into (existing installs keep theirs).
    paths: ManagedPaths,
    library: Option<String>,
    staging: PathBuf,
    download: PathBuf,
    dir_name: String,
    prepared: bool,
    moved: bool,
}

impl Engine {
    pub fn new(config: EngineConfig) -> Result<Engine> {
        config.paths.ensure()?;
        let registry = match &config.db_path {
            Some(p) => Registry::open(p)?,
            None => Registry::open_in_memory()?,
        };
        let data_dir = config
            .db_path
            .as_ref()
            .and_then(|p| p.parent())
            .map(Path::to_path_buf);
        // Locks live next to the database so every process sharing it shares the locks.
        let locks = LockDir::new(match &data_dir {
            Some(d) => d.join("locks"),
            None => config.paths.root.join("Locks"),
        })?;
        let engine = Engine {
            catalog: config.catalog,
            registry,
            paths: config.paths,
            data_dir,
            github: GithubClient::new(config.github)?,
            limits: config.extract_limits,
            busy: Mutex::new(HashSet::new()),
            locks,
            ops: Mutex::new(HashMap::new()),
            downloads: Semaphore::new(MAX_PARALLEL_DOWNLOADS),
            checks: Mutex::new(HashMap::new()),
            check_errors: Mutex::new(HashMap::new()),
            fail_point: Mutex::new(None),
        };
        engine.recover()?;
        Ok(engine)
    }

    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    pub fn paths(&self) -> &ManagedPaths {
        &self.paths
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    #[doc(hidden)]
    pub fn set_fail_point(&self, fp: Option<FailPoint>) {
        *self.fail_point.lock().unwrap() = fp;
    }

    fn fail_point(&self) -> Option<FailPoint> {
        *self.fail_point.lock().unwrap()
    }

    pub fn settings(&self) -> Result<Settings> {
        self.registry.settings()
    }

    /// Saves user settings. `install_root` is ignored here; it can only be changed through
    /// the validated [`Engine::set_install_root`].
    pub fn save_settings(&self, s: &Settings) -> Result<()> {
        if s.check_interval_hours > 24 * 7 {
            return Err(CoreError::InvalidInput(
                "check interval must be at most one week".into(),
            ));
        }
        let mut s = s.clone();
        s.install_root = self.registry.settings()?.install_root;
        self.registry.save_settings(&s)?;
        // Channel changes invalidate the derived summaries but not the raw cache.
        self.checks.lock().unwrap().clear();
        Ok(())
    }

    pub fn events(&self, limit: u32) -> Result<Vec<EventRow>> {
        self.registry.recent_events(limit.min(500))
    }

    pub fn clear_release_cache(&self) -> Result<()> {
        self.registry.http_cache_clear()?;
        self.checks.lock().unwrap().clear();
        self.check_errors.lock().unwrap().clear();
        Ok(())
    }

    fn app(&self, app_id: &str) -> Result<&CatalogApp> {
        if !is_valid_app_id(app_id) {
            return Err(CoreError::InvalidInput(format!("bad app id {app_id:?}")));
        }
        self.catalog.get(app_id)
    }

    /// Takes the in-process busy flag and the cross-process lock for `app`, so neither
    /// this process nor another CraftHub (GUI or CLI) can modify it concurrently.
    fn acquire(&self, app: &CatalogApp) -> Result<BusyGuard<'_>> {
        // The set mutex is held while probing the file lock so `is_busy` never races us.
        let mut set = self.busy.lock().unwrap();
        if set.contains(&app.id) {
            return Err(CoreError::Busy(app.name.clone()));
        }
        let Some(lock) = self.locks.try_lock(&app.id)? else {
            return Err(CoreError::Busy(format!(
                "{} (in another CraftHub window or the command-line tool)",
                app.name
            )));
        };
        set.insert(app.id.clone());
        Ok(BusyGuard {
            set: &self.busy,
            id: app.id.clone(),
            _lock: lock,
        })
    }

    fn is_busy(&self, app_id: &str) -> bool {
        let set = self.busy.lock().unwrap();
        set.contains(app_id) || self.locks.is_locked(app_id)
    }

    /// Requests cancellation of a running install/update. Returns false if unknown.
    pub fn cancel(&self, op_id: &str) -> bool {
        match self.ops.lock().unwrap().get(op_id) {
            Some((_, t)) => {
                t.cancel();
                true
            }
            None => false,
        }
    }

    /// Install/update operations currently running in this process.
    pub fn active_operations(&self) -> Vec<ActiveOperation> {
        self.ops
            .lock()
            .unwrap()
            .iter()
            .map(|(op, (app, _))| ActiveOperation {
                op_id: op.clone(),
                app_id: app.clone(),
            })
            .collect()
    }

    /// Cancels every running operation (used before exiting). Each one rolls back.
    pub fn cancel_all(&self) -> usize {
        let ops = self.ops.lock().unwrap();
        ops.values().for_each(|(_, t)| t.cancel());
        ops.len()
    }

    // ------------------------------------------------------------ libraries

    /// Paths for a library recorded in the registry (`None` = default).
    fn library_paths(&self, library: Option<&str>) -> ManagedPaths {
        match library {
            None => self.paths.clone(),
            Some(root) => ManagedPaths::new(root),
        }
    }

    fn install_paths(&self, rec: &InstallRecord) -> ManagedPaths {
        self.library_paths(rec.library.as_deref())
    }

    /// Where new installs go, as stored in the registry (`None` = default).
    fn new_install_library(&self) -> Result<Option<String>> {
        let root = self.registry.settings()?.install_root;
        if let Some(r) = &root {
            ManagedPaths::new(r).ensure()?;
        }
        Ok(root)
    }

    /// The folder new installs go into.
    pub fn install_root(&self) -> Result<PathBuf> {
        Ok(self
            .new_install_library()?
            .map(PathBuf::from)
            .unwrap_or_else(|| self.paths.root.clone()))
    }

    fn forbidden_library_parents(&self) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = [
            "SystemRoot",
            "ProgramFiles",
            "ProgramFiles(x86)",
            "ProgramW6432",
            "ProgramData",
            "APPDATA",
        ]
        .iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .collect();
        out.extend(self.data_dir.clone());
        if let Ok(exe) = std::env::current_exe()
            && let Some(dir) = exe.parent()
        {
            out.push(dir.to_path_buf());
        }
        let mut libraries = vec![self.paths.clone()];
        libraries.extend(
            self.registry
                .known_libraries()
                .unwrap_or_default()
                .into_iter()
                .map(ManagedPaths::new),
        );
        for l in libraries {
            out.extend([l.apps, l.staging, l.downloads]);
        }
        out
    }

    /// Validates and sets the root for *new* installs. Existing installs stay where they
    /// are and keep being managed there. `None` restores the default folder.
    pub fn set_install_root(&self, root: Option<&Path>) -> Result<PathBuf> {
        let chosen = match root {
            None => None,
            Some(p) => {
                let canon = validate_library_root(p, &self.forbidden_library_parents())?;
                let default = platform::dunce_canonicalize(&self.paths.root);
                if default.as_deref() == Some(canon.as_path()) {
                    None
                } else {
                    Some(canon.display().to_string())
                }
            }
        };
        let mut s = self.registry.settings()?;
        s.install_root = chosen;
        self.registry.save_settings(&s)?;
        self.install_root()
    }

    // ------------------------------------------------------------ notifications

    /// Updates that have not been announced before, marked as announced. A version is
    /// announced at most once, so unchanged versions never notify again.
    pub fn take_new_update_notices(&self) -> Result<Vec<UpdateNotice>> {
        let mut out = Vec::new();
        for v in self.list_apps()? {
            let (true, Some(latest)) = (v.update_available, v.latest.as_ref()) else {
                continue;
            };
            let key = format!("notified:{}", v.id);
            if self.registry.kv_get(&key)?.as_deref() == Some(latest.version.as_str()) {
                continue;
            }
            self.registry.kv_set(&key, &latest.version)?;
            out.push(UpdateNotice {
                app_id: v.id,
                name: v.name,
                version: latest.version.clone(),
            });
        }
        Ok(out)
    }

    // ------------------------------------------------------------ release discovery

    fn summarize(
        &self,
        app: &CatalogApp,
        releases: &[crate::releases::GhRelease],
    ) -> ReleaseSummary {
        ReleaseSummary::build(
            app,
            app.installable_adapter(),
            releases,
            &self.github.config().download_base,
        )
    }

    fn check_state(&self, app: &CatalogApp) -> Option<CheckState> {
        if let Some(s) = self.checks.lock().unwrap().get(&app.id) {
            return Some(s.clone());
        }
        // Offline-friendly: derive from the persisted cache without touching the network.
        let cached = self
            .github
            .cached_releases(&app.repo, &self.registry)
            .ok()
            .flatten()?;
        let state = CheckState {
            summary: self.summarize(app, &cached.releases),
            checked_at: cached.fetched_at,
            source: FetchSource::StaleCache,
            warning: None,
        };
        self.checks
            .lock()
            .unwrap()
            .insert(app.id.clone(), state.clone());
        Some(state)
    }

    async fn refresh(&self, app: &CatalogApp) -> Result<CheckState> {
        let outcome = self.github.fetch_releases(&app.repo, &self.registry).await;
        match outcome {
            Ok(o) => {
                let state = CheckState {
                    summary: self.summarize(app, &o.releases),
                    checked_at: o.fetched_at,
                    source: o.source,
                    warning: o.warning,
                };
                self.checks
                    .lock()
                    .unwrap()
                    .insert(app.id.clone(), state.clone());
                self.check_errors.lock().unwrap().remove(&app.id);
                Ok(state)
            }
            Err(e) => {
                self.check_errors
                    .lock()
                    .unwrap()
                    .insert(app.id.clone(), e.to_string());
                Err(e)
            }
        }
    }

    /// Queries GitHub for one app (or all apps) and returns refreshed views.
    pub async fn check_for_updates(&self, app_id: Option<&str>) -> Result<Vec<AppView>> {
        let targets: Vec<&CatalogApp> = match app_id {
            Some(id) => vec![self.app(id)?],
            None => self.catalog.applications.iter().collect(),
        };
        let results = join_all(targets.iter().map(|a| self.refresh(a))).await;
        for (app, r) in targets.iter().zip(results) {
            if let Err(e) = r {
                tracing::warn!(app = %app.id, error = %e, "release check failed");
            }
        }
        self.list_apps()
    }

    pub fn list_apps(&self) -> Result<Vec<AppView>> {
        let channel = self.registry.settings()?.channel;
        let snapshot = platform::process_snapshot().unwrap_or_default();
        self.catalog
            .applications
            .iter()
            .map(|app| self.view(app, channel, &snapshot))
            .collect()
    }

    pub fn app_view(&self, app_id: &str) -> Result<AppView> {
        let app = self.app(app_id)?;
        let channel = self.registry.settings()?.channel;
        let snapshot = platform::process_snapshot().unwrap_or_default();
        self.view(app, channel, &snapshot)
    }

    fn view(
        &self,
        app: &CatalogApp,
        channel: crate::releases::Channel,
        snapshot: &[RunningProcess],
    ) -> Result<AppView> {
        let install = self.registry.get_install(&app.id)?;
        let state = self.check_state(app);
        let error = self.check_errors.lock().unwrap().get(&app.id).cloned();
        let unsupported_reason = app.unsupported_reason();
        let supported = unsupported_reason.is_none();

        let latest = state
            .as_ref()
            .and_then(|s| s.summary.latest_installable(channel));
        let newest = state.as_ref().and_then(|s| s.summary.newest(channel));
        let newest_unavailable = match (newest, latest) {
            (Some(n), Some(l)) if n.version > l.version => Some(ReleaseView::from(n)),
            (Some(n), None) => Some(ReleaseView::from(n)),
            _ => None,
        };

        let installed_version = install
            .as_ref()
            .and_then(|i| semver::Version::parse(&i.version).ok());
        let update_available =
            matches!((&installed_version, latest), (Some(iv), Some(l)) if l.version > *iv);

        let running = match &install {
            Some(rec) => self
                .install_paths(rec)
                .app_dir(&app.id)
                .map(|d| !platform::processes_in(snapshot, &d).is_empty())
                .unwrap_or(false),
            None => false,
        };

        let status = match (&install, supported) {
            (Some(_), _) if update_available && supported => AppStatus::UpdateAvailable,
            (Some(_), _) => AppStatus::Installed,
            (None, true) if latest.is_some() => AppStatus::NotInstalled,
            (None, true) if state.is_none() => AppStatus::NotInstalled,
            _ => AppStatus::Unavailable,
        };

        let unsupported_reason = unsupported_reason.or_else(|| {
            (status == AppStatus::Unavailable).then(|| match (&state, newest) {
                (Some(s), None) if s.summary.releases.is_empty() => {
                    "No releases have been published yet.".to_string()
                }
                (_, Some(n)) => ReleaseView::from(n)
                    .unavailable_reason
                    .unwrap_or_else(|| "No compatible Windows x64 release found.".into()),
                _ => "No compatible Windows x64 release found.".into(),
            })
        });

        Ok(AppView {
            id: app.id.clone(),
            name: app.name.clone(),
            description: app.description.clone(),
            category: app.category,
            repo: app.repo.clone(),
            repo_url: app.repo_url(),
            releases_url: app.releases_url(),
            supported,
            unsupported_reason,
            audit: app.windows_x64.as_ref().map(|w| w.audit.clone()),
            status,
            installed: install.as_ref().map(|i| self.installed_view(i)),
            latest: latest.map(ReleaseView::from),
            newest_unavailable,
            update_available,
            check: ReleaseCheck {
                checked_at: state.as_ref().map(|s| s.checked_at),
                source: state.as_ref().map(|s| s.source),
                warning: state.as_ref().and_then(|s| s.warning.clone()),
                error,
            },
            running,
            busy: self.is_busy(&app.id),
        })
    }

    fn installed_view(&self, i: &InstallRecord) -> InstalledView {
        let path = self
            .install_paths(i)
            .version_dir(&i.app_id, &i.dir_name)
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        InstalledView {
            version: i.version.clone(),
            tag: i.tag.clone(),
            path,
            executable: i.executable.clone(),
            installed_at: i.installed_at,
            sha256: i.sha256.clone(),
            verification: i.verification.clone(),
            previous_version: i.previous.as_ref().map(|p| p.version.clone()),
            shortcut_path: i.shortcut_path.clone(),
            shortcut_exists: i.shortcut_path.as_deref().is_some_and(|p| {
                let path = Path::new(p);
                path.is_file() && platform::is_owned_shortcut(path, &i.app_id)
            }),
        }
    }

    /// All releases for an app (both channels), newest first, with installability.
    pub async fn versions(&self, app_id: &str) -> Result<Vec<ReleaseView>> {
        let app = self.app(app_id)?;
        let state = match self.check_state(app) {
            Some(s) => s,
            None => self.refresh(app).await?,
        };
        Ok(state
            .summary
            .releases
            .iter()
            .map(ReleaseView::from)
            .collect())
    }

    // ------------------------------------------------------------ install / update

    /// Installs `version` (or the newest installable release in the current channel).
    /// If the app is already installed this performs a transactional update/switch.
    pub async fn install(
        &self,
        app_id: &str,
        version: Option<&str>,
        sink: ProgressSink,
    ) -> Result<InstallOutcome> {
        self.install_with_options(app_id, version, sink, InstallOptions::default())
            .await
    }

    pub async fn install_with_options(
        &self,
        app_id: &str,
        version: Option<&str>,
        sink: ProgressSink,
        options: InstallOptions,
    ) -> Result<InstallOutcome> {
        let app = self.app(app_id)?;
        let adapter = app
            .installable_adapter()
            .filter(|_| app.unsupported_reason().is_none())
            .ok_or_else(|| CoreError::Unsupported(app.unsupported_reason().unwrap_or_default()))?;
        let _busy = self.acquire(app)?;

        let op_id = uuid::Uuid::new_v4().simple().to_string();
        let cancel = CancellationToken::new();
        self.ops
            .lock()
            .unwrap()
            .insert(op_id.clone(), (app.id.clone(), cancel.clone()));
        let _op = OpGuard {
            ops: &self.ops,
            id: op_id.clone(),
        };

        let existing = self.registry.get_install(&app.id)?;
        let kind = if existing.is_some() {
            OpKind::Update
        } else {
            OpKind::Install
        };
        let emit = |phase: Phase, done: u64, total: u64, message: Option<String>| {
            sink(ProgressEvent {
                op_id: op_id.clone(),
                app_id: app.id.clone(),
                kind,
                phase,
                done,
                total,
                message,
            })
        };
        emit(Phase::Resolving, 0, 0, None);

        let result = async {
            let state = match self.refresh(app).await {
                Ok(s) => s,
                Err(e) => self.check_state(app).ok_or(e)?,
            };
            let channel = self.registry.settings()?.channel;
            let release = match version {
                Some(v) => state
                    .summary
                    .find_version(v)
                    .ok_or_else(|| CoreError::ReleaseNotFound(format!("{} {v}", app.name)))?,
                None => state.summary.latest_installable(channel).ok_or_else(|| {
                    CoreError::ReleaseNotFound(format!(
                        "no installable {} release in the {} channel",
                        app.name,
                        channel.as_str()
                    ))
                })?,
            }
            .clone();
            if !release.installable() {
                return Err(CoreError::Unsupported(
                    ReleaseView::from(&release)
                        .unavailable_reason
                        .unwrap_or_default(),
                ));
            }
            if let Some(ex) = &existing
                && ex.version == release.version_text
            {
                return Err(CoreError::AlreadyInstalled(format!(
                    "{} {} is already installed.",
                    app.name, ex.version
                )));
            }
            self.run_transaction(
                app,
                adapter,
                &release,
                &op_id,
                kind,
                existing.as_ref(),
                &cancel,
                &options,
                &emit,
            )
            .await
        }
        .await;

        let now = now_unix();
        let kind_str = match kind {
            OpKind::Install => "install",
            OpKind::Update => "update",
        };
        match &result {
            Ok(o) => {
                self.registry.record_event(
                    &app.id,
                    kind_str,
                    Some(&o.version),
                    "success",
                    None,
                    now,
                )?;
                emit(
                    Phase::Completed,
                    1,
                    1,
                    Some(format!("{} {} is ready.", app.name, o.version)),
                );
            }
            Err(CoreError::Cancelled) => {
                self.registry
                    .record_event(&app.id, kind_str, version, "cancelled", None, now)?;
                emit(
                    Phase::Cancelled,
                    0,
                    0,
                    Some("Cancelled. Nothing was changed.".into()),
                );
            }
            Err(e) => {
                self.registry.record_event(
                    &app.id,
                    kind_str,
                    version,
                    "failed",
                    Some(&e.to_string()),
                    now,
                )?;
                emit(Phase::Failed, 0, 0, Some(e.to_string()));
            }
        }
        result
    }

    /// Updates to the newest installable release in the current channel.
    pub async fn update(&self, app_id: &str, sink: ProgressSink) -> Result<InstallOutcome> {
        let app = self.app(app_id)?;
        let install = self
            .registry
            .get_install(app_id)?
            .ok_or_else(|| CoreError::NotInstalled(app.name.clone()))?;
        let state = self
            .refresh(app)
            .await
            .or_else(|e| self.check_state(app).ok_or(e))?;
        let channel = self.registry.settings()?.channel;
        let latest = state.summary.latest_installable(channel).ok_or_else(|| {
            CoreError::ReleaseNotFound(format!("no installable {} release", app.name))
        })?;
        let current = semver::Version::parse(&install.version).ok();
        if current.is_some_and(|c| c >= latest.version) {
            return Err(CoreError::AlreadyInstalled(format!(
                "{} {} is up to date.",
                app.name, install.version
            )));
        }
        let v = latest.version_text.clone();
        self.install(app_id, Some(&v), sink).await
    }

    /// Updates every installed app sequentially. Running apps and apps without a
    /// compatible release are skipped with a reason; nothing is closed forcibly.
    pub async fn update_all(&self, sink: ProgressSink) -> Result<UpdateAllSummary> {
        let mut summary = UpdateAllSummary::default();
        let installs = self.registry.list_installs()?;
        let _ = self.check_for_updates(None).await;
        let channel = self.registry.settings()?.channel;
        for rec in installs {
            let Ok(app) = self.app(&rec.app_id) else {
                continue;
            };
            let item = |to: Option<String>, reason: Option<String>| UpdateAllItem {
                app_id: app.id.clone(),
                name: app.name.clone(),
                from: Some(rec.version.clone()),
                to,
                reason,
            };
            if let Some(reason) = app.unsupported_reason() {
                summary.skipped.push(item(None, Some(reason)));
                continue;
            }
            let latest = self
                .check_state(app)
                .and_then(|s| s.summary.latest_installable(channel).cloned());
            let Some(latest) = latest else {
                summary
                    .skipped
                    .push(item(None, Some("No compatible release found.".into())));
                continue;
            };
            let current = semver::Version::parse(&rec.version).ok();
            if current.is_some_and(|c| c >= latest.version) {
                continue; // up to date: not listed
            }
            if self.is_running(&app.id, &self.install_paths(&rec))? {
                summary.skipped.push(item(
                    Some(latest.version_text.clone()),
                    Some(format!("{} is running.", app.name)),
                ));
                continue;
            }
            match self
                .install(&app.id, Some(&latest.version_text), sink.clone())
                .await
            {
                Ok(o) => summary.updated.push(item(Some(o.version), None)),
                Err(CoreError::AppRunning(_)) => summary.skipped.push(item(
                    Some(latest.version_text.clone()),
                    Some(format!("{} is running.", app.name)),
                )),
                Err(e) => summary
                    .failed
                    .push(item(Some(latest.version_text.clone()), Some(e.to_string()))),
            }
        }
        Ok(summary)
    }

    fn is_running(&self, app_id: &str, paths: &ManagedPaths) -> Result<bool> {
        let dir = paths.app_dir(app_id)?;
        if !dir.exists() {
            return Ok(false);
        }
        let snapshot = platform::process_snapshot()?;
        Ok(!platform::processes_in(&snapshot, &dir).is_empty())
    }

    fn ensure_not_running(&self, app: &CatalogApp, paths: &ManagedPaths) -> Result<()> {
        if self.is_running(&app.id, paths)? {
            return Err(CoreError::AppRunning(app.name.clone()));
        }
        Ok(())
    }

    fn check_space(&self, root: &Path, needed: u64) -> Result<()> {
        match platform::available_space(root) {
            Ok(available) if available < needed + FREE_SPACE_MARGIN => Err(CoreError::DiskSpace {
                needed: needed + FREE_SPACE_MARGIN,
                available,
            }),
            Ok(_) => Ok(()),
            // Unknown free space is not fatal; the write itself will fail honestly.
            Err(e) => {
                tracing::warn!(error = %e, "free space check unavailable");
                Ok(())
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_transaction(
        &self,
        app: &CatalogApp,
        adapter: &WindowsAdapter,
        release: &ResolvedRelease,
        op_id: &str,
        kind: OpKind,
        existing: Option<&InstallRecord>,
        cancel: &CancellationToken,
        options: &InstallOptions,
        emit: &(dyn Fn(Phase, u64, u64, Option<String>) + Sync),
    ) -> Result<InstallOutcome> {
        let asset = release
            .asset
            .as_ref()
            .map_err(|e| CoreError::Unsupported(e.clone()))?;
        let expected_sha = asset
            .sha256
            .clone()
            .ok_or_else(|| CoreError::Integrity("no SHA-256 digest published".into()))?;
        let label = release.version_text.replace('+', "_");
        let dir_name = format!("{label}_{}", &op_id[..8]);
        if !is_valid_version_label(&dir_name) {
            return Err(CoreError::InvalidInput(format!(
                "unusable version {:?}",
                release.version_text
            )));
        }
        let kind_str = if kind == OpKind::Install {
            "install"
        } else {
            "update"
        };
        // Updates stay in the library the app already lives in; new installs use the
        // currently selected root. Installs are never moved silently.
        let library = match existing {
            Some(rec) => rec.library.clone(),
            None => match &options.library {
                Some(root) => Some(
                    validate_library_root(root, &self.forbidden_library_parents())?
                        .display()
                        .to_string(),
                ),
                None => self.new_install_library()?,
            },
        };
        let paths = self.library_paths(library.as_deref());
        paths.ensure()?;
        self.registry.begin_operation(
            op_id,
            &app.id,
            kind_str,
            Some(&release.version_text),
            Some(&dir_name),
            library.as_deref(),
            now_unix(),
        )?;

        let mut txn = Txn {
            op_id: op_id.to_string(),
            app_id: app.id.clone(),
            staging: paths.staging_dir(op_id)?,
            download: paths.download_file(op_id)?,
            paths,
            library,
            dir_name,
            prepared: false,
            moved: false,
        };

        let result = self
            .transaction_steps(
                app,
                adapter,
                release,
                asset,
                &expected_sha,
                existing,
                cancel,
                emit,
                &mut txn,
            )
            .await;

        match result {
            Ok(outcome) => {
                self.registry
                    .finish_operation(op_id, "completed", None, now_unix())?;
                Ok(outcome)
            }
            Err(e) => {
                if self.fail_point() == Some(FailPoint::CrashBeforeCommit) && txn.moved {
                    // Simulated power loss: leave everything as-is for recovery to find.
                    return Err(e);
                }
                self.rollback_txn(&txn);
                let state = if matches!(e, CoreError::Cancelled) {
                    "cancelled"
                } else {
                    "failed"
                };
                let _ =
                    self.registry
                        .finish_operation(op_id, state, Some(&e.to_string()), now_unix());
                Err(e)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn transaction_steps(
        &self,
        app: &CatalogApp,
        adapter: &WindowsAdapter,
        release: &ResolvedRelease,
        asset: &ResolvedAsset,
        expected_sha: &str,
        existing: Option<&InstallRecord>,
        cancel: &CancellationToken,
        emit: &(dyn Fn(Phase, u64, u64, Option<String>) + Sync),
        txn: &mut Txn,
    ) -> Result<InstallOutcome> {
        let mut warnings = Vec::new();
        if existing.is_some() {
            self.ensure_not_running(app, &txn.paths)?;
        }
        // Rough pre-check (archive + typical expansion); refined after inspection.
        self.check_space(&txn.paths.root, asset.size.saturating_mul(3))?;

        // 1. Download (bounded concurrency) with streaming hash.
        let computed = {
            let _slot = tokio::select! {
                s = self.downloads.acquire() => s.map_err(|_| CoreError::Cancelled)?,
                _ = cancel.cancelled() => return Err(CoreError::Cancelled),
            };
            emit(Phase::Downloading, 0, asset.size, Some(asset.name.clone()));
            download_to_file(
                self.github.http(),
                &self.github.config().policy,
                &asset.url,
                asset.size,
                &txn.download,
                cancel,
                |d, t| emit(Phase::Downloading, d, t, None),
            )
            .await?
        };

        // 2. Verify integrity before anything is unpacked.
        emit(Phase::Verifying, 0, 1, None);
        if !constant_time_eq(computed.as_bytes(), expected_sha.as_bytes()) {
            return Err(CoreError::Integrity(format!(
                "SHA-256 of the download ({computed}) does not match GitHub's release metadata ({expected_sha})"
            )));
        }
        let mut verification = "github-digest".to_string();
        if let Some(url) = &asset.checksums_url {
            match self
                .github
                .fetch_small_text(url, MAX_CHECKSUM_FILE_BYTES as usize)
                .await
            {
                Ok(text) => match find_in_checksums(&text, &asset.name) {
                    Some(h) if constant_time_eq(h.as_bytes(), computed.as_bytes()) => {
                        verification = "github-digest+sha256sums".into();
                    }
                    Some(_) => {
                        return Err(CoreError::Integrity(
                            "the release's SHA256SUMS file disagrees with the downloaded file"
                                .into(),
                        ));
                    }
                    None => warnings
                        .push("The release's checksum file does not list this download.".into()),
                },
                Err(e) => warnings.push(format!("Could not cross-check SHA256SUMS: {e}")),
            }
        }
        if cancel.is_cancelled() {
            return Err(CoreError::Cancelled);
        }

        // 3. Validate the archive layout and extract into private staging.
        let root = asset.archive_root.clone();
        let expanded = extract::inspect(&txn.download, &root, &self.limits)?;
        self.check_space(&txn.paths.root, expanded)?;
        emit(Phase::Extracting, 0, expanded, None);
        std::fs::create_dir_all(&txn.staging).map_err(CoreError::io("creating staging folder"))?;
        let content = txn.staging.join("content");
        let files = {
            let zip = txn.download.clone();
            let dest = content.clone();
            let archive_root = root.clone();
            let limits = self.limits;
            let c = cancel.clone();
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<(u64, u64)>();
            let handle = tokio::task::spawn_blocking(move || {
                let mut last = 0u64;
                extract::extract_portable_zip(
                    &zip,
                    &dest,
                    &archive_root,
                    &limits,
                    &|| c.is_cancelled(),
                    &mut |d, t| {
                        if d == t || d - last >= 4 * 1024 * 1024 {
                            last = d;
                            let _ = tx.send((d, t));
                        }
                    },
                )
            });
            let forward = async {
                while let Some((d, t)) = rx.recv().await {
                    emit(Phase::Extracting, d, t, None);
                }
            };
            let (res, ()) = tokio::join!(handle, forward);
            res.map_err(|e| CoreError::UnsafeArchive(format!("extraction task failed: {e}")))??
        };

        // 4. Apply audited adjustments and verify the executable.
        let mut files: Vec<(String, u64)> = files;
        for strip in &adapter.strip_files {
            if let Some(pos) = files
                .iter()
                .position(|(p, _)| p.eq_ignore_ascii_case(strip))
            {
                std::fs::remove_file(content.join(strip))
                    .map_err(CoreError::io(format!("removing {strip}")))?;
                files.remove(pos);
            }
        }
        if !files.iter().any(|(p, _)| p == &asset.executable) {
            return Err(CoreError::Layout(format!(
                "{} was not found in {root}/",
                asset.executable
            )));
        }
        extract::verify_x64_pe(&content.join(&asset.executable))?;
        if cancel.is_cancelled() {
            return Err(CoreError::Cancelled);
        }

        // 5. Activate: journal the manifest, move into place, then commit.
        emit(Phase::Activating, 0, 1, None);
        if existing.is_some() {
            // The user may have opened the app while we were downloading.
            self.ensure_not_running(app, &txn.paths)?;
        }
        self.registry.prepare_activation(
            &txn.op_id,
            &app.id,
            &txn.dir_name,
            txn.library.as_deref(),
            &files,
        )?;
        txn.prepared = true;
        let app_dir = txn.paths.app_dir(&app.id)?;
        std::fs::create_dir_all(&app_dir).map_err(CoreError::io("creating app folder"))?;
        let target = txn.paths.version_dir(&app.id, &txn.dir_name)?;
        std::fs::rename(&content, &target).map_err(CoreError::io("activating new version"))?;
        txn.moved = true;

        match self.fail_point() {
            Some(FailPoint::ErrorBeforeCommit) | Some(FailPoint::CrashBeforeCommit) => {
                return Err(CoreError::Io {
                    context: "simulated failure before commit".into(),
                    source: std::io::Error::other("fail point"),
                });
            }
            None => {}
        }

        let record = InstallRecord {
            app_id: app.id.clone(),
            version: release.version_text.clone(),
            tag: release.tag.clone(),
            dir_name: txn.dir_name.clone(),
            executable: asset.executable.clone(),
            asset_name: asset.name.clone(),
            sha256: computed.clone(),
            verification: verification.clone(),
            installed_at: now_unix(),
            previous: None,
            library: txn.library.clone(),
            shortcut_path: existing.and_then(|e| e.shortcut_path.clone()),
        };
        let evicted = self.registry.commit_activation(&txn.op_id, &record)?;

        // 6. Post-commit cleanup. Failures here never undo a successful activation.
        emit(Phase::CleaningUp, 0, 1, None);
        if let Some(old) = evicted {
            match self.remove_dir_with_manifest(&app.id, &old) {
                Ok(r) if !r.kept_entries.is_empty() => warnings.push(format!(
                    "Kept {} item(s) CraftHub did not install in an old version folder.",
                    r.kept_entries.len()
                )),
                Ok(_) => {}
                Err(e) => warnings.push(format!("Could not remove an old version: {e}")),
            }
        }
        let _ = txn.paths.remove_temp(&txn.staging);
        let _ = txn.paths.remove_temp(&txn.download);

        if existing.is_some_and(|e| e.shortcut_path.is_some())
            && let Err(e) = self.sync_shortcut(&app.id)
        {
            warnings.push(format!("Could not update the desktop shortcut: {e}"));
        }

        Ok(InstallOutcome {
            app_id: app.id.clone(),
            op_id: txn.op_id.clone(),
            version: release.version_text.clone(),
            previous_version: existing.map(|e| e.version.clone()),
            verification,
            path: target.display().to_string(),
            warnings,
        })
    }

    fn rollback_txn(&self, txn: &Txn) {
        if txn.moved {
            if let Err(e) = self.remove_dir_with_manifest(&txn.app_id, &txn.dir_name) {
                tracing::error!(error = %e, "failed to remove partially activated version");
            }
        } else if txn.prepared {
            let _ = self.registry.delete_files_for(&txn.app_id, &txn.dir_name);
        }
        let _ = txn.paths.remove_temp(&txn.staging);
        let _ = txn.paths.remove_temp(&txn.download);
        remove_app_dir_if_empty(&txn.paths, &txn.app_id);
    }

    /// Removes a version folder using its manifest, in the library the manifest names.
    fn remove_dir_with_manifest(&self, app_id: &str, dir_name: &str) -> Result<RemovalReport> {
        let Some(library) = self.registry.manifest_library(app_id, dir_name)? else {
            return Ok(RemovalReport {
                dir_removed: true,
                ..Default::default()
            });
        };
        let paths = self.library_paths(library.as_deref());
        let manifest = self.registry.files_for(app_id, dir_name)?;
        let report = remove_version_dir(&paths, app_id, dir_name, &manifest)?;
        self.registry.delete_files_for(app_id, dir_name)?;
        remove_app_dir_if_empty(&paths, app_id);
        Ok(report)
    }

    // ------------------------------------------------------------ rollback / uninstall / launch

    /// Switches back to the retained previous version.
    pub fn rollback(&self, app_id: &str) -> Result<InstalledView> {
        let app = self.app(app_id)?;
        let _busy = self.acquire(app)?;
        let rec = self
            .registry
            .get_install(app_id)?
            .ok_or_else(|| CoreError::NotInstalled(app.name.clone()))?;
        let prev = rec.previous.clone().ok_or_else(|| {
            CoreError::Unsupported(format!(
                "No earlier {} version is available to roll back to.",
                app.name
            ))
        })?;
        let paths = self.install_paths(&rec);
        self.ensure_not_running(app, &paths)?;
        let exe = paths
            .version_dir(app_id, &prev.dir_name)?
            .join(&rec.executable);
        self.verify_manifest_file(&paths, app_id, &prev.dir_name, &rec.executable, &exe)?;
        self.registry.swap_active_and_previous(app_id, now_unix())?;
        self.registry.record_event(
            app_id,
            "rollback",
            Some(&prev.version),
            "success",
            Some(&format!("from {}", rec.version)),
            now_unix(),
        )?;
        let rec = self
            .registry
            .get_install(app_id)?
            .ok_or_else(|| CoreError::NotInstalled(app.name.clone()))?;
        Ok(self.installed_view(&rec))
    }

    /// Removes CraftHub-managed files for the app. User data elsewhere is untouched and
    /// unknown files inside the install folder are kept and reported.
    pub fn uninstall(&self, app_id: &str) -> Result<UninstallOutcome> {
        let app = self.app(app_id)?;
        let _busy = self.acquire(app)?;
        let rec = self
            .registry
            .get_install(app_id)?
            .ok_or_else(|| CoreError::NotInstalled(app.name.clone()))?;
        let paths = self.install_paths(&rec);
        self.ensure_not_running(app, &paths)?;
        if let Some(path) = rec.shortcut_path.as_deref() {
            platform::remove_owned_shortcut(Path::new(path), app_id)?;
        }

        let mut dirs = vec![rec.dir_name.clone()];
        if let Some(p) = &rec.previous {
            dirs.push(p.dir_name.clone());
        }
        // The registry row goes first: a crash mid-delete leaves orphan files that the
        // startup sweep removes via their manifests, never a record pointing at nothing.
        self.registry.delete_install(app_id)?;
        let mut removed = 0;
        let mut kept = Vec::new();
        let mut kept_path = None;
        for d in dirs {
            let r = self.remove_dir_with_manifest(app_id, &d)?;
            removed += r.removed_files;
            if !r.kept_entries.is_empty() {
                kept_path = paths
                    .version_dir(app_id, &d)
                    .ok()
                    .map(|p| p.display().to_string());
                kept.extend(r.kept_entries);
            }
        }
        remove_app_dir_if_empty(&paths, app_id);
        self.registry.record_event(
            app_id,
            "uninstall",
            Some(&rec.version),
            "success",
            None,
            now_unix(),
        )?;
        Ok(UninstallOutcome {
            app_id: app_id.to_string(),
            removed_files: removed,
            kept_entries: kept,
            kept_path,
        })
    }

    /// Starts the installed app. Never blocks on the child process.
    pub fn launch(&self, app_id: &str) -> Result<u32> {
        let app = self.app(app_id)?;
        let rec = self
            .registry
            .get_install(app_id)?
            .ok_or_else(|| CoreError::NotInstalled(app.name.clone()))?;
        let paths = self.install_paths(&rec);
        let dir = paths.version_dir(app_id, &rec.dir_name)?;
        let exe = dir.join(&rec.executable);
        self.verify_manifest_file(&paths, app_id, &rec.dir_name, &rec.executable, &exe)?;
        let pid = platform::spawn_detached(&exe, &dir)?;
        tracing::info!(app = %app_id, pid, "launched");
        Ok(pid)
    }

    pub fn install_dir(&self, app_id: &str) -> Result<PathBuf> {
        let app = self.app(app_id)?;
        let rec = self
            .registry
            .get_install(app_id)?
            .ok_or_else(|| CoreError::NotInstalled(app.name.clone()))?;
        self.install_paths(&rec).version_dir(app_id, &rec.dir_name)
    }

    /// Validates a per-application library choice without changing the global default.
    pub fn validate_install_root(&self, root: &Path) -> Result<PathBuf> {
        validate_library_root(root, &self.forbidden_library_parents())
    }

    pub fn create_shortcut(&self, app_id: &str) -> Result<ShortcutView> {
        let app = self.app(app_id)?;
        let rec = self
            .registry
            .get_install(app_id)?
            .ok_or_else(|| CoreError::NotInstalled(app.name.clone()))?;
        let paths = self.install_paths(&rec);
        let dir = paths.version_dir(app_id, &rec.dir_name)?;
        let exe = dir.join(&rec.executable);
        self.verify_manifest_file(&paths, app_id, &rec.dir_name, &rec.executable, &exe)?;
        let preferred = rec.shortcut_path.as_deref().map(Path::new);
        let path = platform::create_or_update_shortcut(&app.name, app_id, &exe, preferred)?;
        self.registry
            .set_shortcut_path(app_id, Some(&path.display().to_string()))?;
        Ok(ShortcutView {
            path: Some(path.display().to_string()),
            exists: true,
        })
    }

    pub fn remove_shortcut(&self, app_id: &str) -> Result<ShortcutView> {
        let app = self.app(app_id)?;
        let rec = self
            .registry
            .get_install(app_id)?
            .ok_or_else(|| CoreError::NotInstalled(app.name.clone()))?;
        if let Some(path) = rec.shortcut_path.as_deref() {
            platform::remove_owned_shortcut(Path::new(path), app_id)?;
        }
        self.registry.set_shortcut_path(app_id, None)?;
        Ok(ShortcutView {
            path: None,
            exists: false,
        })
    }

    fn sync_shortcut(&self, app_id: &str) -> Result<()> {
        let _ = self.create_shortcut(app_id)?;
        Ok(())
    }

    /// Confirms `path` is inside the managed apps folder, is a regular file, and has the
    /// size recorded in the manifest.
    fn verify_manifest_file(
        &self,
        paths: &ManagedPaths,
        app_id: &str,
        dir_name: &str,
        rel: &str,
        path: &Path,
    ) -> Result<()> {
        let missing =
            || CoreError::Launch(format!("{} is missing; reinstall the app.", path.display()));
        let meta = std::fs::symlink_metadata(path).map_err(|_| missing())?;
        if !meta.is_file() || platform::is_link_like(&meta) {
            return Err(CoreError::PathSafety(format!(
                "{} is not a regular file",
                path.display()
            )));
        }
        let canon = platform::dunce_canonicalize(path).ok_or_else(missing)?;
        let apps = platform::dunce_canonicalize(&paths.apps).ok_or_else(missing)?;
        if !canon.starts_with(&apps) {
            return Err(CoreError::PathSafety(
                "executable is outside the managed folder".into(),
            ));
        }
        let manifest = self.registry.files_for(app_id, dir_name)?;
        match manifest.iter().find(|(p, _)| p == rel) {
            Some((_, size)) if *size == meta.len() => Ok(()),
            Some(_) => Err(CoreError::Integrity(format!(
                "{} was modified after installation; reinstall the app.",
                path.display()
            ))),
            None => Err(missing()),
        }
    }

    // ------------------------------------------------------------ recovery

    /// Repairs state after a crash: undoes half-activated versions using the journal,
    /// removes orphaned version folders by manifest, and clears CraftHub temp items.
    ///
    /// Several processes can share the database (GUI + CLI), so every repair first takes
    /// the app's cross-process lock. Anything whose app is locked belongs to a live
    /// operation in another process and is left alone.
    fn recover(&self) -> Result<()> {
        let mut held: HashMap<String, Option<AppLock>> = HashMap::new();
        let mut lock_for = |app_id: &str| -> bool {
            if !is_valid_app_id(app_id) {
                return false;
            }
            held.entry(app_id.to_string())
                .or_insert_with(|| self.locks.try_lock(app_id).ok().flatten())
                .is_some()
        };

        for op in self.registry.unfinished_operations()? {
            if !lock_for(&op.app_id) {
                continue; // still running elsewhere
            }
            let rec = self.registry.get_install(&op.app_id)?;
            let committed = op.state == "committed";
            if !committed && let Some(dir) = &op.target_dir_name {
                let referenced = rec.as_ref().is_some_and(|r| {
                    &r.dir_name == dir || r.previous.as_ref().is_some_and(|p| &p.dir_name == dir)
                });
                if !referenced && let Err(e) = self.remove_dir_with_manifest(&op.app_id, dir) {
                    tracing::error!(error = %e, op = %op.id, "recovery could not remove version");
                }
            }
            let (state, msg) = if committed {
                ("completed", "recovered after restart")
            } else {
                ("failed", "interrupted; previous version kept")
            };
            self.registry
                .finish_operation(&op.id, state, Some(msg), now_unix())?;
            self.registry.record_event(
                &op.app_id,
                &op.kind,
                op.target_version.as_deref(),
                if committed { "success" } else { "interrupted" },
                Some(msg),
                now_unix(),
            )?;
        }

        // Orphan sweep: manifests for folders that are neither active nor previous.
        for app in &self.catalog.applications {
            let dirs = self.registry.manifest_dirs(&app.id)?;
            if dirs.is_empty() || !lock_for(&app.id) {
                continue;
            }
            let rec = self.registry.get_install(&app.id)?;
            for dir in dirs {
                let referenced = rec.as_ref().is_some_and(|r| {
                    r.dir_name == dir || r.previous.as_ref().is_some_and(|p| p.dir_name == dir)
                });
                if !referenced && let Err(e) = self.remove_dir_with_manifest(&app.id, &dir) {
                    tracing::warn!(error = %e, app = %app.id, "orphan sweep failed");
                }
            }
        }

        // Temp sweep across every library. A temp item is named after its operation; an
        // operation row is always written (under the app lock) before its temp files, so:
        // unknown op => orphan; known op whose app lock we can take => dead.
        let mut libraries = vec![self.paths.clone()];
        for l in self.registry.known_libraries()? {
            libraries.push(ManagedPaths::new(l));
        }
        if let Some(r) = self.registry.settings()?.install_root {
            libraries.push(ManagedPaths::new(r));
        }
        for lib in libraries {
            if !lib.has_marker() {
                continue;
            }
            for item in lib.temp_items() {
                let name = item
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let op_id = name.split('.').next().unwrap_or_default();
                let removable = match self.registry.operation_app(op_id)? {
                    None => true,
                    Some(app_id) => lock_for(&app_id),
                };
                if removable && let Err(e) = lib.remove_temp(&item) {
                    tracing::warn!(error = %e, "could not remove temp item");
                }
            }
        }
        Ok(())
    }
}
