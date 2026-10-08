import { formatBytes, PHASE_LABEL, progressFraction } from "../format";
import type { ProgressEvent } from "../types";

/** Shows real byte progress for download/unpack; other phases are indeterminate. */
export function OperationProgress({ op, appName }: { op: ProgressEvent; appName: string }) {
  const fraction = progressFraction(op);
  const pct = fraction === null ? undefined : Math.round(fraction * 100);
  const detail =
    op.phase === "downloading" && op.total > 0
      ? `${formatBytes(op.done)} of ${formatBytes(op.total)}`
      : op.phase === "failed" || op.phase === "cancelled"
        ? (op.message ?? "")
        : "";
  return (
    <div className="op-progress">
      <div className="op-progress-label">
        <span>
          {PHASE_LABEL[op.phase]}
          {pct !== undefined ? ` ${pct}%` : "…"}
        </span>
        <span className="muted">{detail}</span>
      </div>
      <div
        className={`bar ${fraction === null ? "bar-indeterminate" : ""}`}
        role="progressbar"
        aria-label={`${appName}: ${PHASE_LABEL[op.phase]}`}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={pct}
      >
        <div className="bar-fill" style={fraction === null ? undefined : { width: `${pct}%` }} />
      </div>
    </div>
  );
}
