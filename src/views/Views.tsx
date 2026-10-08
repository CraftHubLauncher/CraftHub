import { useEffect, useState } from "react";
import { FolderOpen, RefreshCw, Trash2 } from "lucide-react";
import { AppGrid } from "../components/AppCard";
import { AppIcon } from "../components/AppIcon";
import { DISCLAIMER } from "../components/Chrome";
import { matchesQuery } from "../format";
import { useStore } from "../store";
import type {
  Channel,
  EventRow,
  SelfUpdateInfo,
  SelfUpdateStatus,
  Settings,
  UpdateMode,
} from "../types";

type DetailsFn = (id: string) => void;

export function HomeView({ query, onDetails }: { query: string; onDetails: DetailsFn }) {
  const s = useStore();
  const apps = s.apps.filter((a) => matchesQuery(a, query));
  const installed = apps.filter((a) => a.installed);
  const updates = apps.filter((a) => a.updateAvailable);
  const discover = apps.filter((a) => !a.installed && a.status === "notInstalled");
  return (
    <div className="view">
      <section className="hero">
        <h1>Your creative tools, in one place.</h1>
        <p className="muted">
          Install, open and update the Craft apps from their official GitHub releases. {DISCLAIMER}
        </p>
      </section>
      {s.env?.engineError && <p className="note note-warn">{s.env.engineError}</p>}
      {installed.length > 0 && (
        <section>
          <h2>Your apps</h2>
          <div className="launcher">
            {installed.map((a) => (
              <button
                key={a.id}
                className="launch-tile"
                onClick={() => void s.launch(a.id)}
                aria-label={`Open ${a.name}`}
                title={`Open ${a.name} ${a.installed?.version}`}
              >
                <AppIcon appId={a.id} size={52} />
                <span>{a.name}</span>
                {a.updateAvailable && <span className="dot" aria-label="update available" />}
              </button>
            ))}
          </div>
        </section>
      )}
      {updates.length > 0 && (
        <section>
          <h2>Updates</h2>
          <AppGrid apps={updates} onDetails={onDetails} empty="" />
        </section>
      )}
      <section>
        <h2>{installed.length > 0 ? "Discover more" : "Get started"}</h2>
        <AppGrid
          apps={discover}
          onDetails={onDetails}
          empty={
            s.apps.some((a) => a.check.checkedAt)
              ? "Every available app is installed."
              : "Check for updates to see what's available."
          }
        />
      </section>
    </div>
  );
}

export function AppsView({
  filter,
  query,
  onDetails,
}: {
  filter: "all" | "installed" | "updates";
  query: string;
  onDetails: DetailsFn;
}) {
  const s = useStore();
  const base = s.apps.filter((a) => matchesQuery(a, query));
  const suite = base.filter((a) => a.category === "craft-suite");
  const studio = base.filter((a) => a.category === "ai-studio");

  if (filter === "all") {
    return (
      <div className="view">
        <h1>All Apps</h1>
        <AppGrid apps={suite} onDetails={onDetails} empty="No apps match your search." />
        {studio.length > 0 && (
          <section>
            <h2>Separate AI studio</h2>
            <p className="muted">ArtCraft is a distinct product from the Craft suite.</p>
            <AppGrid apps={studio} onDetails={onDetails} empty="" />
          </section>
        )}
      </div>
    );
  }
  if (filter === "installed") {
    return (
      <div className="view">
        <h1>Installed</h1>
        <AppGrid
          apps={base.filter((a) => a.installed)}
          onDetails={onDetails}
          empty="No apps installed yet. Browse All Apps to get started."
        />
      </div>
    );
  }
  const updates = base.filter((a) => a.updateAvailable);
  const anyChecked = s.apps.some((a) => a.check.checkedAt);
  return (
    <div className="view">
      <div className="view-head">
        <h1>Updates</h1>
        <button
          className="btn btn-accent"
          onClick={() => void s.updateAll()}
          disabled={updates.length === 0 || s.updatingAll}
          title={updates.length === 0 ? "No updates available" : "Install updates one at a time"}
        >
          <RefreshCw size={16} className={s.updatingAll ? "spin" : ""} />
          {s.updatingAll ? "Updating…" : "Update all"}
        </button>
      </div>
      <p className="muted">
        Updates install one at a time. Apps that are open are skipped, never closed. CraftHub keeps
        the previous version so you can roll back.
      </p>
      <AppGrid
        apps={updates}
        onDetails={onDetails}
        empty={
          anyChecked
            ? "Everything is up to date."
            : "Check for updates to see if new versions are available."
        }
      />
    </div>
  );
}

const INTERVALS = [0, 1, 6, 12, 24];

