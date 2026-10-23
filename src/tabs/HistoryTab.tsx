import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { formatRunTime, statusLabel, triggerLabel } from "../format";
import { useI18n } from "../i18n";
import type { HistoryEntry } from "../types";

type Props = {
  /** Bumped by the parent whenever a run finishes, to reload the list. */
  version: number;
  busy: boolean;
  onRetry: () => void;
  onError: (message: string) => void;
};

export default function HistoryTab({ version, busy, onRetry, onError }: Props) {
  const { t } = useI18n();
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
          <h2>{t("history.title")}</h2>
          <p className="muted">{t("history.subtitle")}</p>
        </div>
        <label className="check small">
          <input
            type="checkbox"
            checked={onlyFailed}
            onChange={(e) => setOnlyFailed(e.target.checked)}
          />
          {t("history.onlyFailed")}
        </label>
      </div>

      {history.length === 0 ? (
        <p className="muted" style={{ marginTop: 12 }}>
          {t("history.empty")}
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
  const { lang, t } = useI18n();
  const openUri = h.destinations?.find((d) => d.status === "ok" && d.uri)?.uri;
  return (
    <li style={{ alignItems: "flex-start" }}>
      <div style={{ flex: 1 }}>
        <div className="row-gap">
          <strong>{formatRunTime(h.created_at, lang)}</strong>
          <span className="tag">{statusLabel(t, h.overall_status)}</span>
          <span className="tag">{triggerLabel(t, h.trigger)}</span>
        </div>
        <p className="muted">{h.message}</p>
        {expanded && (
          <div className="history-detail">
            <div>
              <strong>{t("history.sources")}</strong>
              <ul>
                {(h.sources ?? []).map((s) => (
                  <li key={s.key}>
                    {s.label}: {statusLabel(t, s.status)}
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
              <strong>{t("history.destinations")}</strong>
              <ul>
                {(h.destinations ?? []).map((d, idx) => (
                  <li key={`${d.name}-${idx}`}>
                    {d.name}: {statusLabel(t, d.status)}
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
        <button onClick={onToggle}>{expanded ? t("history.hide") : t("history.details")}</button>
        {openUri && (
          <button onClick={() => invoke("open_path", { path: openUri })}>
            {t("history.openFolder")}
          </button>
        )}
        {h.overall_status !== "ok" && (
          <button className="primary" disabled={busy} onClick={onRetry}>
            {t("history.retry")}
          </button>
        )}
      </div>
    </li>
  );
}
