import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import {
  isPermissionGranted,
  requestPermission,
} from "@tauri-apps/plugin-notification";
import "./App.css";

type GroupView = { id: string; label: string; default_enabled: boolean };
type AgentView = {
  key: string;
  label: string;
  installed: boolean;
  disabled: boolean;
  root_path?: string | null;
  groups: GroupView[];
  enabled: boolean;
  paths: Record<string, boolean>;
};

type Destination =
  | {
      type: "local";
      id: string;
      name: string;
      enabled: boolean;
      local: { root_path: string };
    }
  | {
      type: "s3";
      id: string;
      name: string;
      enabled: boolean;
      s3: Record<string, string>;
    };

type CustomSource = {
  id: string;
  label: string;
  path: string;
  enabled: boolean;
};

type AppConfig = {
  destinations: Destination[];
  sources: {
    agents: Record<string, unknown>;
    custom: CustomSource[];
  };
  encryption: { enabled: boolean; password: string | null };
  schedule: { enabled: boolean; time_local: string };
  app: {
    hostname_override: string | null;
    start_on_login: boolean;
    minimize_to_tray: boolean;
    language: string;
  };
};

type Bootstrap = {
  config: AppConfig;
  agents: AgentView[];
  config_dir: string;
  config_path: string;
  presets_path: string;
  hostname: string;
};

type BackupResult = {
  id?: string;
  created_at: string;
  overall_status: string;
  message: string;
  destinations: Array<{
    name: string;
    status: string;
    uri?: string | null;
    error?: string | null;
  }>;
  sources?: Array<{
    key: string;
    label: string;
    status: string;
    archive?: string;
    bytes?: number;
    error?: string | null;
  }>;
};

type HistoryEntry = {
  id: string;
  created_at: string;
  hostname: string;
  trigger: string;
  overall_status: string;
  message: string;
  sources: BackupResult["sources"];
  destinations: BackupResult["destinations"];
};

type Tab = "backup" | "destinations" | "history" | "settings";

