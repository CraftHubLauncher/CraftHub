import { Download, ExternalLink, Play, RefreshCw, RotateCw, Search, X } from "lucide-react";
import { formatBytes, isActivePhase } from "../format";
import { useStore } from "../store";
import type { AppView } from "../types";
import { OperationProgress } from "./Progress";

/** Phases after which cancelling could leave things half-done, so it is disabled. */
const CANCELLABLE = new Set(["resolving", "downloading", "verifying", "extracting"]);

export function AppActions({ app, compact = false }: { app: AppView; compact?: boolean }) {
  const s = useStore();
  const op = s.ops[app.id];

  if (op && isActivePhase(op.phase)) {
    const canCancel = CANCELLABLE.has(op.phase);
    return (
      <div className="actions actions-progress">
        <OperationProgress op={op} appName={app.name} />
        <button
          className="btn btn-ghost"
          onClick={() => void s.cancel(app.id)}
          disabled={!canCancel}
          title={
            canCancel
              ? "Cancel and discard the download"
              : "Finishing up; it is not safe to cancel now"
          }
        >
          <X size={16} /> Cancel
        </button>
      </div>
    );
  }

  if (app.busy) {
    return (
      <div className="actions">
        <button className="btn" disabled title="Another operation is running for this app">
          Working…
        </button>
      </div>
    );
  }

  const open = (
    <button
      className="btn btn-primary"
      onClick={() => void s.launch(app.id)}
      aria-label={`Open ${app.name}`}
    >
      <Play size={16} /> Open
    </button>
  );

  switch (app.status) {
    case "unavailable":
      return (
        <div className="actions">
          <button
            className="btn"
            disabled
            title={app.unsupportedReason ?? "Unavailable"}
            aria-label={`${app.name} is unavailable`}
          >
            Unavailable
          </button>
          {!compact && (
            <button className="btn btn-ghost" onClick={() => void s.openReleases(app.id)}>
              <ExternalLink size={16} /> Releases
            </button>
          )}
        </div>
      );
    case "notInstalled":
      if (!app.latest) {
        return (
          <div className="actions">
            <button
              className="btn"
              onClick={() => void s.checkForUpdates()}
              disabled={s.checking}
              title="Ask GitHub which releases are available"
            >
              <Search size={16} /> {s.checking ? "Checking…" : "Check availability"}
            </button>
          </div>
        );
      }
      return (
        <div className="actions">
          <button
            className="btn btn-primary"
            onClick={() => void s.requestInstall(app.id)}
            aria-label={`Install ${app.name} ${app.latest.version}`}
            title={
              app.latest.asset
                ? `${app.latest.asset.name} · ${formatBytes(app.latest.asset.size)}`
                : undefined
            }
          >
            <Download size={16} /> Install
          </button>
        </div>
      );
    case "installed":
      return <div className="actions">{open}</div>;
    case "updateAvailable":
      return (
        <div className="actions">
          <button
            className="btn btn-accent"
            onClick={() => void s.requestUpdate(app.id)}
            disabled={app.running}
            title={
              app.running ? `Close ${app.name} to update it` : `Update to ${app.latest?.version}`
            }
            aria-label={`Update ${app.name} to ${app.latest?.version}`}
          >
            <RefreshCw size={16} /> Update
          </button>
          {open}
        </div>
      );
  }
}

/** Inline, persistent error for the last failed install/update, with Retry. */
export function FailureNotice({ app }: { app: AppView }) {
  const s = useStore();
  const f = s.failures[app.id];
  if (!f || s.ops[app.id]) return null;
  const hint =
    f.kind === "appRunning"
      ? `Close ${app.name}, then retry.`
      : f.kind === "network" || f.kind === "rateLimited"
        ? "Check your connection, then retry."
        : f.kind === "diskSpace"
          ? "Free up disk space, then retry."
          : null;
  return (
    <div className="failure" role="alert">
      <div>
        <strong>{f.action === "update" ? "Update failed" : "Install failed"}.</strong> {f.message}
        {hint && <div className="muted">{hint}</div>}
        {f.action === "update" && (
          <div className="muted">The previously installed version is still in place.</div>
        )}
      </div>
      <div className="actions">
        <button className="btn btn-small" onClick={() => void s.retry(app.id)}>
          <RotateCw size={14} /> Retry
        </button>
        <button
          className="btn btn-small btn-ghost"
          onClick={() => s.dismissFailure(app.id)}
          aria-label={`Dismiss ${app.name} error`}
        >
          Dismiss
        </button>
      </div>
    </div>
  );
}
