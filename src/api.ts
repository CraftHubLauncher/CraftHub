// Typed wrappers for every Rust command. The UI never builds URLs or paths itself.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  ActiveOperation,
  AppView,
  Environment,
  ErrorPayload,
  EventRow,
  InstalledView,
  InstallOutcome,
  NavigateRequest,
  ProgressEvent,
  ReleaseView,
  SelfUpdateInfo,
  SelfUpdateStatus,
  Settings,
  ShortcutView,
  UninstallOutcome,
  UpdateAllSummary,
} from "./types";

export const EVENT_PROGRESS = "crafthub://progress";
export const EVENT_APPS_CHANGED = "crafthub://apps-changed";
export const EVENT_NAVIGATE = "crafthub://navigate";
export const EVENT_CONFIRM_EXIT = "crafthub://confirm-exit";
export const EVENT_AUTO_UPDATE = "crafthub://auto-update";

export class CommandError extends Error {
  readonly kind: string;
  constructor(payload: ErrorPayload) {
    super(payload.message);
    this.kind = payload.kind;
  }
}

function toError(e: unknown): CommandError {
  if (e && typeof e === "object" && "message" in e && "kind" in e) {
    return new CommandError(e as ErrorPayload);
  }
  return new CommandError({ kind: "unknown", message: String(e) });
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    throw toError(e);
  }
}

export const api = {
  getEnvironment: () => call<Environment>("get_environment"),
  listApps: () => call<AppView[]>("list_apps"),
  checkForUpdates: (appId?: string) =>
    call<AppView[]>("check_for_updates", { appId: appId ?? null }),
  getVersions: (appId: string) => call<ReleaseView[]>("get_versions", { appId }),
  installApp: (appId: string, version?: string, installRoot?: string, createShortcut = true) =>
    call<InstallOutcome>("install_app", {
      appId,
      version: version ?? null,
      installRoot: installRoot ?? null,
      createShortcut,
    }),
  updateApp: (appId: string, createShortcut = false) =>
    call<InstallOutcome>("update_app", { appId, createShortcut }),
  updateAll: () => call<UpdateAllSummary>("update_all"),
  uninstallApp: (appId: string) => call<UninstallOutcome>("uninstall_app", { appId }),
  rollbackApp: (appId: string) => call<InstalledView>("rollback_app", { appId }),
  launchApp: (appId: string) => call<number>("launch_app", { appId }),
  cancelOperation: (opId: string) => call<boolean>("cancel_operation", { opId }),
  openReleasesPage: (appId: string, tag?: string) =>
    call<void>("open_releases_page", { appId, tag: tag ?? null }),
  openInstallFolder: (appId: string) => call<void>("open_install_folder", { appId }),
  openLogsFolder: () => call<void>("open_logs_folder"),
  getSettings: () => call<Settings>("get_settings"),
  saveSettings: (settings: Settings) => call<Settings>("save_settings", { settings }),
  getHistory: (limit?: number) => call<EventRow[]>("get_history", { limit: limit ?? null }),
  clearReleaseCache: () => call<void>("clear_release_cache"),
  chooseInstallRoot: () => call<string | null>("choose_install_root"),
  resetInstallRoot: () => call<string>("reset_install_root"),
  chooseAppInstallRoot: () => call<string | null>("choose_app_install_root"),
  getShortcutStatus: (appId: string) => call<ShortcutView>("get_shortcut_status", { appId }),
  createShortcut: (appId: string) => call<ShortcutView>("create_shortcut", { appId }),
  removeShortcut: (appId: string) => call<ShortcutView>("remove_shortcut", { appId }),
  exitApp: () => call<void>("exit_app"),
  getSelfUpdateStatus: () => call<SelfUpdateStatus>("get_self_update_status"),
  checkSelfUpdate: () => call<SelfUpdateInfo | null>("check_self_update"),
  installSelfUpdate: () => call<void>("install_self_update"),
  onNavigate: (cb: (n: NavigateRequest) => void): Promise<UnlistenFn> =>
    listen<NavigateRequest>(EVENT_NAVIGATE, (e) => cb(e.payload)),
  onConfirmExit: (cb: (ops: ActiveOperation[]) => void): Promise<UnlistenFn> =>
    listen<ActiveOperation[]>(EVENT_CONFIRM_EXIT, (e) => cb(e.payload)),
  onAutoUpdate: (cb: (s: UpdateAllSummary) => void): Promise<UnlistenFn> =>
    listen<UpdateAllSummary>(EVENT_AUTO_UPDATE, (e) => cb(e.payload)),
  onProgress: (cb: (e: ProgressEvent) => void): Promise<UnlistenFn> =>
    listen<ProgressEvent>(EVENT_PROGRESS, (e) => cb(e.payload)),
  onAppsChanged: (cb: (apps: AppView[]) => void): Promise<UnlistenFn> =>
    listen<AppView[]>(EVENT_APPS_CHANGED, (e) => cb(e.payload)),
};

export type Api = typeof api;
