//! Tauri shell: a thin, typed IPC layer over `crafthub_core::Engine`. The web view can
//! only call the commands below (see capabilities/default.json); it never receives
//! filesystem, shell, HTTP or URL-opening capabilities of its own.

mod commands;
mod desktop;
mod selfupdate;

use std::path::PathBuf;
use std::sync::Arc;

use crafthub_core::{Engine, default_data_dir, production_config};
use tauri::Manager;

pub const EVENT_PROGRESS: &str = "crafthub://progress";
pub const EVENT_APPS_CHANGED: &str = "crafthub://apps-changed";

pub struct AppState {
    pub engine: Result<Arc<Engine>, String>,
    pub data_dir: PathBuf,
    pub logs_dir: PathBuf,
    _log_guard: Option<tracing_appender::non_blocking::WorkerGuard>,
}

fn init_logging(logs_dir: &std::path::Path) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    use tracing_subscriber::EnvFilter;
    std::fs::create_dir_all(logs_dir).ok()?;
    let appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("crafthub")
        .filename_suffix("log")
        .max_log_files(7)
        .build(logs_dir)
        .ok()?;
    let (writer, guard) = tracing_appender::non_blocking(appender);
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("CRAFTHUB_LOG").unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(writer)
        .with_ansi(false)
        .try_init()
        .ok()?;
    Some(guard)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default()
        // Must be first: a second CraftHub instance would race on the same managed folders.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            desktop::show_main(app, None);
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init());
    // The updater is only registered in builds that carry an update-signing public key.
    // Registered only when the self-update gate passes (final identity, public key, endpoint).
    let context = tauri::generate_context!();
    if let Some(key) = selfupdate::pubkey_for(&context.config().identifier) {
        builder = builder.plugin(tauri_plugin_updater::Builder::new().pubkey(key).build());
    }
    builder
        .on_window_event(desktop::on_window_event)
        .setup(|app| {
            let data_dir = default_data_dir()
                .or_else(|| app.path().app_local_data_dir().ok())
                .ok_or("cannot determine the data folder")?;
            let logs_dir = data_dir.join("logs");
            let guard = init_logging(&logs_dir);
            tracing::info!(version = env!("CARGO_PKG_VERSION"), "CraftHub starting");

            let engine = production_config(&data_dir)
                .and_then(Engine::new)
                .map(Arc::new)
                .map_err(|e| {
                    tracing::error!(error = %e, "engine failed to start");
                    e.to_string()
                });
            if let Ok(e) = &engine {
                desktop::spawn_update_checker(app.handle().clone(), e.clone());
            }
            app.manage(AppState {
                engine,
                data_dir,
                logs_dir,
                _log_guard: guard,
            });
            if let Err(e) = desktop::build_tray(app.handle()) {
                tracing::warn!(error = %e, "tray icon unavailable");
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_environment,
            commands::list_apps,
            commands::check_for_updates,
            commands::get_versions,
            commands::install_app,
            commands::update_app,
            commands::update_all,
            commands::uninstall_app,
            commands::rollback_app,
            commands::launch_app,
            commands::cancel_operation,
            commands::open_releases_page,
            commands::open_install_folder,
            commands::open_logs_folder,
            commands::get_settings,
            commands::save_settings,
            commands::get_history,
            commands::clear_release_cache,
            commands::choose_install_root,
            commands::reset_install_root,
            commands::choose_app_install_root,
            commands::get_shortcut_status,
            commands::create_shortcut,
            commands::remove_shortcut,
            commands::exit_app,
            commands::get_self_update_status,
            commands::check_self_update,
            commands::install_self_update,
        ])
        .run(context)
        .expect("error while running CraftHub");
}
