import { formatBytes } from "../format";
import type { Progress } from "../types";

export default function ProgressCard({ progress }: { progress: Progress }) {
  return (
    <div className="progress-card">
      <div className="row-between">
        <span>{progress.label}</span>
        <span className="muted">
          {progress.percent}%
          {progress.total > 0 ? ` · ${progress.current}/${progress.total}` : ""}
        </span>
      </div>
      <div className="progress-bar">
        <div
          className="progress-fill"
          style={{ width: `${Math.min(100, progress.percent)}%` }}
        />
      </div>
      {progress.bytes_total > 0 && (
        <p className="muted" style={{ marginTop: 6 }}>
          {formatBytes(progress.bytes_done)} / {formatBytes(progress.bytes_total)}
        </p>
      )}
    </div>
  );
}
