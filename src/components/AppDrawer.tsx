import { useState } from "react";
import {
  ExternalLink,
  FolderOpen,
  History,
  ShieldCheck,
  Trash2,
  TriangleAlert,
} from "lucide-react";
import { formatBytes, formatDate, formatRelative, verificationLabel } from "../format";
import { useStore } from "../store";
import type { AppView, ReleaseView } from "../types";
import { AppActions, FailureNotice } from "./AppActions";
import { StatusPill } from "./AppCard";
import { AppIcon } from "./AppIcon";
import { Modal } from "./Modal";

export function AppDrawer({ app, onClose }: { app: AppView; onClose: () => void }) {
  const s = useStore();
  const [confirmUninstall, setConfirmUninstall] = useState(false);
  const [versions, setVersions] = useState<ReleaseView[] | null>(null);
  const [versionsError, setVersionsError] = useState<string | null>(null);
  const [loadingVersions, setLoadingVersions] = useState(false);
  const blockedByRunning = app.running ? `Close ${app.name} first.` : undefined;

  const loadVersions = async () => {
    setLoadingVersions(true);
    try {
      setVersions(await s.api.getVersions(app.id));
      setVersionsError(null);
    } catch (e) {
      setVersionsError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoadingVersions(false);
    }
  };

  return (
    <Modal title={app.name} onClose={onClose} variant="drawer">
      <section className="drawer-hero">
        <AppIcon appId={app.id} size={56} />
        <div>
          <p className="muted">{app.description}</p>
          <div className="card-meta">
            <StatusPill app={app} />
            {app.running && <span className="tag tag-live">Running</span>}
          </div>
        </div>
      </section>
      {app.category === "ai-studio" && (
        <p className="note">ArtCraft is a separate AI studio product, listed here for discovery.</p>
      )}
      <FailureNotice app={app} />
      <AppActions app={app} />
      {app.unsupportedReason && (
        <p className="note note-warn">
          <TriangleAlert size={16} /> {app.unsupportedReason}
        </p>
      )}

      {app.installed && (
        <section className="section">
          <h3>Installed</h3>
          <dl className="facts">
            <dt>Version</dt>
            <dd>
              {app.installed.version} <span className="muted">({app.installed.tag})</span>
            </dd>
            <dt>Location</dt>
            <dd className="path">
              <span title={app.installed.path}>{app.installed.path}</span>
              <button className="btn btn-small btn-ghost" onClick={() => void s.openFolder(app.id)}>
                <FolderOpen size={14} /> Open folder
              </button>
            </dd>
            <dt>Integrity</dt>
            <dd>
              <ShieldCheck size={14} className="ok" />{" "}
              {verificationLabel(app.installed.verification)}
              <div className="mono muted" title={app.installed.sha256}>
                SHA-256 {app.installed.sha256.slice(0, 16)}…
              </div>
            </dd>
            <dt>Installed</dt>
            <dd>{formatRelative(app.installed.installedAt)}</dd>
            <dt>Desktop shortcut</dt>
            <dd className="path">
              <span>{app.installed.shortcutExists ? "Created by CraftHub" : "Not created"}</span>
              {app.installed.shortcutExists ? (
                <button
                  className="btn btn-small btn-ghost"
                  onClick={() => void s.removeShortcut(app.id)}
                >
                  Remove shortcut
                </button>
              ) : (
                <button
                  className="btn btn-small btn-ghost"
                  onClick={() => void s.createShortcut(app.id)}
                >
                  Create shortcut
                </button>
              )}
            </dd>
            {app.installed.previousVersion && (
              <>
                <dt>Rollback</dt>
                <dd>
                  {app.installed.previousVersion} is kept as a fallback.{" "}
                  <button
                    className="btn btn-small"
                    onClick={() => void s.rollback(app.id)}
                    disabled={!!blockedByRunning || app.busy}
                    title={blockedByRunning ?? `Switch back to ${app.installed.previousVersion}`}
                  >
                    <History size={14} /> Roll back to {app.installed.previousVersion}
                  </button>
                </dd>
              </>
            )}
          </dl>
        </section>
      )}

      {app.latest && (
        <section className="section">
          <h3>
            Latest release {app.latest.version}
            {app.latest.prerelease && <span className="tag">pre-release</span>}
          </h3>
          <p className="muted">
            {formatDate(app.latest.publishedAt)}
            {app.latest.asset &&
              ` · ${app.latest.asset.name} · ${formatBytes(app.latest.asset.size)}`}
          </p>
          <p className="fineprint">
            CraftHub checks the download's SHA-256 against GitHub's release metadata before
            unpacking. This confirms the file is the one attached to the release; it is not a
            publisher signature.
          </p>
          {app.latest.notes ? (
            <pre className="notes">{app.latest.notes}</pre>
          ) : (
            <p className="muted">No release notes were published.</p>
          )}
          <button
            className="btn btn-ghost"
            onClick={() => void s.openReleases(app.id, app.latest!.tag)}
          >
            <ExternalLink size={16} /> View release on GitHub
          </button>
        </section>
      )}

      {app.newestUnavailable && (
        <p className="note">
          A newer release ({app.newestUnavailable.version}) exists but can't be installed:{" "}
          {app.newestUnavailable.unavailableReason}
        </p>
      )}

      {app.supported && (
        <section className="section">
          <h3>Versions</h3>
          {!versions && (
            <button
              className="btn btn-ghost"
              onClick={() => void loadVersions()}
              disabled={loadingVersions}
            >
              {loadingVersions ? "Loading…" : "Show all versions"}
            </button>
          )}
          {versionsError && <p className="note note-warn">{versionsError}</p>}
          {versions && (
            <ul className="versions">
              {versions.map((v) => {
                const current = v.version === app.installed?.version;
                return (
                  <li key={v.tag}>
                    <span>
                      {v.version} {v.prerelease && <span className="tag">pre-release</span>}
                      {current && <span className="tag tag-live">installed</span>}
                    </span>
                    <span className="muted">{formatDate(v.publishedAt)}</span>
                    {v.installable ? (
                      <button
                        className="btn btn-small"
                        disabled={current || app.busy || (!!app.installed && app.running)}
                        title={
                          current
                            ? "Already installed"
                            : app.installed
                              ? blockedByRunning
                              : undefined
                        }
                        onClick={() => void s.requestInstall(app.id, v.version)}
                      >
                        {app.installed ? "Switch" : "Install"}
                      </button>
                    ) : (
                      <span className="muted small" title={v.unavailableReason ?? ""}>
                        unavailable
                      </span>
                    )}
                  </li>
                );
              })}
            </ul>
          )}
        </section>
      )}

      <section className="section">
        <h3>Source</h3>
        <p className="muted">
          Official releases from <span className="mono">github.com/{app.repo}</span>. Last checked{" "}
          {formatRelative(app.check.checkedAt)}
          {app.check.source === "staleCache" && " (cached)"}.
        </p>
        {app.check.warning && <p className="note note-warn">{app.check.warning}</p>}
        {app.check.error && <p className="note note-warn">{app.check.error}</p>}
        <button className="btn btn-ghost" onClick={() => void s.openReleases(app.id)}>
          <ExternalLink size={16} /> All releases on GitHub
        </button>
      </section>

      {app.installed && (
        <section className="section danger">
          <h3>Uninstall</h3>
          <p className="muted">
            Removes the files CraftHub installed. Documents and settings stored elsewhere are not
            touched.
          </p>
          <button
            className="btn btn-danger"
            onClick={() => setConfirmUninstall(true)}
            disabled={!!blockedByRunning || app.busy}
            title={blockedByRunning}
          >
            <Trash2 size={16} /> Uninstall {app.name}
          </button>
        </section>
      )}

      {confirmUninstall && (
        <Modal
          title={`Uninstall ${app.name}?`}
          onClose={() => setConfirmUninstall(false)}
          footer={
            <>
              <button className="btn" onClick={() => setConfirmUninstall(false)}>
                Keep
              </button>
              <button
                className="btn btn-danger"
                onClick={() => {
                  setConfirmUninstall(false);
                  void s.uninstall(app.id);
                }}
              >
                Uninstall
              </button>
            </>
          }
        >
          <p>
            CraftHub will remove {app.name} {app.installed?.version}
            {app.installed?.previousVersion
              ? ` and the fallback copy of ${app.installed.previousVersion}`
              : ""}
            .
          </p>
          <p className="muted">
            Your projects and app settings in other folders stay where they are. Files inside the
            install folder that CraftHub did not put there are also kept.
          </p>
        </Modal>
      )}
    </Modal>
  );
}
