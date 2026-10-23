import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  isPermissionGranted,
  requestPermission,
} from "@tauri-apps/plugin-notification";
import "./App.css";
import { formatRunTime } from "./format";
import { browserLang, I18nProvider, translator, type MessageKey } from "./i18n";
import BackupTab from "./tabs/BackupTab";
import DestinationsTab from "./tabs/DestinationsTab";
import HistoryTab from "./tabs/HistoryTab";
import SettingsTab from "./tabs/SettingsTab";
import type {
  BackupResult,
  Bootstrap,
  Estimate,
  HistoryEntry,
  Progress,
  StatusView,
  Tab,
  WithRefresh,
} from "./types";

const TABS: ReadonlyArray<readonly [Tab, MessageKey]> = [
  ["backup", "tab.backup"],
  ["destinations", "tab.destinations"],
  ["history", "tab.history"],
  ["settings", "tab.settings"],
];

export default function App() {
  const [tab, setTab] = useState<Tab>("backup");
  const [boot, setBoot] = useState<Bootstrap | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [lastResult, setLastResult] = useState<BackupResult | null>(null);
  const [latestFailure, setLatestFailure] = useState<HistoryEntry | null>(null);
  const [status, setStatus] = useState<StatusView | null>(null);
  const [estimate, setEstimate] = useState<Estimate | null>(null);
  const [progress, setProgress] = useState<Progress | null>(null);
  const [historyVersion, setHistoryVersion] = useState(0);
  const lang = boot?.language ?? browserLang();
  const t = translator(lang);

  useEffect(() => {
    document.documentElement.lang = lang === "zh" ? "zh-CN" : "en";
  }, [lang]);

  const estimateSeq = useRef(0);
  const refreshEstimate = useCallback(async () => {
    const seq = ++estimateSeq.current;
    try {
      const data = await invoke<Estimate>("estimate_backup_size");
      if (seq === estimateSeq.current) setEstimate(data);
    } catch {
      if (seq === estimateSeq.current) setEstimate(null);
    }
  }, []);

  const refreshStatus = useCallback(async () => {
    setStatus(await invoke<StatusView>("get_status"));
  }, []);

  const refreshAfterRun = useCallback(async () => {
    setHistoryVersion((v) => v + 1);
    const [failure] = await Promise.all([
      invoke<HistoryEntry | null>("get_latest_failure"),
      refreshStatus(),
      refreshEstimate(),
    ]);
    setLatestFailure(failure);
  }, [refreshStatus, refreshEstimate]);

  useEffect(() => {
    invoke<Bootstrap>("get_bootstrap")
      .then(setBoot)
      .catch((e) => setError(String(e)));
    refreshAfterRun().catch(() => undefined);
    (async () => {
      if (!(await isPermissionGranted())) await requestPermission();
    })().catch(() => undefined);

    const unlistenFinished = listen<BackupResult>("backup-finished", (event) => {
      setLastResult(event.payload);
      setProgress(null);
      if (event.payload.overall_status !== "ok") {
        setError(event.payload.message);
      }
      refreshAfterRun().catch(() => undefined);
    });
    const unlistenProgress = listen<Progress>("backup-progress", (event) => {
      setProgress(event.payload);
    });
    return () => {
      unlistenFinished.then((f) => f()).catch(() => undefined);
      unlistenProgress.then((f) => f()).catch(() => undefined);
    };
  }, [refreshAfterRun]);

  const withRefresh: WithRefresh = async (fn) => {
    setError(null);
    try {
      setBoot(await fn());
      await Promise.all([refreshEstimate(), refreshStatus()]);
    } catch (e) {
      setError(String(e));
    }
  };

  async function startBackup(command: "run_backup_now" | "retry_backup", label: string) {
    setBusy(true);
    setError(null);
    setProgress({
      phase: "estimating",
      current: 0,
      total: 1,
      label,
      bytes_done: 0,
      bytes_total: estimate?.bytes ?? 0,
      percent: 0,
    });
    try {
      const result = await invoke<BackupResult>(command);
      setLastResult(result);
      if (result.overall_status === "failed") {
        setError(result.message);
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
      setProgress(null);
      await refreshAfterRun().catch(() => undefined);
    }
  }
  const runBackup = () => startBackup("run_backup_now", t("app.preparing"));
  const retryBackup = () => startBackup("retry_backup", t("app.preparingRetry"));

  if (!boot) {
    return (
      <div className="shell">
        <p className="muted">{t("app.loading")}</p>
        {error && <p className="error">{error}</p>}
      </div>
    );
  }

  return (
    <I18nProvider lang={lang}>
      <div className="shell">
        <header className="top">
          <div>
            <h1>Agent Backup</h1>
            <p className="muted">
              {t("app.host")} <code>{boot.hostname}</code> · {t("app.oneWay")}
            </p>
          </div>
          <nav className="tabs">
            {TABS.map(([id, labelKey]) => (
              <button
                key={id}
                className={tab === id ? "tab active" : "tab"}
                onClick={() => setTab(id)}
              >
                {t(labelKey)}
              </button>
            ))}
          </nav>
        </header>

        {error && <div className="banner error-banner">{error}</div>}
        {boot.warnings.length > 0 && (
          <div className="banner error-banner">
            {t("app.presetWarnings")}
            <ul>
              {boot.warnings.map((w) => (
                <li key={w}>{w}</li>
              ))}
            </ul>
          </div>
        )}
        {latestFailure && (
          <div className="banner error-banner row-between">
            <div>
              {t("app.latestFailure", { time: formatRunTime(latestFailure.created_at, lang) })}
              {latestFailure.message}
            </div>
            <div className="row-gap">
              <button onClick={() => setTab("history")}>{t("app.review")}</button>
              <button className="primary" disabled={busy} onClick={retryBackup}>
                {busy ? t("app.retrying") : t("app.runAgain")}
              </button>
            </div>
          </div>
        )}
        {lastResult && lastResult.overall_status === "ok" && (
          <div className="banner ok-banner">
            {t("app.backupDone", { message: lastResult.message })}
          </div>
        )}

        {tab === "backup" && (
          <BackupTab
            boot={boot}
            busy={busy}
            estimate={estimate}
            progress={progress}
            status={status}
            withRefresh={withRefresh}
            onBackup={runBackup}
          />
        )}
        {tab === "destinations" && <DestinationsTab boot={boot} withRefresh={withRefresh} />}
        {tab === "history" && (
          <HistoryTab
            version={historyVersion}
            busy={busy}
            onRetry={retryBackup}
            onError={setError}
          />
        )}
        {tab === "settings" && <SettingsTab boot={boot} withRefresh={withRefresh} />}
      </div>
    </I18nProvider>
  );
}
