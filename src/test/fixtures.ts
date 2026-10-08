import { vi } from "vitest";
import type { Api } from "../api";
import type {
  ActiveOperation,
  AppView,
  NavigateRequest,
  ProgressEvent,
  ReleaseView,
  Settings,
  UpdateAllSummary,
} from "../types";

export const DEFAULT_SETTINGS: Settings = {
  channel: "stable",
  checkOnStartup: true,
  checkIntervalHours: 6,
  updateMode: "notify",
  notifications: true,
  minimizeToTray: true,
  installRoot: null,
  createShortcuts: true,
};

const IDS: Array<[string, string]> = [
  ["photocraft", "PhotoCraft"],
  ["vectorcraft", "VectorCraft"],
  ["filmcraft", "FilmCraft"],
  ["lightcraft", "LightCraft"],
  ["pdfcraft", "PDFCraft"],
  ["effectcraft", "EffectCraft"],
  ["designcraft", "DesignCraft"],
  ["soundcraft", "SoundCraft"],
  ["wordcraft", "WordCraft"],
  ["gridcraft", "GridCraft"],
  ["deckcraft", "DeckCraft"],
  ["cadcraft", "CADCraft"],
];

export function release(version: string): ReleaseView {
  return {
    tag: `v${version}`,
    version,
    name: `v${version}`,
    publishedAt: "2026-10-07T10:33:19Z",
    notes: "Notes <script>alert(1)</script>",
    htmlUrl: `https://github.com/storytold/x/releases/tag/v${version}`,
    prerelease: false,
    installable: true,
    asset: {
      name: `x-${version}-windows-x64-portable.zip`,
      url: "",
      size: 65251740,
      sha256: "ab".repeat(32),
      hasChecksumFile: true,
    },
    unavailableReason: null,
  };
}

export function app(id: string, name: string, patch: Partial<AppView> = {}): AppView {
  return {
    id,
    name,
    description: `${name} description`,
    category: "craft-suite",
    repo: `storytold/${id}`,
    repoUrl: `https://github.com/storytold/${id}`,
    releasesUrl: `https://github.com/storytold/${id}/releases`,
    supported: true,
    unsupportedReason: null,
    audit: null,
    status: "notInstalled",
    installed: null,
    latest: release("1.0.0"),
    newestUnavailable: null,
    updateAvailable: false,
    check: { checkedAt: 1_790_000_000, source: "network", warning: null, error: null },
    running: false,
    busy: false,
    ...patch,
  };
}

export function catalogFixture(): AppView[] {
  const list = IDS.map(([id, name]) => app(id, name));
  const sound = list.find((a) => a.id === "soundcraft")!;
  Object.assign(sound, {
    status: "unavailable",
    latest: null,
    unsupportedReason: "No releases have been published yet.",
  });
  list.push(
    app("artcraft", "ArtCraft", {
      category: "ai-studio",
      supported: false,
      status: "unavailable",
      latest: null,
      unsupportedReason: "Windows releases ship only an installer.",
    }),
  );
  return list;
}

export function fakeApi(apps: AppView[]) {
  let progressCb: ((e: ProgressEvent) => void) | null = null;
  let navigateCb: ((n: NavigateRequest) => void) | null = null;
  let exitCb: ((ops: ActiveOperation[]) => void) | null = null;
  const api = {
    getEnvironment: vi.fn(async () => ({
      version: "0.1.0",
      platformSupported: true,
      defaultInstallRoot: "C:\\Apps",
      installRoot: "C:\\Apps",
      dataDir: "C:\\Data",
      logsDir: "C:\\Data\\logs",
      engineError: null,
    })),
    listApps: vi.fn(async () => apps),
    checkForUpdates: vi.fn(async () => apps),
    getVersions: vi.fn(async () => [release("1.0.0")]),
    installApp: vi.fn(async (appId: string) => ({
      appId,
      opId: "op1",
      version: "1.0.0",
      previousVersion: null,
      verification: "github-digest",
      path: "C:\\Apps\\x",
      warnings: [],
    })),
    updateApp: vi.fn(async (appId: string) => ({
      appId,
      opId: "op2",
      version: "2.0.0",
      previousVersion: "1.0.0",
      verification: "github-digest",
      path: "C:\\Apps\\x",
      warnings: [],
    })),
    updateAll: vi.fn(async (): Promise<UpdateAllSummary> => ({
      updated: [],
      skipped: [],
      failed: [],
    })),
    uninstallApp: vi.fn(async (appId: string) => ({
      appId,
      removedFiles: 3,
      keptEntries: [],
      keptPath: null,
    })),
    rollbackApp: vi.fn(),
    launchApp: vi.fn(async () => 1234),
    cancelOperation: vi.fn(async () => true),
    openReleasesPage: vi.fn(async () => undefined),
    openInstallFolder: vi.fn(async () => undefined),
    openLogsFolder: vi.fn(async () => undefined),
    getSettings: vi.fn(async (): Promise<Settings> => ({ ...DEFAULT_SETTINGS })),
    saveSettings: vi.fn(async (s: Settings) => s),
    getHistory: vi.fn(async () => []),
    clearReleaseCache: vi.fn(async () => undefined),
    onProgress: vi.fn(async (cb: (e: ProgressEvent) => void) => {
      progressCb = cb;
      return () => {
        progressCb = null;
      };
    }),
    onAppsChanged: vi.fn(async () => () => {}),
    chooseInstallRoot: vi.fn(async (): Promise<string | null> => "D:\\CraftApps"),
    resetInstallRoot: vi.fn(async () => "C:\\Apps"),
    chooseAppInstallRoot: vi.fn(async (): Promise<string | null> => "C:\\Apps"),
    getShortcutStatus: vi.fn(async () => ({ path: null, exists: false })),
    createShortcut: vi.fn(async () => ({ path: "C:\\Desktop\\Craft.lnk", exists: true })),
    removeShortcut: vi.fn(async () => ({ path: null, exists: false })),
    exitApp: vi.fn(async () => undefined),
    getSelfUpdateStatus: vi.fn(async () => ({
      configured: false,
      currentVersion: "0.1.0",
      reason: "This build has no update-signing key.",
    })),
    checkSelfUpdate: vi.fn(async () => null),
    installSelfUpdate: vi.fn(async () => undefined),
    onNavigate: vi.fn(async (cb: (n: NavigateRequest) => void) => {
      navigateCb = cb;
      return () => {};
    }),
    onConfirmExit: vi.fn(async (cb: (ops: ActiveOperation[]) => void) => {
      exitCb = cb;
      return () => {};
    }),
    onAutoUpdate: vi.fn(async () => () => {}),
  };
  return {
    api: api as unknown as Api & typeof api,
    emitProgress: (e: ProgressEvent) => progressCb?.(e),
    emitNavigate: (n: NavigateRequest) => navigateCb?.(n),
    emitConfirmExit: (ops: ActiveOperation[]) => exitCb?.(ops),
  };
}
