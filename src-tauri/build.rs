// Every IPC command is declared here so that it gets an explicit permission; the
// capability file then grants exactly this list to the main window and nothing else.
const COMMANDS: &[&str] = &[
    "get_environment",
    "list_apps",
    "check_for_updates",
    "get_versions",
    "install_app",
    "update_app",
    "update_all",
    "uninstall_app",
    "rollback_app",
    "launch_app",
    "cancel_operation",
    "open_releases_page",
    "open_install_folder",
    "open_logs_folder",
    "get_settings",
    "save_settings",
    "get_history",
    "clear_release_cache",
    "choose_install_root",
    "reset_install_root",
    "choose_app_install_root",
    "get_shortcut_status",
    "create_shortcut",
    "remove_shortcut",
    "exit_app",
    "get_self_update_status",
    "check_self_update",
    "install_self_update",
];

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("failed to run tauri-build");
}
