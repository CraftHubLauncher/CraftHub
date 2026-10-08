import { FolderOpen } from "lucide-react";
import { useStore } from "../store";
import { Modal } from "./Modal";

export function InstallDialog() {
  const s = useStore();
  const d = s.installDialog;
  if (!d) return null;
  const app = s.apps.find((a) => a.id === d.appId);
  if (!app) return null;
  const finalPath = d.update ? (app.installed?.path ?? d.parent) : `${d.parent}\\Apps\\${d.appId}`;

  return (
    <Modal
      title={d.update ? `Update ${app.name}` : `Install ${app.name}`}
      onClose={s.cancelInstallDialog}
      footer={
        <>
          <button className="btn" onClick={s.cancelInstallDialog}>
            Cancel
          </button>
          <button className="btn btn-primary" onClick={() => void s.confirmInstall()}>
            {d.update ? "Update" : "Install"}
          </button>
        </>
      }
    >
      <p>
        {d.update
          ? "The update will keep the existing installation location."
          : "Choose where this app should be installed."}
      </p>
      <dl className="facts">
        <dt>Application</dt>
        <dd>{app.name}</dd>
        <dt>Version</dt>
        <dd>{d.version ?? app.latest?.version ?? "latest"}</dd>
        <dt>Parent folder</dt>
        <dd className="path">
          <span>{d.parent || "Not selected"}</span>
        </dd>
        <dt>App folder</dt>
        <dd className="path">
          <span>{finalPath}</span>
        </dd>
      </dl>
      {!d.update && (
        <button className="btn btn-ghost" onClick={() => void s.browseInstallRoot()}>
          <FolderOpen size={16} /> Browse…
        </button>
      )}
      <label className="check">
        <input
          type="checkbox"
          checked={d.createShortcut}
          onChange={(e) => s.updateInstallDialog({ createShortcut: e.target.checked })}
        />
        <span>Create desktop shortcut</span>
      </label>
      <p className="fineprint">
        CraftHub uses a separate app folder under the selected parent and will not overwrite
        unrelated files.
      </p>
    </Modal>
  );
}