export default function App() {
  const [tab, setTab] = useState<Tab>("backup");
  const [boot, setBoot] = useState<Bootstrap | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [lastResult, setLastResult] = useState<BackupResult | null>(null);
  const [docs, setDocs] = useState<string>("");
  const [passwordDraft, setPasswordDraft] = useState("");
  const [hostnameDraft, setHostnameDraft] = useState("");
  const [scheduleTime, setScheduleTime] = useState("21:00");
  const [version, setVersion] = useState("0.1.0");
  const [history, setHistory] = useState<HistoryEntry[]>([]);
  const [onlyFailed, setOnlyFailed] = useState(false);
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [latestFailure, setLatestFailure] = useState<HistoryEntry | null>(null);
  const [showS3Form, setShowS3Form] = useState(false);
  const [s3TestMsg, setS3TestMsg] = useState<string | null>(null);
  const [s3Form, setS3Form] = useState({
    name: "S3",
    endpoint: "",
    region: "auto",
    bucket: "",
    prefix: "",
    accessKey: "",
    secretKey: "",
  });

  const refresh = useCallback(async () => {
    const data = await invoke<Bootstrap>("get_bootstrap");
    setBoot(data);
    setHostnameDraft(data.config.app.hostname_override ?? "");
    setScheduleTime(data.config.schedule.time_local || "21:00");
    setPasswordDraft("");
  }, []);

  const refreshHistory = useCallback(async (failedOnly = onlyFailed) => {
    const rows = await invoke<HistoryEntry[]>("list_history", {
      onlyFailed: failedOnly,
    });
    setHistory(rows);
    const fail = await invoke<HistoryEntry | null>("get_latest_failure");
    setLatestFailure(fail);
  }, [onlyFailed]);

  useEffect(() => {
    refresh().catch((e) => setError(String(e)));
    refreshHistory(false).catch(() => undefined);
    invoke<string>("get_docs_hint")
      .then(setDocs)
      .catch(() => undefined);
    invoke<string>("get_app_version")
      .then(setVersion)
      .catch(() => undefined);
    (async () => {
      let granted = await isPermissionGranted();
      if (!granted) {
        const perm = await requestPermission();
        granted = perm === "granted";
      }
    })().catch(() => undefined);

    const unlisten = listen<BackupResult>("backup-finished", (event) => {
      setLastResult(event.payload);
      if (event.payload.overall_status !== "ok") {
        setError(event.payload.message);
      }
      refreshHistory().catch(() => undefined);
    });
    return () => {
      unlisten.then((f) => f()).catch(() => undefined);
    };
  }, [refresh, refreshHistory]);

  useEffect(() => {
    if (tab === "history") {
      refreshHistory(onlyFailed).catch((e) => setError(String(e)));
    }
  }, [tab, onlyFailed, refreshHistory]);

  const enabledDestCount = useMemo(
    () => boot?.config.destinations.filter((d) => d.enabled).length ?? 0,
    [boot],
  );

  async function withRefresh(fn: () => Promise<Bootstrap>) {
    setError(null);
    try {
      const data = await fn();
      setBoot(data);
    } catch (e) {
      setError(String(e));
    }
  }

  async function runBackup() {
    setBusy(true);
    setError(null);
    try {
      const result = await invoke<BackupResult>("run_backup_now");
      setLastResult(result);
      await refreshHistory();
      if (result.overall_status === "failed") {
        setError(result.message);
      }
    } catch (e) {
      setError(String(e));
      await refreshHistory().catch(() => undefined);
    } finally {
      setBusy(false);
    }
  }

  async function retryBackup() {
    setBusy(true);
    setError(null);
    try {
      const result = await invoke<BackupResult>("retry_backup");
      setLastResult(result);
      await refreshHistory();
      if (result.overall_status === "failed") {
        setError(result.message);
      } else {
        setLatestFailure(null);
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  if (!boot) {
    return (
      <div className="shell">
        <p className="muted">加载中…</p>
        {error && <p className="error">{error}</p>}
      </div>
    );
  }

  return (
    <div className="shell">
      <header className="top">
        <div>
          <h1>Agent Backup</h1>
          <p className="muted">
            主机 <code>{boot.hostname}</code> · 单向备份
          </p>
        </div>
        <nav className="tabs">
          {(
            [
              ["backup", "备份"],
              ["destinations", "目标"],
              ["history", "记录"],
              ["settings", "设置"],
            ] as const
          ).map(([id, label]) => (
            <button
              key={id}
              className={tab === id ? "tab active" : "tab"}
              onClick={() => setTab(id)}
            >
              {label}
            </button>
          ))}
        </nav>
      </header>

      {error && <div className="banner error-banner">{error}</div>}
      {latestFailure && (
        <div className="banner error-banner row-between">
          <div>
            最近一次备份未完全成功（{latestFailure.created_at}）：
            {latestFailure.message}
          </div>
          <div className="row-gap">
            <button onClick={() => setTab("history")}>去处理</button>
            <button className="primary" disabled={busy} onClick={retryBackup}>
              {busy ? "重试中…" : "整次重跑"}
            </button>
          </div>
        </div>
      )}
      {lastResult && lastResult.overall_status === "ok" && (
        <div className="banner ok-banner">
          备份完成：{lastResult.message}
        </div>
      )}

      {tab === "backup" && (
        <section className="panel">
          <div className="row-between">
            <div>
              <h2>备份源</h2>
              <p className="muted">密钥与缓存默认排除；会话默认不勾选。</p>
            </div>
            <button
              className="primary"
              disabled={busy || enabledDestCount === 0}
              onClick={runBackup}
            >
              {busy ? "备份中…" : "立即备份"}
            </button>
          </div>
          {enabledDestCount === 0 && (
            <p className="hint">
              尚未配置目标。请先到「目标」添加一个本地目录。
            </p>
          )}
          {boot.config.encryption.enabled && !boot.config.encryption.password && (
            <p className="hint">
              已开启加密但未设置密码。请到「设置」填写密码后再备份。
            </p>
          )}
          <p className="muted">将写入 {enabledDestCount} 个启用目标</p>

          <div className="cards">
            {boot.agents.map((agent) => (
              <article
                key={agent.key}
                className={agent.installed ? "card" : "card dim"}
              >
                <div className="row-between">
                  <label className="check">
                    <input
                      type="checkbox"
                      checked={agent.enabled && agent.installed}
                      disabled={!agent.installed}
                      onChange={(e) =>
                        withRefresh(() =>
                          invoke("set_agent_enabled", {
                            key: agent.key,
                            enabled: e.target.checked,
                          }),
                        )
                      }
                    />
                    <strong>{agent.label}</strong>
                  </label>
                  <span className="tag">
                    {agent.installed ? "已安装" : "未检测到"}
                  </span>
                </div>
                {agent.root_path && (
                  <p className="path">{agent.root_path}</p>
                )}
                {agent.installed && (
                  <div className="groups">
                    {agent.groups.map((g) => (
                      <label key={g.id} className="check small">
                        <input
                          type="checkbox"
                          checked={Boolean(agent.paths[g.id])}
                          disabled={!agent.enabled}
                          onChange={(e) =>
                            withRefresh(() =>
                              invoke("set_agent_path_enabled", {
                                key: agent.key,
                                groupId: g.id,
                                enabled: e.target.checked,
                              }),
                            )
                          }
                        />
                        {g.label}
                      </label>
                    ))}
                  </div>
                )}
              </article>
            ))}
          </div>

          <div className="section-head">
            <h3>自定义文件夹</h3>
            <button
              onClick={async () => {
                const selected = await open({ directory: true, multiple: false });
                if (!selected || Array.isArray(selected)) return;
                const label = selected.split(/[/\\]/).filter(Boolean).pop() ?? "custom";
                await withRefresh(() =>
                  invoke("add_custom_source", { label, path: selected }),
                );
              }}
            >
              添加文件夹
            </button>
          </div>
          {boot.config.sources.custom.length === 0 ? (
            <p className="muted">暂无自定义源</p>
          ) : (
            <ul className="list">
              {boot.config.sources.custom.map((c) => (
                <li key={c.id}>
                  <div>
                    <strong>{c.label}</strong>
                    <div className="path">{c.path}</div>
                  </div>
                  <button
                    className="danger"
                    onClick={() =>
                      withRefresh(() =>
                        invoke("remove_custom_source", { id: c.id }),
                      )
                    }
                  >
                    删除
                  </button>
                </li>
              ))}
            </ul>
          )}
        </section>
      )}

      {tab === "destinations" && (
        <section className="panel">
          <div className="row-between">
            <div>
              <h2>备份目标</h2>
              <p className="muted">
                支持多个目标：本地目录 + S3 兼容存储。一次备份会尽力写入全部启用目标。
              </p>
            </div>
            <div className="row-gap">
              <button
                className="primary"
                onClick={async () => {
                  const selected = await open({
                    directory: true,
                    multiple: false,
                  });
                  if (!selected || Array.isArray(selected)) return;
                  await withRefresh(() =>
                    invoke("add_local_destination", {
                      name: "本地目录",
                      rootPath: selected,
                    }),
                  );
                }}
              >
                添加本地目录
              </button>
              <button onClick={() => setShowS3Form((v) => !v)}>
                {showS3Form ? "收起 S3 表单" : "添加 S3"}
              </button>
            </div>
          </div>

          {showS3Form && (
            <div className="card" style={{ marginTop: 14 }}>
              <h3>S3 兼容目标</h3>
              <p className="muted">
                适用于 AWS S3、Cloudflare R2、MinIO、部分 OSS。默认 path-style。
              </p>
              <div className="form-grid">
                {(
                  [
                    ["name", "显示名", "text"],
                    ["endpoint", "Endpoint", "text"],
                    ["region", "Region", "text"],
                    ["bucket", "Bucket", "text"],
                    ["prefix", "Prefix（可选）", "text"],
                    ["accessKey", "Access Key", "text"],
                    ["secretKey", "Secret Key", "password"],
                  ] as const
                ).map(([key, label, type]) => (
                  <label key={key} className="field">
                    <span>{label}</span>
                    <input
                      type={type}
                      value={s3Form[key]}
                      onChange={(e) =>
                        setS3Form((prev) => ({ ...prev, [key]: e.target.value }))
                      }
                    />
                  </label>
                ))}
              </div>
              <div className="row-gap" style={{ marginTop: 12 }}>
                <button
                  onClick={async () => {
                    setS3TestMsg(null);
                    try {
                      const msg = await invoke<string>("test_s3_destination", {
                        endpoint: s3Form.endpoint,
                        region: s3Form.region,
                        bucket: s3Form.bucket,
                        prefix: s3Form.prefix,
                        accessKey: s3Form.accessKey,
                        secretKey: s3Form.secretKey,
                      });
                      setS3TestMsg(msg);
                    } catch (e) {
                      setS3TestMsg(String(e));
                    }
                  }}
                >
                  测试连接
                </button>
                <button
                  className="primary"
                  onClick={async () => {
                    setS3TestMsg(null);
                    await withRefresh(() =>
                      invoke("add_s3_destination", {
                        name: s3Form.name,
                        endpoint: s3Form.endpoint,
                        region: s3Form.region,
                        bucket: s3Form.bucket,
                        prefix: s3Form.prefix,
                        accessKey: s3Form.accessKey,
                        secretKey: s3Form.secretKey,
                      }),
                    );
                    setShowS3Form(false);
                    setS3Form({
                      name: "S3",
                      endpoint: "",
                      region: "auto",
                      bucket: "",
                      prefix: "",
                      accessKey: "",
                      secretKey: "",
                    });
                  }}
                >
                  保存 S3 目标
                </button>
              </div>
              {s3TestMsg && <p className="muted" style={{ marginTop: 8 }}>{s3TestMsg}</p>}
            </div>
          )}

          {boot.config.destinations.length === 0 ? (
            <p className="hint">还没有目标。添加后即可开始备份。</p>
          ) : (
            <ul className="list" style={{ marginTop: 14 }}>
              {boot.config.destinations.map((d) => (
                <li key={d.id}>
                  <div>
                    <div className="row-gap">
                      <strong>{d.name}</strong>
                      <span className="tag">{d.type}</span>
                    </div>
                    <div className="path">
                      {d.type === "local"
                        ? d.local.root_path
                        : `${d.s3.bucket} @ ${d.s3.endpoint}${
                            d.s3.prefix ? ` / ${d.s3.prefix}` : ""
                          }`}
                    </div>
                    <div className="muted">
                      实际写入：…/backups/{boot.hostname}/&lt;timestamp&gt;/
                    </div>
                  </div>
                  <div className="row-gap">
                    <label className="check small">
                      <input
                        type="checkbox"
                        checked={d.enabled}
                        onChange={(e) =>
                          withRefresh(() =>
                            invoke("set_destination_enabled", {
                              id: d.id,
                              enabled: e.target.checked,
                            }),
                          )
                        }
                      />
                      启用
                    </label>
                    <button
                      className="danger"
                      onClick={() =>
                        withRefresh(() =>
                          invoke("remove_destination", { id: d.id }),
                        )
                      }
                    >
                      删除
                    </button>
                  </div>
                </li>
              ))}
            </ul>
          )}
        </section>
      )}

      {tab === "history" && (
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
              {history.map((h) => {
                const openUri = h.destinations?.find(
                  (d) => d.status === "ok" && d.uri,
                )?.uri;
                const expanded = expandedId === h.id;
                return (
                  <li key={h.id} style={{ alignItems: "flex-start" }}>
                    <div style={{ flex: 1 }}>
                      <div className="row-gap">
                        <strong>{h.created_at}</strong>
                        <span className="tag">{h.overall_status}</span>
                        <span className="tag">{h.trigger}</span>
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
                      <button
                        onClick={() =>
                          setExpandedId(expanded ? null : h.id)
                        }
                      >
                        {expanded ? "收起" : "详情"}
                      </button>
                      {openUri && (
                        <button onClick={() => invoke("open_path", { path: openUri })}>
                          打开目录
                        </button>
                      )}
                      {h.overall_status !== "ok" && (
                        <button
                          className="primary"
                          disabled={busy}
                          onClick={retryBackup}
                        >
                          重试
                        </button>
                      )}
                    </div>
                  </li>
                );
              })}
            </ul>
          )}
        </section>
      )}

      {tab === "settings" && (
        <section className="panel">
          <h2>设置</h2>
          <div className="settings-grid">
            <div className="card">
              <h3>备份策略</h3>
              <label className="check">
                <input
                  type="checkbox"
                  checked={boot.config.schedule.enabled}
                  onChange={(e) =>
                    withRefresh(() =>
                      invoke("update_schedule", {
                        enabled: e.target.checked,
                        timeLocal: scheduleTime,
                      }),
                    )
                  }
                />
                每日定时备份
              </label>
              <div className="row-gap" style={{ marginTop: 10 }}>
                <label className="muted">时间（本地）</label>
                <input
                  type="time"
                  value={scheduleTime}
                  onChange={(e) => setScheduleTime(e.target.value)}
                />
                <button
                  onClick={() =>
                    withRefresh(() =>
                      invoke("update_schedule", {
                        enabled: boot.config.schedule.enabled,
                        timeLocal: scheduleTime,
                      }),
                    )
                  }
                >
                  保存时间
                </button>
              </div>
              <p className="muted" style={{ marginTop: 8 }}>
                托盘常驻时到点自动备份；与手动备份互斥，冲突则跳过。
              </p>

              <hr className="sep" />

              <label className="check">
                <input
                  type="checkbox"
                  checked={boot.config.encryption.enabled}
                  onChange={(e) =>
                    withRefresh(() =>
                      invoke("update_encryption", {
                        enabled: e.target.checked,
                        password: passwordDraft || boot.config.encryption.password,
                      }),
                    )
                  }
                />
                可选加密（默认开启，AES-256 Zip）
              </label>
              <div className="row-gap" style={{ marginTop: 10 }}>
                <input
                  type="password"
                  placeholder={
                    boot.config.encryption.password
                      ? "已设置密码（输入新密码可修改）"
                      : "设置加密密码"
                  }
                  value={passwordDraft}
                  onChange={(e) => setPasswordDraft(e.target.value)}
                />
                <button
                  onClick={() =>
                    withRefresh(() =>
                      invoke("update_encryption", {
                        enabled: boot.config.encryption.enabled,
                        password: passwordDraft,
                      }),
                    )
                  }
                >
                  保存密码
                </button>
              </div>

              <hr className="sep" />

              <label className="muted">自定义主机名（备份目录名）</label>
              <div className="row-gap" style={{ marginTop: 8 }}>
                <input
                  type="text"
                  placeholder={boot.hostname}
                  value={hostnameDraft}
                  onChange={(e) => setHostnameDraft(e.target.value)}
                />
                <button
                  onClick={() =>
                    withRefresh(() =>
                      invoke("update_hostname_override", {
                        hostname: hostnameDraft || null,
                      }),
                    )
                  }
                >
                  保存
                </button>
              </div>
              <p className="path">当前使用：{boot.hostname}</p>
            </div>

            <div className="card">
              <h3>应用</h3>
              <label className="check">
                <input
                  type="checkbox"
                  checked={boot.config.app.start_on_login}
                  onChange={(e) =>
                    withRefresh(() =>
                      invoke("set_start_on_login", {
                        enabled: e.target.checked,
                      }),
                    )
                  }
                />
                登录时启动
              </label>
              <p className="muted" style={{ marginTop: 8 }}>
                关闭窗口会最小化到托盘，以支持每日定时。
              </p>
              <p className="muted">版本 {version}</p>

              <hr className="sep" />

              <h3>配置位置</h3>
              <p className="path">{boot.config_dir}</p>
              <div className="row-gap">
                <button onClick={() => invoke("open_config_dir")}>
                  打开配置目录
                </button>
                <button onClick={() => invoke("open_presets_file")}>
                  打开预设目录
                </button>
                <button
                  onClick={() =>
                    withRefresh(() => invoke("reset_presets")).then(() =>
                      setError(null),
                    )
                  }
                >
                  重置 Agent 预设
                </button>
              </div>
            </div>

            <div className="card">
              <h3>配置说明</h3>
              <pre className="docs">{docs || "文档加载中…"}</pre>
            </div>
          </div>
        </section>
      )}
    </div>
  );
}
