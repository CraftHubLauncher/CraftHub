import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { api as realApi, CommandError, type Api } from "./api";
import { isActivePhase } from "./format";

export type ViewId = "home" | "all" | "installed" | "updates" | "settings";
import type {
  ActiveOperation,
  AppView,
  Environment,
  ProgressEvent,
  Settings,
  UpdateAllSummary,
} from "./types";

export interface Toast {
  id: number;
  kind: "error" | "success" | "info";
  text: string;
}

/** Last failed install/update per app, so the card can offer Retry. */
export interface Failure {
  message: string;
  kind: string;
  action: "install" | "update";
  version?: string;
}

export interface InstallDialogState {
  appId: string;
  version?: string;
  parent: string;
  createShortcut: boolean;
  update: boolean;
}

export interface Store {
  api: Api;
  env: Environment | null;
  settings: Settings | null;
  apps: AppView[];
  loaded: boolean;
  checking: boolean;
  updatingAll: boolean;
  ops: Record<string, ProgressEvent>;
  failures: Record<string, Failure>;
  toasts: Toast[];
  updateAllSummary: UpdateAllSummary | null;
  view: ViewId;
  detailsId: string | null;
  setView: (v: ViewId) => void;
  setDetailsId: (id: string | null) => void;
  exitRequest: ActiveOperation[] | null;
  refresh: () => Promise<void>;
  refreshEnvironment: () => Promise<void>;
  checkForUpdates: () => Promise<void>;
  install: (appId: string, version?: string) => Promise<void>;
  update: (appId: string, createShortcut?: boolean) => Promise<void>;
  retry: (appId: string) => Promise<void>;
  dismissFailure: (appId: string) => void;
  updateAll: () => Promise<void>;
  uninstall: (appId: string) => Promise<void>;
  rollback: (appId: string) => Promise<void>;
  launch: (appId: string) => Promise<void>;
  cancel: (appId: string) => Promise<void>;
  openReleases: (appId: string, tag?: string) => Promise<void>;
  openFolder: (appId: string) => Promise<void>;
  saveSettings: (s: Settings) => Promise<void>;
  dismissToast: (id: number) => void;
  clearUpdateAllSummary: () => void;
  answerExit: (exit: boolean) => Promise<void>;
  notify: (kind: Toast["kind"], text: string) => void;
  installDialog: InstallDialogState | null;
  requestInstall: (appId: string, version?: string) => Promise<void>;
  requestUpdate: (appId: string) => Promise<void>;
  browseInstallRoot: () => Promise<void>;
  confirmInstall: () => Promise<void>;
  cancelInstallDialog: () => void;
  updateInstallDialog: (patch: Partial<InstallDialogState>) => void;
  createShortcut: (appId: string) => Promise<void>;
  removeShortcut: (appId: string) => Promise<void>;
}

const Ctx = createContext<Store | null>(null);

export function useStore(): Store {
  const s = useContext(Ctx);
  if (!s) throw new Error("useStore must be used inside <StoreProvider>");
  return s;
}

