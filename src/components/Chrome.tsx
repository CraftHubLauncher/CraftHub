import {
  Download,
  House,
  LayoutGrid,
  RefreshCw,
  Settings as SettingsIcon,
  Wifi,
  WifiOff,
  X,
} from "lucide-react";
import { useStore } from "../store";
import type { AppView } from "../types";
import { Modal } from "./Modal";
import craftHubIcon from "../assets/branding/craftHubIcon.png";

export type { ViewId } from "../store";
import type { ViewId } from "../store";

export const DISCLAIMER =
  "Unofficial community application manager. Not affiliated with or endorsed by Storytold or ArtCraft.";

export function Logo() {
  return <img className="brand-logo" src={craftHubIcon} alt="" aria-hidden="true" />;
}

export function Sidebar({ view, onNavigate }: { view: ViewId; onNavigate: (v: ViewId) => void }) {
  const { apps } = useStore();
  const installed = apps.filter((a) => a.installed).length;
  const updates = apps.filter((a) => a.updateAvailable).length;
  const items: Array<{ id: ViewId; label: string; icon: typeof House; badge?: number }> = [
    { id: "home", label: "Home", icon: House },
    { id: "all", label: "All Apps", icon: LayoutGrid },
    { id: "installed", label: "Installed", icon: Download, badge: installed || undefined },
    { id: "updates", label: "Updates", icon: RefreshCw, badge: updates || undefined },
    { id: "settings", label: "Settings", icon: SettingsIcon },
  ];
  return (
    <nav className="sidebar" aria-label="Main">
      <div className="brand">
        <Logo />
        <div>
          <strong>CraftHub</strong>
          <span>Your creative tools, in one place.</span>
        </div>
      </div>
      <ul>
        {items.map((it) => (
          <li key={it.id}>
            <button
              className={`nav-item ${view === it.id ? "active" : ""}`}
              aria-current={view === it.id ? "page" : undefined}
              onClick={() => onNavigate(it.id)}
            >
              <it.icon size={18} />
              <span>{it.label}</span>
              {it.badge !== undefined && (
                <span
                  className={`badge ${it.id === "updates" ? "badge-accent" : ""}`}
                  aria-label={`${it.badge}`}
                >
                  {it.badge}
                </span>
              )}
            </button>
          </li>
        ))}
      </ul>
      <p className="sidebar-foot">{DISCLAIMER}</p>
    </nav>
  );
}

function connectivity(apps: AppView[]): { label: string; ok: boolean; detail: string } {
  const checked = apps.filter((a) => a.check.checkedAt);
  if (checked.length === 0)
    return { label: "Not checked", ok: true, detail: "No release check yet." };
  const problems = apps.filter((a) => a.check.error || a.check.warning);
  const anyLive = apps.some(
    (a) => a.check.source === "network" || a.check.source === "notModified",
  );
  if (problems.length > 0) {
    const first = problems[0]!;
    return {
      label: anyLive ? "Partly offline" : "Offline",
      ok: false,
      detail: first.check.error ?? first.check.warning ?? "",
    };
  }
  return anyLive
    ? { label: "Online", ok: true, detail: "Connected to GitHub." }
    : {
        label: "Cached",
        ok: true,
        detail: "Showing cached release data. Check for updates to refresh.",
      };
}

export function TopBar({ query, onQuery }: { query: string; onQuery: (q: string) => void }) {
  const s = useStore();
  const c = connectivity(s.apps);
  return (
    <header className="topbar">
      <input
        type="search"
        className="search"
        placeholder="Search apps"
        aria-label="Search apps"
        value={query}
        onChange={(e) => onQuery(e.target.value)}
      />
      <span className={`conn ${c.ok ? "" : "conn-bad"}`} title={c.detail} role="status">
        {c.ok ? <Wifi size={16} /> : <WifiOff size={16} />} {c.label}
      </span>
      <button className="btn" onClick={() => void s.checkForUpdates()} disabled={s.checking}>
        <RefreshCw size={16} className={s.checking ? "spin" : ""} />
        {s.checking ? "Checking…" : "Check for updates"}
      </button>
    </header>
  );
}

export function Toasts() {
  const { toasts, dismissToast } = useStore();
  return (
    <div className="toasts" aria-live="polite">
      {toasts.map((t) => (
        <div
          key={t.id}
          className={`toast toast-${t.kind}`}
          role={t.kind === "error" ? "alert" : "status"}
        >
          <span>{t.text}</span>
          <button className="icon-btn" onClick={() => dismissToast(t.id)} aria-label="Dismiss">
            <X size={14} />
          </button>
        </div>
      ))}
    </div>
  );
}

export function UpdateAllSummaryDialog() {
  const { updateAllSummary: s, clearUpdateAllSummary } = useStore();
  if (!s) return null;
  const nothing = s.updated.length + s.skipped.length + s.failed.length === 0;
  return (
    <Modal
      title="Update All finished"
      onClose={clearUpdateAllSummary}
      footer={
        <button className="btn btn-primary" onClick={clearUpdateAllSummary}>
          Close
        </button>
      }
    >
      {nothing && <p>Everything was already up to date.</p>}
      {s.updated.length > 0 && (
        <>
          <h3>Updated</h3>
          <ul className="summary-list">
            {s.updated.map((i) => (
              <li key={i.appId}>
                {i.name}: {i.from} → {i.to}
              </li>
            ))}
          </ul>
        </>
      )}
      {s.skipped.length > 0 && (
        <>
          <h3>Skipped</h3>
          <ul className="summary-list">
            {s.skipped.map((i) => (
              <li key={i.appId}>
                {i.name}: {i.reason}
              </li>
            ))}
          </ul>
        </>
      )}
      {s.failed.length > 0 && (
        <>
          <h3>Failed</h3>
          <ul className="summary-list">
            {s.failed.map((i) => (
              <li key={i.appId}>
                {i.name}: {i.reason} (the previous version is still installed)
              </li>
            ))}
          </ul>
        </>
      )}
    </Modal>
  );
}

/** Explains offline/rate-limited states and what still works. */
export function OfflineBanner() {
  const { apps } = useStore();
  const c = connectivity(apps);
  if (c.ok) return null;
  return (
    <div className="banner" role="status">
      <WifiOff size={16} />
      <span>
        {c.label === "Offline"
          ? "GitHub can't be reached right now."
          : "Some release checks failed."}{" "}
        Installed apps still open. Release information may be out of date
        {c.detail ? ` (${c.detail})` : ""}.
      </span>
    </div>
  );
}

/** Asked before quitting from the tray while installs/updates are running. */
export function ExitDialog() {
  const s = useStore();
  if (!s.exitRequest) return null;
  const names = s.exitRequest
    .map((o) => s.apps.find((a) => a.id === o.appId)?.name ?? o.appId)
    .join(", ");
  return (
    <Modal
      title="Quit while installing?"
      onClose={() => void s.answerExit(false)}
      footer={
        <>
          <button className="btn btn-primary" onClick={() => void s.answerExit(false)}>
            Keep working
          </button>
          <button className="btn btn-danger" onClick={() => void s.answerExit(true)}>
            Cancel and quit
          </button>
        </>
      }
    >
      <p>CraftHub is still working on: {names}.</p>
      <p className="muted">
        Quitting cancels these operations. Nothing half-installed is kept; installed versions stay
        as they were.
      </p>
    </Modal>
  );
}
