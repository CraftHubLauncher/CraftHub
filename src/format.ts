import type { AppView, Phase, ProgressEvent } from "./types";

export function formatBytes(n: number): string {
  if (!Number.isFinite(n) || n < 0) return "—";
  const units = ["B", "KB", "MB", "GB"];
  let v = n;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v >= 10 || i === 0 ? v.toFixed(0) : v.toFixed(1)} ${units[i]}`;
}

export function formatRelative(unixSeconds: number | null, now = Date.now()): string {
  if (!unixSeconds) return "never";
  const diff = Math.round(now / 1000 - unixSeconds);
  if (diff < 45) return "just now";
  if (diff < 3600) return `${Math.round(diff / 60)} min ago`;
  if (diff < 86400) return `${Math.round(diff / 3600)} h ago`;
  return `${Math.round(diff / 86400)} d ago`;
}

export function formatDate(iso: string | null): string {
  if (!iso) return "";
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? "" : d.toLocaleDateString();
}

export const PHASE_LABEL: Record<Phase, string> = {
  resolving: "Checking release",
  downloading: "Downloading",
  verifying: "Verifying SHA-256",
  extracting: "Unpacking",
  activating: "Activating",
  cleaningUp: "Cleaning up",
  completed: "Done",
  failed: "Failed",
  cancelled: "Cancelled",
};

export function isActivePhase(p: Phase): boolean {
  return p !== "completed" && p !== "failed" && p !== "cancelled";
}

/** Only download and extraction report measurable progress; other phases are indeterminate. */
export function progressFraction(e: ProgressEvent): number | null {
  if ((e.phase === "downloading" || e.phase === "extracting") && e.total > 0) {
    return Math.min(1, e.done / e.total);
  }
  return null;
}

export function verificationLabel(v: string): string {
  switch (v) {
    case "github-digest+sha256sums":
      return "SHA-256 matches GitHub release metadata and the release's SHA256SUMS file";
    case "github-digest":
      return "SHA-256 matches GitHub release metadata";
    default:
      return v;
  }
}

export function statusLabel(app: AppView): string {
  if (app.busy) return "Working…";
  switch (app.status) {
    case "unavailable":
      return "Unavailable";
    case "notInstalled":
      return app.latest ? "Available" : "Not installed";
    case "installed":
      return "Installed";
    case "updateAvailable":
      return "Update available";
  }
}

export function matchesQuery(app: AppView, q: string): boolean {
  const s = q.trim().toLowerCase();
  if (!s) return true;
  return [app.name, app.description, app.id].some((f) => f.toLowerCase().includes(s));
}