function messageOf(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

function kindOf(e: unknown): string {
  return e instanceof CommandError ? e.kind : "unknown";
}

export function StoreProvider({ api = realApi, children }: { api?: Api; children: ReactNode }) {
  const [env, setEnv] = useState<Environment | null>(null);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [apps, setApps] = useState<AppView[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [checking, setChecking] = useState(false);
  const [updatingAll, setUpdatingAll] = useState(false);
  const [ops, setOps] = useState<Record<string, ProgressEvent>>({});
  const [failures, setFailures] = useState<Record<string, Failure>>({});
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [updateAllSummary, setUpdateAllSummary] = useState<UpdateAllSummary | null>(null);
  const [view, setView] = useState<ViewId>("home");
  const [detailsId, setDetailsId] = useState<string | null>(null);
  const [exitRequest, setExitRequest] = useState<ActiveOperation[] | null>(null);
  const [installDialog, setInstallDialog] = useState<InstallDialogState | null>(null);
  const toastId = useRef(0);

  const notify = useCallback((kind: Toast["kind"], text: string) => {
    const id = ++toastId.current;
    setToasts((t) => [...t.slice(-4), { id, kind, text }]);
    if (kind !== "error") {
      setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), 6000);
    }
  }, []);

  const dismissToast = useCallback((id: number) => {
    setToasts((t) => t.filter((x) => x.id !== id));
  }, []);

  const refresh = useCallback(async () => {
    try {
      setApps(await api.listApps());
    } catch (e) {
      notify("error", messageOf(e));
    } finally {
      setLoaded(true);
    }
  }, [api, notify]);

  const refreshEnvironment = useCallback(async () => {
    try {
      const [environment, s] = await Promise.all([api.getEnvironment(), api.getSettings()]);
      setEnv(environment);
      setSettings(s);
    } catch (e) {
      notify("error", messageOf(e));
    }
  }, [api, notify]);

  useEffect(() => {
    let disposed = false;
    const unlisteners: Array<() => void> = [];
    (async () => {
      await refreshEnvironment();
      await refresh();
      const subs = await Promise.all([
        api.onProgress((ev) => {
          setOps((o) => ({ ...o, [ev.appId]: ev }));
          if (!isActivePhase(ev.phase)) {
            setTimeout(
              () =>
                setOps((o) => {
                  if (o[ev.appId]?.opId !== ev.opId) return o;
                  const next = { ...o };
                  delete next[ev.appId];
                  return next;
                }),
              1500,
            );
          }
        }),
        api.onAppsChanged((list) => setApps(list)),
        api.onNavigate((n) => {
          // Notification/tray click: go to the page, or open one app's details.
          if (n.view === "app") {
            setView("updates");
            setDetailsId(n.appId);
          } else {
            setView(n.view);
          }
        }),
        api.onConfirmExit((active) => setExitRequest(active)),
        api.onAutoUpdate((summary) => {
          if (summary.updated.length + summary.failed.length > 0) setUpdateAllSummary(summary);
        }),
      ]);
      if (disposed) subs.forEach((u) => u());
      else unlisteners.push(...subs);
    })();
    return () => {
      disposed = true;
      unlisteners.forEach((u) => u());
    };
  }, [api, refresh, refreshEnvironment]);

  const nameOf = useCallback(
    (appId: string) => apps.find((a) => a.id === appId)?.name ?? appId,
    [apps],
  );

  const checkForUpdates = useCallback(async () => {
    setChecking(true);
    try {
      const list = await api.checkForUpdates();
      setApps(list);
      const n = list.filter((a) => a.updateAvailable).length;
      const offline = list.some((a) => a.check.error || a.check.warning);
      notify(
        offline ? "error" : "info",
        offline
          ? "Some release information could not be refreshed. Showing what CraftHub last saw."
          : n > 0
            ? `${n} update${n === 1 ? "" : "s"} available.`
            : "All installed apps are up to date.",
      );
    } catch (e) {
      notify("error", messageOf(e));
    } finally {
      setChecking(false);
    }
  }, [api, notify]);

  const runInstall = useCallback(
    async (
      appId: string,
      failure: Omit<Failure, "message" | "kind">,
      work: () => Promise<{ version: string; warnings: string[] }>,
    ) => {
      setFailures(({ [appId]: _, ...rest }) => rest);
      try {
        const out = await work();
        notify("success", `${nameOf(appId)} ${out.version} installed.`);
        out.warnings.forEach((w) => notify("info", w));
      } catch (e) {
        if (kindOf(e) === "cancelled") {
          notify("info", `${nameOf(appId)}: cancelled. Nothing was changed.`);
        } else {
          setFailures((f) => ({
            ...f,
            [appId]: { ...failure, message: messageOf(e), kind: kindOf(e) },
          }));
          notify("error", `${nameOf(appId)}: ${messageOf(e)}`);
        }
      } finally {
        await refresh();
      }
    },
    [nameOf, notify, refresh],
  );

  const install = useCallback(
    (appId: string, version?: string) =>
      runInstall(appId, { action: "install", version }, () => api.installApp(appId, version)),
    [api, runInstall],
  );

  const update = useCallback(
    (appId: string, createShortcut = false) =>
      runInstall(appId, { action: "update" }, () =>
        createShortcut ? api.updateApp(appId, true) : api.updateApp(appId),
      ),
    [api, runInstall],
  );

  const requestInstall = useCallback(
    async (appId: string, version?: string) => {
      const app = apps.find((a) => a.id === appId);
      if (!app || app.installed) {
        await install(appId, version);
        return;
      }
      const parent = env?.installRoot ?? env?.defaultInstallRoot ?? "";
      setInstallDialog({
        appId,
        version,
        parent,
        createShortcut: settings?.createShortcuts ?? true,
        update: false,
      });
    },
    [apps, env, install, settings],
  );

  const requestUpdate = useCallback(
    async (appId: string) => {
      const app = apps.find((a) => a.id === appId);
      if (!app?.installed || app.installed.shortcutPath) {
        await update(appId);
        return;
      }
      setInstallDialog({
        appId,
        parent: app.installed.path,
        createShortcut: settings?.createShortcuts ?? true,
        update: true,
      });
    },
    [apps, settings, update],
  );

  const browseInstallRoot = useCallback(async () => {
    try {
      const parent = await api.chooseAppInstallRoot();
      if (parent) setInstallDialog((d) => (d ? { ...d, parent } : d));
    } catch (e) {
      notify("error", messageOf(e));
    }
  }, [api, notify]);

  const confirmInstall = useCallback(async () => {
    const d = installDialog;
    if (!d) return;
    setInstallDialog(null);
    if (settings) {
      try {
        setSettings(await api.saveSettings({ ...settings, createShortcuts: d.createShortcut }));
      } catch (e) {
        notify("error", `Could not save shortcut preference: ${messageOf(e)}`);
      }
    }
    if (d.update) await update(d.appId, d.createShortcut);
    else
      await runInstall(d.appId, { action: "install", version: d.version }, () =>
        api.installApp(d.appId, d.version, d.parent || undefined, d.createShortcut),
      );
  }, [api, installDialog, notify, runInstall, settings, update]);

  const retry = useCallback(
    async (appId: string) => {
      const f = failures[appId];
      if (!f) return;
      if (f.action === "update") await update(appId);
      else await install(appId, f.version);
    },
    [failures, install, update],
  );

  const dismissFailure = useCallback((appId: string) => {
    setFailures(({ [appId]: _, ...rest }) => rest);
  }, []);

  const updateAll = useCallback(async () => {
    setUpdatingAll(true);
    try {
      setUpdateAllSummary(await api.updateAll());
    } catch (e) {
      notify("error", messageOf(e));
    } finally {
      setUpdatingAll(false);
      await refresh();
    }
  }, [api, notify, refresh]);

  const uninstall = useCallback(
    async (appId: string) => {
      try {
        const out = await api.uninstallApp(appId);
        notify("success", `${nameOf(appId)} was uninstalled.`);
        if (out.keptEntries.length > 0 && out.keptPath) {
          notify(
            "info",
            `Kept ${out.keptEntries.join(", ")} in ${out.keptPath} because CraftHub did not install them.`,
          );
        }
      } catch (e) {
        notify("error", `${nameOf(appId)}: ${messageOf(e)}`);
      } finally {
        await refresh();
      }
    },
    [api, nameOf, notify, refresh],
  );

  const rollback = useCallback(
    async (appId: string) => {
      try {
        const v = await api.rollbackApp(appId);
        notify("success", `${nameOf(appId)} switched to ${v.version}.`);
      } catch (e) {
        notify("error", `${nameOf(appId)}: ${messageOf(e)}`);
      } finally {
        await refresh();
      }
    },
    [api, nameOf, notify, refresh],
  );

  const launch = useCallback(
    async (appId: string) => {
      try {
        await api.launchApp(appId);
        notify("success", `Starting ${nameOf(appId)}…`);
        setTimeout(() => void refresh(), 2500);
      } catch (e) {
        notify("error", `${nameOf(appId)}: ${messageOf(e)}`);
      }
    },
    [api, nameOf, notify, refresh],
  );

  const cancel = useCallback(
    async (appId: string) => {
      const op = ops[appId];
      if (!op) return;
      try {
        await api.cancelOperation(op.opId);
      } catch (e) {
        notify("error", messageOf(e));
      }
    },
    [api, notify, ops],
  );

  const guard = useCallback(
    async (fn: () => Promise<unknown>) => {
      try {
        await fn();
      } catch (e) {
        notify("error", messageOf(e));
      }
    },
    [notify],
  );

  const saveSettings = useCallback(
    async (s: Settings) => {
      try {
        setSettings(await api.saveSettings(s));
        await refresh();
      } catch (e) {
        notify("error", messageOf(e));
      }
    },
    [api, notify, refresh],
  );

  const createShortcut = useCallback(
    async (appId: string) => {
      try {
        await api.createShortcut(appId);
        notify("success", `${nameOf(appId)} desktop shortcut is ready.`);
      } catch (e) {
        notify("error", `${nameOf(appId)}: ${messageOf(e)}`);
      } finally {
        await refresh();
      }
    },
    [api, nameOf, notify, refresh],
  );

  const removeShortcut = useCallback(
    async (appId: string) => {
      try {
        await api.removeShortcut(appId);
        notify("success", `${nameOf(appId)} desktop shortcut removed.`);
      } catch (e) {
        notify("error", `${nameOf(appId)}: ${messageOf(e)}`);
      } finally {
        await refresh();
      }
    },
    [api, nameOf, notify, refresh],
  );

  const answerExit = useCallback(
    async (exit: boolean) => {
      setExitRequest(null);
      if (exit) await guard(() => api.exitApp());
    },
    [api, guard],
  );

  const store = useMemo<Store>(
    () => ({
      api,
      env,
      settings,
      apps,
      loaded,
      checking,
      updatingAll,
      ops,
      failures,
      toasts,
      updateAllSummary,
      view,
      detailsId,
      setView,
      setDetailsId,
      exitRequest,
      refresh,
      refreshEnvironment,
      checkForUpdates,
      install,
      update,
      retry,
      dismissFailure,
      updateAll,
      uninstall,
      rollback,
      launch,
      cancel,
      openReleases: (appId, tag) => guard(() => api.openReleasesPage(appId, tag)),
      openFolder: (appId) => guard(() => api.openInstallFolder(appId)),
      saveSettings,
      dismissToast,
      clearUpdateAllSummary: () => setUpdateAllSummary(null),
      answerExit,
      notify,
      installDialog,
      requestInstall,
      requestUpdate,
      browseInstallRoot,
      confirmInstall,
      cancelInstallDialog: () => setInstallDialog(null),
      updateInstallDialog: (patch) => setInstallDialog((d) => (d ? { ...d, ...patch } : d)),
      createShortcut,
      removeShortcut,
    }),
    [
      api,
      env,
      settings,
      apps,
      loaded,
      checking,
      updatingAll,
      ops,
      failures,
      toasts,
      updateAllSummary,
      view,
      detailsId,
      exitRequest,
      refresh,
      refreshEnvironment,
      checkForUpdates,
      install,
      update,
      retry,
      dismissFailure,
      updateAll,
      uninstall,
      rollback,
      launch,
      cancel,
      guard,
      saveSettings,
      dismissToast,
      answerExit,
      notify,
      installDialog,
      requestInstall,
      requestUpdate,
      browseInstallRoot,
      confirmInstall,
      createShortcut,
      removeShortcut,
    ],
  );

  return <Ctx.Provider value={store}>{children}</Ctx.Provider>;
}
