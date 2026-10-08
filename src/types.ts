// Mirrors the serde (camelCase) shapes in crates/crafthub-core and src-tauri.

export type AppStatus = "unavailable" | "notInstalled" | "installed" | "updateAvailable";
export type FetchSource = "network" | "notModified" | "staleCache";
export type Channel = "stable" | "beta";

export interface ResolvedAsset {
  name: string;
  url: string;
  size: number;
  sha256: string | null;
  hasChecksumFile: boolean;
}

export interface ReleaseView {
  tag: string;
  version: string;
  name: string | null;
  publishedAt: string | null;
  notes: string | null;
  htmlUrl: string;
  prerelease: boolean;
  installable: boolean;
  asset: ResolvedAsset | null;
  unavailableReason: string | null;
}

export interface InstalledView {
  version: string;
  tag: string;
  path: string;
  executable: string;
  installedAt: number;
  sha256: string;
  verification: string;
  previousVersion: string | null;
  shortcutPath?: string | null;
  shortcutExists?: boolean;
}

export interface AuditInfo {
  date: string;
  version: string;
  reference: string;
}

export interface ReleaseCheck {
  checkedAt: number | null;
  source: FetchSource | null;
  warning: string | null;
  error: string | null;
}

export interface AppView {
  id: string;
  name: string;
  description: string;
  category: "craft-suite" | "ai-studio";
  repo: string;
  repoUrl: string;
  releasesUrl: string;
  supported: boolean;
  unsupportedReason: string | null;
  audit: AuditInfo | null;
  status: AppStatus;
  installed: InstalledView | null;
  latest: ReleaseView | null;
  newestUnavailable: ReleaseView | null;
  updateAvailable: boolean;
  check: ReleaseCheck;
  running: boolean;
  busy: boolean;
}

export type Phase =
  | "resolving"
  | "downloading"
  | "verifying"
  | "extracting"
  | "activating"
  | "cleaningUp"
  | "completed"
  | "failed"
  | "cancelled";

export interface ProgressEvent {
  opId: string;
  appId: string;
  kind: "install" | "update";
  phase: Phase;
  done: number;
  total: number;
  message: string | null;
}

export interface InstallOutcome {
  appId: string;
  opId: string;
  version: string;
  previousVersion: string | null;
  verification: string;
  path: string;
  warnings: string[];
}

export interface ShortcutView {
  path: string | null;
  exists: boolean;
}

export interface UninstallOutcome {
  appId: string;
  removedFiles: number;
  keptEntries: string[];
  keptPath: string | null;
}

export interface UpdateAllItem {
  appId: string;
  name: string;
  from: string | null;
  to: string | null;
  reason: string | null;
}

export interface UpdateAllSummary {
  updated: UpdateAllItem[];
  skipped: UpdateAllItem[];
  failed: UpdateAllItem[];
}

export type UpdateMode = "manual" | "notify" | "automatic";

export interface Settings {
  channel: Channel;
  checkOnStartup: boolean;
  checkIntervalHours: number;
  updateMode: UpdateMode;
  notifications: boolean;
  minimizeToTray: boolean;
  /** Read-only from the UI; changed only via chooseInstallRoot/resetInstallRoot. */
  installRoot: string | null;
  createShortcuts?: boolean;
}

export interface SelfUpdateStatus {
  configured: boolean;
  currentVersion: string;
  reason: string | null;
}

export interface SelfUpdateInfo {
  version: string;
  notes: string | null;
  date: string | null;
}

export interface ActiveOperation {
  opId: string;
  appId: string;
}

export interface NavigateRequest {
  view: "home" | "all" | "installed" | "updates" | "settings" | "app";
  appId: string | null;
}

export interface Environment {
  version: string;
  platformSupported: boolean;
  defaultInstallRoot: string | null;
  installRoot: string | null;
  dataDir: string;
  logsDir: string;
  engineError: string | null;
}

export interface EventRow {
  id: number;
  appId: string;
  kind: string;
  version: string | null;
  outcome: string;
  message: string | null;
  at: number;
}

export interface ErrorPayload {
  kind: string;
  message: string;
}
