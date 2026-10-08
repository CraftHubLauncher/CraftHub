//! IPC commands. All inputs are validated by the engine (app ids, versions); URLs and
//! paths are always derived server-side from the catalog/registry, never taken from the UI.

use std::sync::Arc;

use crafthub_core::engine::{
    InstallOptions, InstallOutcome, InstalledView, ReleaseView, ShortcutView, UninstallOutcome,
    UpdateAllSummary,
};
use crafthub_core::registry::{EventRow, Settings};
use crafthub_core::{AppView, CoreError, Engine, ProgressSink, platform};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_opener::OpenerExt;

use crate::{AppState, EVENT_APPS_CHANGED, EVENT_PROGRESS};

type Cmd<T> = Result<T, CoreError>;

fn engine(state: &State<'_, AppState>) -> Cmd<Arc<Engine>> {
    state
        .engine
        .clone()
        .map_err(|e| CoreError::Internal(format!("CraftHub could not start its engine: {e}")))
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Cmd<T> + Send + 'static) -> Cmd<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| CoreError::Internal(e.to_string()))?
}

fn progress_sink(app: &AppHandle) -> ProgressSink {
    let app = app.clone();
    Arc::new(move |event| {
        let _ = app.emit(EVENT_PROGRESS, event);
    })
}

fn notify_changed(app: &AppHandle, engine: &Engine) {
    if let Ok(views) = engine.list_apps() {
        let _ = app.emit(EVENT_APPS_CHANGED, views);
    }
}

