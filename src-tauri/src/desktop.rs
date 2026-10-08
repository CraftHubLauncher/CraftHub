//! Windows desktop integration: notification-area (tray) icon, close-to-tray behaviour,
//! native update notifications, and the background update-check loop.

use std::sync::Arc;
use std::time::Duration;

use crafthub_core::Engine;
use crafthub_core::engine::UpdateNotice;
use crafthub_core::registry::UpdateMode;
use crafthub_core::util::now_unix;
use serde::Serialize;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, WindowEvent};

use crate::{AppState, EVENT_APPS_CHANGED, EVENT_PROGRESS};

pub const EVENT_NAVIGATE: &str = "crafthub://navigate";
pub const EVENT_CONFIRM_EXIT: &str = "crafthub://confirm-exit";
pub const EVENT_AUTO_UPDATE: &str = "crafthub://auto-update";

/// Where the UI should go when the window is brought up (e.g. from a notification).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Navigate {
    pub view: &'static str,
    pub app_id: Option<String>,
}

fn engine(app: &AppHandle) -> Option<Arc<Engine>> {
    app.try_state::<AppState>()
        .and_then(|s| s.engine.as_ref().ok().cloned())
}

pub fn show_main(app: &AppHandle, nav: Option<Navigate>) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
    if let Some(n) = nav {
        let _ = app.emit(EVENT_NAVIGATE, n);
    }
}

// ---------------------------------------------------------------- tray

pub fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open CraftHub", true, None::<&str>)?;
    let check = MenuItem::with_id(app, "check", "Check for updates", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit CraftHub", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&open, &check, &sep, &quit])?;

    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("CraftHub")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main(app, None),
            "check" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move { tray_check(&app).await });
            }
            "quit" => request_exit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle(), None);
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

/// Manual check from the tray: always reports the result.
async fn tray_check(app: &AppHandle) {
    let Some(e) = engine(app) else { return };
    match e.check_for_updates(None).await {
        Ok(views) => {
            let n = views.iter().filter(|v| v.update_available).count();
            let _ = app.emit(EVENT_APPS_CHANGED, views);
            let notices = e.take_new_update_notices().unwrap_or_default();
            if n == 0 {
                notify(app, "CraftHub", "All installed apps are up to date.", None);
            } else if !notices.is_empty() {
                announce(app, &notices);
            } else {
                notify(
                    app,
                    "CraftHub",
                    &format!("{n} update{} available.", if n == 1 { "" } else { "s" }),
                    Some(Navigate {
                        view: "updates",
                        app_id: None,
                    }),
                );
            }
        }
        Err(err) => notify(
            app,
            "CraftHub could not check for updates",
            &err.to_string(),
            None,
        ),
    }
}

// ---------------------------------------------------------------- close / exit

/// Handles the window close button. With minimize-to-tray enabled, or whenever an
/// install/update is running, the window hides instead of quitting so no operation is
/// cut off. Otherwise CraftHub exits normally.
pub fn on_window_event(window: &tauri::Window, event: &WindowEvent) {
    let WindowEvent::CloseRequested { api, .. } = event else {
        return;
    };
    let app = window.app_handle();
    let Some(e) = engine(app) else { return };
    let to_tray = e.settings().map(|s| s.minimize_to_tray).unwrap_or(true);
    let busy = !e.active_operations().is_empty();
    if to_tray || busy {
        api.prevent_close();
        let _ = window.hide();
        if busy && !to_tray {
            notify(
                app,
                "CraftHub is still working",
                "An install or update is in progress. CraftHub will keep running in the notification area until it finishes.",
                None,
            );
        }
    }
}

/// Quit from the tray. If work is in progress the UI is asked to confirm first; nothing
/// is ever interrupted silently.
pub fn request_exit(app: &AppHandle) {
    match engine(app) {
        Some(e) if !e.active_operations().is_empty() => {
            show_main(app, None);
            let _ = app.emit(EVENT_CONFIRM_EXIT, e.active_operations());
        }
        _ => app.exit(0),
    }
}

