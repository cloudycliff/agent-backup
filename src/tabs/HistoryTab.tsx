import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { formatRunTime, TRIGGER_LABELS } from "../format";
import type { HistoryEntry } from "../types";

type Props = {
  /** Bumped by the parent whenever a run finishes, to reload the list. */
  version: number;
  busy: boolean;
  onRetry: () => void;
  onError: (message: string) => void;
};

export default function HistoryTab({ version, busy, onRetry, onError }: Props) {
  const [history, setHistory] = useState<HistoryEntry[]>([]);
  const [onlyFailed, setOnlyFailed] = useState(false);
  const [expandedId, setExpandedId] = useState<string | null>(null);

  useEffect(() => {
    invoke<HistoryEntry[]>("list_history", { onlyFailed })
      .then(setHistory)
      .catch((e) => onError(String(e)));
  }, [onlyFailed, version, onError]);

  return (
    <section className="panel">
      <div className="row-between">
        <div>
          <h2>备份记录</h2>
          <p className="muted">成功静默；失败会系统通知。重试会按当前配置整次重跑。</p>
        </div>
        <label className="check small">
          <input
            type="checkbox"
            checked={onlyFailed}
            onChange={(e) => setOnlyFailed(e.target.checked)}
          />
          仅失败
        </label>
      </div>

      {history.length === 0 ? (
        <p className="muted" style={{ marginTop: 12 }}>
          暂无记录
        </p>
      ) : (
        <ul className="list" style={{ marginTop: 12 }}>
          {history.map((h) => (
            <HistoryItem
              key={h.id}
              entry={h}
              expanded={expandedId === h.id}
              onToggle={() => setExpandedId(expandedId === h.id ? null : h.id)}
              busy={busy}
              onRetry={onRetry}
            />
          ))}
        </ul>
      )}
    </section>
  );
}

function HistoryItem({
  entry: h,
  expanded,
  onToggle,
  busy,
  onRetry,
}: {
  entry: HistoryEntry;
  expanded: boolean;
  onToggle: () => void;
  busy: boolean;
  onRetry: () => void;
}) {
  const openUri = h.destinations?.find((d) => d.status === "ok" && d.uri)?.uri;
  return (
    <li style={{ alignItems: "flex-start" }}>
      <div style={{ flex: 1 }}>
        <div className="row-gap">
          <strong>{formatRunTime(h.created_at)}</strong>
          <span className="tag">{h.overall_status}</span>
          <span className="tag">{TRIGGER_LABELS[h.trigger] ?? h.trigger}</span>
        </div>
        <p className="muted">{h.message}</p>
        {expanded && (
          <div className="history-detail">
            <div>
              <strong>源</strong>
              <ul>
                {(h.sources ?? []).map((s) => (
                  <li key={s.key}>
                    {s.label}: {s.status}
                    {s.archive ? ` · ${s.archive}` : ""}
                    {s.error ? ` · ${s.error}` : ""}
                    {s.warnings && s.warnings.length > 0 && (
                      <ul className="muted">
                        {s.warnings.map((w) => (
                          <li key={w}>{w}</li>
                        ))}
                      </ul>
                    )}
                  </li>
                ))}
              </ul>
            </div>
            <div>
              <strong>目标</strong>
              <ul>
                {(h.destinations ?? []).map((d, idx) => (
                  <li key={`${d.name}-${idx}`}>
                    {d.name}: {d.status}
                    {d.error ? ` · ${d.error}` : ""}
                    {d.uri ? ` · ${d.uri}` : ""}
                  </li>
                ))}
              </ul>
            </div>
          </div>
        )}
      </div>
      <div className="row-gap" style={{ flexDirection: "column" }}>
        <button onClick={onToggle}>{expanded ? "收起" : "详情"}</button>
        {openUri && (
          <button onClick={() => invoke("open_path", { path: openUri })}>
            打开目录
          </button>
        )}
        {h.overall_status !== "ok" && (
          <button className="primary" disabled={busy} onClick={onRetry}>
            重试
          </button>
        )}
      </div>
    </li>
  );
}