fn check_version_arg(v: &Option<String>) -> Cmd<()> {
    match v {
        Some(s)
            if s.is_empty()
                || s.len() > 64
                || !s
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-+".contains(&b)) =>
        {
            Err(CoreError::InvalidInput("bad version".into()))
        }
        _ => Ok(()),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Environment {
    version: &'static str,
    platform_supported: bool,
    /// Default install root (`%LOCALAPPDATA%\Programs\CraftHub`).
    default_install_root: Option<String>,
    /// Root used for new installs (default or user-chosen).
    install_root: Option<String>,
    data_dir: String,
    logs_dir: String,
    engine_error: Option<String>,
}

#[tauri::command]
pub async fn get_environment(state: State<'_, AppState>) -> Cmd<Environment> {
    let e = state.engine.as_ref().ok();
    Ok(Environment {
        version: env!("CARGO_PKG_VERSION"),
        platform_supported: cfg!(all(windows, target_arch = "x86_64")),
        default_install_root: e.map(|e| e.paths().root.display().to_string()),
        install_root: e
            .and_then(|e| e.install_root().ok())
            .map(|p| p.display().to_string()),
        data_dir: state.data_dir.display().to_string(),
        logs_dir: state.logs_dir.display().to_string(),
        engine_error: state.engine.as_ref().err().cloned(),
    })
}

#[tauri::command]
pub async fn list_apps(state: State<'_, AppState>) -> Cmd<Vec<AppView>> {
    let e = engine(&state)?;
    blocking(move || e.list_apps()).await
}

#[tauri::command]
pub async fn check_for_updates(
    state: State<'_, AppState>,
    app_id: Option<String>,
) -> Cmd<Vec<AppView>> {
    engine(&state)?.check_for_updates(app_id.as_deref()).await
}

#[tauri::command]
pub async fn get_versions(state: State<'_, AppState>, app_id: String) -> Cmd<Vec<ReleaseView>> {
    engine(&state)?.versions(&app_id).await
}

#[tauri::command]
pub async fn install_app(
    app: AppHandle,
    state: State<'_, AppState>,
    app_id: String,
    version: Option<String>,
    install_root: Option<String>,
    create_shortcut: bool,
) -> Cmd<InstallOutcome> {
    check_version_arg(&version)?;
    let e = engine(&state)?;
    let library = install_root
        .as_deref()
        .map(std::path::PathBuf::from)
        .map(|p| e.validate_install_root(&p))
        .transpose()?;
    let r = e
        .install_with_options(
            &app_id,
            version.as_deref(),
            progress_sink(&app),
            InstallOptions { library },
        )
        .await;
    let r = match r {
        Ok(mut out) if create_shortcut => {
            if let Err(err) = e.create_shortcut(&app_id) {
                out.warnings
                    .push(format!("Could not create the desktop shortcut: {err}"));
            }
            Ok(out)
        }
        other => other,
    };
    notify_changed(&app, &e);
    r
}

#[tauri::command]
pub async fn update_app(
    app: AppHandle,
    state: State<'_, AppState>,
    app_id: String,
    create_shortcut: bool,
) -> Cmd<InstallOutcome> {
    let e = engine(&state)?;
    let r = e.update(&app_id, progress_sink(&app)).await;
    let r = match r {
        Ok(mut out) if create_shortcut => {
            if let Err(err) = e.create_shortcut(&app_id) {
                out.warnings
                    .push(format!("Could not create the desktop shortcut: {err}"));
            }
            Ok(out)
        }
        other => other,
    };
    notify_changed(&app, &e);
    r
}

#[tauri::command]
pub async fn update_all(app: AppHandle, state: State<'_, AppState>) -> Cmd<UpdateAllSummary> {
    let e = engine(&state)?;
    let r = e.update_all(progress_sink(&app)).await;
    notify_changed(&app, &e);
    r
}

#[tauri::command]
pub async fn uninstall_app(
    app: AppHandle,
    state: State<'_, AppState>,
    app_id: String,
) -> Cmd<UninstallOutcome> {
    let e = engine(&state)?;
    let e2 = e.clone();
    let r = blocking(move || e2.uninstall(&app_id)).await;
    notify_changed(&app, &e);
    r
}

#[tauri::command]
pub async fn rollback_app(
    app: AppHandle,
    state: State<'_, AppState>,
    app_id: String,
) -> Cmd<InstalledView> {
    let e = engine(&state)?;
    let e2 = e.clone();
    let r = blocking(move || e2.rollback(&app_id)).await;
    notify_changed(&app, &e);
    r
}

#[tauri::command]
pub async fn launch_app(state: State<'_, AppState>, app_id: String) -> Cmd<u32> {
    let e = engine(&state)?;
    blocking(move || e.launch(&app_id)).await
}

#[tauri::command]
pub async fn cancel_operation(state: State<'_, AppState>, op_id: String) -> Cmd<bool> {
    Ok(engine(&state)?.cancel(&op_id))
}

/// Opens the app's GitHub releases page, or a specific release page when `tag` names a
/// release CraftHub already knows about. The URL is never taken from the UI.
#[tauri::command]
pub async fn open_releases_page(
    app: AppHandle,
    state: State<'_, AppState>,
    app_id: String,
    tag: Option<String>,
) -> Cmd<()> {
    let e = engine(&state)?;
    let entry = e.catalog().get(&app_id)?.clone();
    let mut url = entry.releases_url();
    if let Some(tag) = tag {
        let known = e.versions(&app_id).await?;
        let rel = known
            .iter()
            .find(|r| r.tag == tag)
            .ok_or_else(|| CoreError::ReleaseNotFound(tag.clone()))?;
        let parsed = url::Url::parse(&rel.html_url)
            .map_err(|_| CoreError::UntrustedUrl("release link".into()))?;
        let expected_prefix = format!("/{}/releases/", entry.repo);
        if parsed.scheme() != "https"
            || parsed.host_str() != Some("github.com")
            || !parsed.path().starts_with(&expected_prefix)
        {
            return Err(CoreError::UntrustedUrl(
                "release link is not on github.com".into(),
            ));
        }
        url = parsed.to_string();
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| CoreError::Launch(e.to_string()))
}

#[tauri::command]
pub async fn open_install_folder(state: State<'_, AppState>, app_id: String) -> Cmd<()> {
    let e = engine(&state)?;
    blocking(move || platform::open_folder(&e.install_dir(&app_id)?)).await
}

#[tauri::command]
pub async fn open_logs_folder(state: State<'_, AppState>) -> Cmd<()> {
    let dir = state.logs_dir.clone();
    blocking(move || {
        std::fs::create_dir_all(&dir).map_err(CoreError::io("creating logs folder"))?;
        platform::open_folder(&dir)
    })
    .await
}

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> Cmd<Settings> {
    engine(&state)?.settings()
}

#[tauri::command]
pub async fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> Cmd<Settings> {
    let e = engine(&state)?;
    e.save_settings(&settings)?;
    notify_changed(&app, &e);
    e.settings()
}

#[tauri::command]
pub async fn get_history(state: State<'_, AppState>, limit: Option<u32>) -> Cmd<Vec<EventRow>> {
    engine(&state)?.events(limit.unwrap_or(100))
}

#[tauri::command]
pub async fn clear_release_cache(app: AppHandle, state: State<'_, AppState>) -> Cmd<()> {
    let e = engine(&state)?;
    e.clear_release_cache()?;
    notify_changed(&app, &e);
    Ok(())
}

/// Opens a native folder picker (in Rust; the UI never supplies a path), validates the
/// choice and makes it the root for new installs. Returns `None` if the user cancelled.
#[tauri::command]
pub async fn choose_install_root(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Cmd<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let e = engine(&state)?;
    let start = e.install_root().ok();
    let picked = blocking(move || {
        let mut dialog = app
            .dialog()
            .file()
            .set_title("Choose where CraftHub installs new apps");
        if let Some(dir) = start {
            dialog = dialog.set_directory(dir);
        }
        Ok(dialog.blocking_pick_folder())
    })
    .await?;
    let Some(picked) = picked else {
        return Ok(None);
    };
    let path = picked
        .into_path()
        .map_err(|_| CoreError::InvalidInput("Choose a folder on this computer.".into()))?;
    let root = blocking(move || e.set_install_root(Some(&path))).await?;
    Ok(Some(root.display().to_string()))
}

#[tauri::command]
pub async fn reset_install_root(state: State<'_, AppState>) -> Cmd<String> {
    let e = engine(&state)?;
    let root = blocking(move || e.set_install_root(None)).await?;
    Ok(root.display().to_string())
}

#[tauri::command]
pub async fn choose_app_install_root(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Cmd<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let e = engine(&state)?;
    let start = e.install_root().ok();
    let picked = blocking(move || {
        let mut dialog = app
            .dialog()
            .file()
            .set_title("Choose this app's installation parent");
        if let Some(dir) = start {
            dialog = dialog.set_directory(dir);
        }
        Ok(dialog.blocking_pick_folder())
    })
    .await?;
    let Some(picked) = picked else {
        return Ok(None);
    };
    let path = picked
        .into_path()
        .map_err(|_| CoreError::InvalidInput("Choose a folder on this computer.".into()))?;
    let root = blocking(move || e.validate_install_root(&path)).await?;
    Ok(Some(root.display().to_string()))
}

#[tauri::command]
pub async fn get_shortcut_status(state: State<'_, AppState>, app_id: String) -> Cmd<ShortcutView> {
    let e = engine(&state)?;
    let view = e.app_view(&app_id)?;
    let installed = view.installed.ok_or(CoreError::NotInstalled(view.name))?;
    Ok(ShortcutView {
        path: installed.shortcut_path,
        exists: installed.shortcut_exists,
    })
}

#[tauri::command]
pub async fn create_shortcut(
    app: AppHandle,
    state: State<'_, AppState>,
    app_id: String,
) -> Cmd<ShortcutView> {
    let e = engine(&state)?;
    let result = blocking({
        let e = e.clone();
        move || e.create_shortcut(&app_id)
    })
    .await;
    notify_changed(&app, &e);
    result
}

#[tauri::command]
pub async fn remove_shortcut(
    app: AppHandle,
    state: State<'_, AppState>,
    app_id: String,
) -> Cmd<ShortcutView> {
    let e = engine(&state)?;
    let result = blocking({
        let e = e.clone();
        move || e.remove_shortcut(&app_id)
    })
    .await;
    notify_changed(&app, &e);
    result
}

/// Exit requested by the user after confirming that running installs may be cancelled.
#[tauri::command]
pub async fn exit_app(app: AppHandle) -> Cmd<()> {
    crate::desktop::exit_after_cancelling(app).await;
    Ok(())
}

#[tauri::command]
pub async fn get_self_update_status(app: AppHandle) -> Cmd<crate::selfupdate::SelfUpdateStatus> {
    Ok(crate::selfupdate::status(&app))
}

#[tauri::command]
pub async fn check_self_update(app: AppHandle) -> Cmd<Option<crate::selfupdate::SelfUpdateInfo>> {
    crate::selfupdate::check(&app)
        .await
        .map_err(CoreError::Unsupported)
}

#[tauri::command]
pub async fn install_self_update(app: AppHandle, state: State<'_, AppState>) -> Cmd<()> {
    if !engine(&state)?.active_operations().is_empty() {
        return Err(CoreError::Busy(
            "CraftHub (wait for running installs to finish first)".into(),
        ));
    }
    crate::selfupdate::install(&app)
        .await
        .map_err(CoreError::Unsupported)
}
