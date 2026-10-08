import { useStore } from "../store";
import { statusLabel } from "../format";
import type { AppView } from "../types";
import { AppActions, FailureNotice } from "./AppActions";
import { AppIcon } from "./AppIcon";

export function StatusPill({ app }: { app: AppView }) {
  return <span className={`pill pill-${app.busy ? "busy" : app.status}`}>{statusLabel(app)}</span>;
}

export function VersionLine({ app }: { app: AppView }) {
  const parts: string[] = [];
  if (app.installed) parts.push(`Installed ${app.installed.version}`);
  if (app.latest && app.latest.version !== app.installed?.version)
    parts.push(`Latest ${app.latest.version}`);
  if (!app.installed && !app.latest && app.status !== "unavailable") parts.push("Not checked yet");
  return <span className="version-line">{parts.join(" · ") || " "}</span>;
}

export function AppCard({ app, onDetails }: { app: AppView; onDetails: (id: string) => void }) {
  return (
    <article
      className={`card ${app.status === "unavailable" ? "card-dim" : ""}`}
      aria-label={app.name}
    >
      <button
        className="card-main"
        onClick={() => onDetails(app.id)}
        aria-label={`${app.name} details`}
      >
        <AppIcon appId={app.id} />
        <div className="card-text">
          <div className="card-title">
            <h3>{app.name}</h3>
            {app.category === "ai-studio" && (
              <span className="tag">AI studio · separate product</span>
            )}
            {app.running && <span className="tag tag-live">Running</span>}
          </div>
          <p className="muted">{app.description}</p>
          <div className="card-meta">
            <StatusPill app={app} />
            <VersionLine app={app} />
          </div>
          {app.status === "unavailable" && app.unsupportedReason && (
            <p className="reason">{app.unsupportedReason}</p>
          )}
        </div>
      </button>
      <FailureNotice app={app} />
      <AppActions app={app} />
    </article>
  );
}

export function AppGrid({
  apps,
  onDetails,
  empty,
}: {
  apps: AppView[];
  onDetails: (id: string) => void;
  empty: string;
}) {
  const s = useStore();
  if (!s.loaded) return <p className="muted">Loading…</p>;
  if (apps.length === 0) return <p className="empty">{empty}</p>;
  return (
    <div className="grid">
      {apps.map((a) => (
        <AppCard key={a.id} app={a} onDetails={onDetails} />
      ))}
    </div>
  );
}