const UPDATE_MODES: Array<{ id: UpdateMode; label: string; detail: string }> = [
  { id: "manual", label: "Manual", detail: "show updates in CraftHub; you check and install" },
  { id: "notify", label: "Notify", detail: "check in the background and tell me" },
  {
    id: "automatic",
    label: "Automatic",
    detail: "install verified updates for apps that are closed",
  },
];

function InstallLocation() {
  const s = useStore();
  const [busy, setBusy] = useState(false);
  const env = s.env;
  const custom = !!s.settings?.installRoot;
  const run = async (fn: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await fn();
      await s.refreshEnvironment();
    } catch (e) {
      s.notify("error", e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <section className="section">
      <h2>Install location</h2>
      <dl className="facts">
        <dt>New apps go to</dt>
        <dd className="mono">{env?.installRoot ?? "—"}</dd>
        {custom && (
          <>
            <dt>Default</dt>
            <dd className="mono">{env?.defaultInstallRoot ?? "—"}</dd>
          </>
        )}
      </dl>
      <p className="fineprint">
        Changing this only affects apps you install from now on. Apps already installed stay where
        they are and keep updating there. Choose an empty folder on a local drive that you can write
        to; CraftHub never asks for administrator rights.
      </p>
      <div className="row">
        <button
          className="btn"
          disabled={busy}
          onClick={() =>
            void run(async () => {
              const root = await s.api.chooseInstallRoot();
              if (root) s.notify("success", `New apps will be installed in ${root}.`);
            })
          }
        >
          <FolderOpen size={16} /> Change…
        </button>
        {custom && (
          <button
            className="btn btn-ghost"
            disabled={busy}
            onClick={() =>
              void run(async () => {
                await s.api.resetInstallRoot();
                s.notify("info", "New apps will be installed in the default folder.");
              })
            }
          >
            Use default
          </button>
        )}
      </div>
    </section>
  );
}

function SelfUpdate() {
  const s = useStore();
  const [status, setStatus] = useState<SelfUpdateStatus | null>(null);
  const [found, setFound] = useState<SelfUpdateInfo | null | undefined>(undefined);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    s.api.getSelfUpdateStatus().then(setStatus, () => setStatus(null));
  }, [s.api]);
  if (!status) return null;
  const act = async (fn: () => Promise<void>) => {
    setBusy(true);
    try {
      await fn();
    } catch (e) {
      s.notify("error", e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="selfupdate">
      <p>CraftHub {status.currentVersion}</p>
      {!status.configured ? (
        <p className="fineprint">{status.reason}</p>
      ) : (
        <>
          <div className="row">
            <button
              className="btn"
              disabled={busy}
              onClick={() => void act(async () => setFound(await s.api.checkSelfUpdate()))}
            >
              Check for CraftHub updates
            </button>
            {found && (
              <button
                className="btn btn-primary"
                disabled={busy}
                onClick={() => void act(() => s.api.installSelfUpdate())}
              >
                Install CraftHub {found.version} and restart
              </button>
            )}
          </div>
          {found === null && <p className="muted">CraftHub is up to date.</p>}
          {found?.notes && <pre className="notes">{found.notes}</pre>}
          <p className="fineprint">
            CraftHub updates are verified with the project&apos;s update-signing key before they are
            installed.
          </p>
        </>
      )}
    </div>
  );
}

export function SettingsView() {
  const s = useStore();
  const [history, setHistory] = useState<EventRow[] | null>(null);
  const settings = s.settings;

  useEffect(() => {
    s.api.getHistory(50).then(setHistory, () => setHistory([]));
  }, [s.api, s.apps]);

  const save = (patch: Partial<Settings>) => {
    if (settings) void s.saveSettings({ ...settings, ...patch });
  };
  const nameOf = (id: string) => s.apps.find((a) => a.id === id)?.name ?? id;

  return (
    <div className="view settings">
      <h1>Settings</h1>

      <section className="section">
        <h2>Updates</h2>
        {!settings ? (
          <p className="muted">Loading…</p>
        ) : (
          <>
            <fieldset>
              <legend>When updates are found</legend>
              {UPDATE_MODES.map((m) => (
                <label key={m.id} className="radio radio-block">
                  <input
                    type="radio"
                    name="updateMode"
                    checked={settings.updateMode === m.id}
                    onChange={() => save({ updateMode: m.id })}
                  />
                  <span>
                    <strong>{m.label}</strong>
                    <span className="muted"> — {m.detail}</span>
                  </span>
                </label>
              ))}
            </fieldset>
            <fieldset>
              <legend>Release channel</legend>
              {(["stable", "beta"] as Channel[]).map((c) => (
                <label key={c} className="radio">
                  <input
                    type="radio"
                    name="channel"
                    checked={settings.channel === c}
                    onChange={() => save({ channel: c })}
                  />
                  {c === "stable" ? "Stable releases only" : "Include pre-releases (beta)"}
                </label>
              ))}
            </fieldset>
            <label className="check">
              <input
                type="checkbox"
                checked={settings.checkOnStartup}
                onChange={(e) => save({ checkOnStartup: e.target.checked })}
              />
              Check for updates when CraftHub starts
            </label>
            <label className="check">
              <input
                type="checkbox"
                checked={settings.createShortcuts ?? true}
                onChange={(e) => save({ createShortcuts: e.target.checked })}
              />
              Create desktop shortcuts by default for new installations
            </label>
            <label className="field">
              Check in the background
              <select
                value={settings.checkIntervalHours}
                disabled={settings.updateMode === "manual"}
                title={
                  settings.updateMode === "manual"
                    ? "Background checks are off in Manual mode"
                    : undefined
                }
                onChange={(e) => save({ checkIntervalHours: Number(e.target.value) })}
              >
                {INTERVALS.map((h) => (
                  <option key={h} value={h}>
                    {h === 0 ? "Never" : `Every ${h} hour${h === 1 ? "" : "s"}`}
                  </option>
                ))}
              </select>
            </label>
            <label className="check">
              <input
                type="checkbox"
                checked={settings.notifications}
                disabled={settings.updateMode === "manual"}
                onChange={(e) => save({ notifications: e.target.checked })}
              />
              Show Windows notifications for new updates (each version is announced once)
            </label>
            <p className="fineprint">
              GitHub limits how often release information can be requested; CraftHub reuses cached
              data and never checks more often than you choose. Running apps are never closed —
              automatic updates wait until the app is closed.
            </p>
          </>
        )}
      </section>

      {settings && (
        <section className="section">
          <h2>Window</h2>
          <label className="check">
            <input
              type="checkbox"
              checked={settings.minimizeToTray}
              onChange={(e) => save({ minimizeToTray: e.target.checked })}
            />
            Keep CraftHub running in the notification area when the window is closed
          </label>
          <p className="fineprint">
            Even when this is off, closing the window during an install keeps CraftHub running until
            the install finishes. Use Quit in the notification-area menu to exit.
          </p>
        </section>
      )}

      <InstallLocation />

      <section className="section">
        <h2>Storage</h2>
        <dl className="facts">
          <dt>CraftHub data</dt>
          <dd className="mono">{s.env?.dataDir ?? "—"}</dd>
          <dt>Logs</dt>
          <dd className="mono">{s.env?.logsDir ?? "—"}</dd>
        </dl>
        <div className="row">
          <button
            className="btn"
            onClick={() =>
              void s.api.openLogsFolder().catch((e) => s.notify("error", String(e.message ?? e)))
            }
          >
            <FolderOpen size={16} /> Open logs folder
          </button>
          <button
            className="btn"
            onClick={() =>
              void s.api
                .clearReleaseCache()
                .then(() => s.notify("info", "Cached release data cleared."))
                .then(() => s.refresh())
                .catch((e) => s.notify("error", String(e.message ?? e)))
            }
          >
            <Trash2 size={16} /> Clear release cache
          </button>
        </div>
      </section>

      <section className="section">
        <h2>History</h2>
        {!history ? (
          <p className="muted">Loading…</p>
        ) : history.length === 0 ? (
          <p className="muted">Nothing yet.</p>
        ) : (
          <table className="history">
            <thead>
              <tr>
                <th>When</th>
                <th>App</th>
                <th>Action</th>
                <th>Version</th>
                <th>Result</th>
              </tr>
            </thead>
            <tbody>
              {history.map((h) => (
                <tr key={h.id} title={h.message ?? ""}>
                  <td>{new Date(h.at * 1000).toLocaleString()}</td>
                  <td>{nameOf(h.appId)}</td>
                  <td>{h.kind}</td>
                  <td>{h.version ?? ""}</td>
                  <td
                    className={h.outcome === "success" ? "ok" : h.outcome === "failed" ? "bad" : ""}
                  >
                    {h.outcome}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </section>

      <section className="section">
        <h2>About</h2>
        <SelfUpdate />
        <p className="muted">{DISCLAIMER}</p>
        <p className="fineprint">
          Apps are downloaded only from the official GitHub releases of the storytold repositories
          and checked against GitHub's published SHA-256 before installation. A matching SHA-256
          proves the file is the one attached to the release, not who built it. Upstream apps and
          unsigned CraftHub builds may trigger Windows SmartScreen warnings.
        </p>
      </section>
    </div>
  );
}