/// Cancels running operations (each rolls back), waits for them to finish, then exits.
pub async fn exit_after_cancelling(app: AppHandle) {
    if let Some(e) = engine(&app) {
        e.cancel_all();
        for _ in 0..300 {
            if e.active_operations().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    app.exit(0);
}

// ---------------------------------------------------------------- notifications

/// Windows identifies the sender of a toast by AppUserModelID. The NSIS installer
/// registers CraftHub's identifier via its Start menu shortcut; uninstalled builds fall
/// back to PowerShell's ID so notifications still appear during development.
#[cfg(windows)]
fn toast_app_id(app: &AppHandle) -> String {
    let installed = std::env::var_os("APPDATA")
        .map(|a| {
            std::path::Path::new(&a)
                .join(r"Microsoft\Windows\Start Menu\Programs\CraftHub.lnk")
                .exists()
        })
        .unwrap_or(false);
    if installed && !cfg!(debug_assertions) {
        app.config().identifier.clone()
    } else {
        tauri_winrt_notification::Toast::POWERSHELL_APP_ID.to_string()
    }
}

pub fn notify(app: &AppHandle, title: &str, body: &str, nav: Option<Navigate>) {
    #[cfg(windows)]
    {
        use tauri_winrt_notification::Toast;
        let handle = app.clone();
        let result = Toast::new(&toast_app_id(app))
            .title(title)
            .text1(body)
            .on_activated(move |_| {
                show_main(&handle, nav.clone());
                Ok(())
            })
            .show();
        if let Err(e) = result {
            tracing::warn!(error = %e, "could not show notification");
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (app, title, body, nav);
    }
}

fn announce(app: &AppHandle, notices: &[UpdateNotice]) {
    match notices {
        [] => {}
        [one] => notify(
            app,
            &format!("{} {} is available", one.name, one.version),
            "Open CraftHub to review the release notes and update.",
            Some(Navigate {
                view: "app",
                app_id: Some(one.app_id.clone()),
            }),
        ),
        many => notify(
            app,
            &format!("{} updates are available", many.len()),
            &many
                .iter()
                .map(|n| format!("{} {}", n.name, n.version))
                .collect::<Vec<_>>()
                .join(", "),
            Some(Navigate {
                view: "updates",
                app_id: None,
            }),
        ),
    }
}

// ---------------------------------------------------------------- background checks

/// Startup check (optional) plus periodic checks in Notify/Automatic modes. Automatic
/// mode installs verified updates only for apps that are not running; running apps are
/// skipped and retried on the next cycle. Rate limits and offline periods surface as
/// stale-cache warnings inside the engine and simply wait for the next interval.
pub fn spawn_update_checker(app: AppHandle, engine: Arc<Engine>) {
    tauri::async_runtime::spawn(async move {
        let mut last_check: Option<i64> = None;
        if engine
            .settings()
            .map(|s| s.check_on_startup)
            .unwrap_or(false)
        {
            tokio::time::sleep(Duration::from_secs(3)).await;
            run_cycle(&app, &engine).await;
            last_check = Some(now_unix());
        }
        loop {
            tokio::time::sleep(Duration::from_secs(60)).await;
            let Ok(s) = engine.settings() else { continue };
            if s.update_mode == UpdateMode::Manual || s.check_interval_hours == 0 {
                continue;
            }
            let due = last_check
                .is_none_or(|t| now_unix() - t >= i64::from(s.check_interval_hours) * 3600);
            if due {
                last_check = Some(now_unix());
                run_cycle(&app, &engine).await;
            }
        }
    });
}

async fn run_cycle(app: &AppHandle, engine: &Arc<Engine>) {
    let Ok(settings) = engine.settings() else {
        return;
    };
    match engine.check_for_updates(None).await {
        Ok(views) => {
            let _ = app.emit(EVENT_APPS_CHANGED, views);
        }
        Err(e) => {
            tracing::warn!(error = %e, "background check failed");
            return;
        }
    }

    if settings.update_mode == UpdateMode::Automatic {
        let handle = app.clone();
        let sink: crafthub_core::ProgressSink = Arc::new(move |ev| {
            let _ = handle.emit(EVENT_PROGRESS, ev);
        });
        match engine.update_all(sink).await {
            Ok(summary) => {
                if let Ok(views) = engine.list_apps() {
                    let _ = app.emit(EVENT_APPS_CHANGED, views);
                }
                let _ = app.emit(EVENT_AUTO_UPDATE, &summary);
                if settings.notifications && !summary.updated.is_empty() {
                    let list = summary
                        .updated
                        .iter()
                        .map(|i| format!("{} {}", i.name, i.to.clone().unwrap_or_default()))
                        .collect::<Vec<_>>()
                        .join(", ");
                    notify(
                        app,
                        "CraftHub installed updates",
                        &format!("{list}. The previous versions are kept for rollback."),
                        Some(Navigate {
                            view: "installed",
                            app_id: None,
                        }),
                    );
                }
            }
            Err(e) => tracing::warn!(error = %e, "automatic update failed"),
        }
    }

    // Whatever is still pending (manual approval, or skipped because the app is open).
    if settings.update_mode != UpdateMode::Manual && settings.notifications {
        match engine.take_new_update_notices() {
            Ok(n) => announce(app, &n),
            Err(e) => tracing::warn!(error = %e, "could not compute update notices"),
        }
    }
}
