import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import ProgressCard from "../components/ProgressCard";
import StatusLine from "../components/StatusLine";
import { formatBytes } from "../format";
import { useI18n } from "../i18n";
import type {
  AgentView,
  Bootstrap,
  Estimate,
  Progress,
  StatusView,
  WithRefresh,
} from "../types";

type Props = {
  boot: Bootstrap;
  busy: boolean;
  estimate: Estimate | null;
  progress: Progress | null;
  status: StatusView | null;
  withRefresh: WithRefresh;
  onBackup: () => void;
};

export default function BackupTab({
  boot,
  busy,
  estimate,
  progress,
  status,
  withRefresh,
  onBackup,
}: Props) {
  const { t } = useI18n();
  const enabledDestCount = boot.config.destinations.filter((d) => d.enabled).length;
  const missingPassword =
    boot.config.encryption.enabled && !boot.config.encryption.password;

  async function addCustomSource() {
    const selected = await open({ directory: true, multiple: false });
    if (!selected || Array.isArray(selected)) return;
    const label = selected.split(/[/\\]/).filter(Boolean).pop() ?? "custom";
    await withRefresh(() => invoke("add_custom_source", { label, path: selected }));
  }

  return (
    <section className="panel">
      <div className="row-between">
        <div>
          <h2>{t("backup.title")}</h2>
          <p className="muted">{t("backup.subtitle")}</p>
        </div>
        <button
          className="primary"
          disabled={busy || enabledDestCount === 0 || missingPassword}
          onClick={onBackup}
        >
          {busy ? t("backup.running") : t("backup.now")}
        </button>
      </div>
      {enabledDestCount === 0 && <p className="hint">{t("backup.noDestination")}</p>}
      {missingPassword && <p className="hint">{t("backup.missingPassword")}</p>}
      {status && <StatusLine status={status} />}
      <p className="muted">
        {t("backup.willWrite", { count: enabledDestCount })}
        {estimate
          ? t("backup.estimate", {
              files: estimate.file_count,
              size: formatBytes(estimate.bytes),
            })
          : ""}
      </p>
      {progress && <ProgressCard progress={progress} />}
      <div className="cards">
        {boot.agents.map((agent) => (
          <AgentCard key={agent.key} agent={agent} withRefresh={withRefresh} />
        ))}
      </div>

      <div className="section-head">
        <h3>{t("backup.customTitle")}</h3>
        <button onClick={addCustomSource}>{t("backup.addFolder")}</button>
      </div>
      {boot.config.sources.custom.length === 0 ? (
        <p className="muted">{t("backup.noCustom")}</p>
      ) : (
        <ul className="list">
          {boot.config.sources.custom.map((c) => (
            <li key={c.id}>
              <label className="check">
                <input
                  type="checkbox"
                  checked={c.enabled}
                  onChange={(e) =>
                    withRefresh(() =>
                      invoke("set_custom_source_enabled", {
                        id: c.id,
                        enabled: e.target.checked,
                      }),
                    )
                  }
                />
                <div>
                  <strong>{c.label}</strong>
                  <div className="path">{c.path}</div>
                </div>
              </label>
              <button
                className="danger"
                onClick={() =>
                  withRefresh(() => invoke("remove_custom_source", { id: c.id }))
                }
              >
                {t("common.delete")}
              </button>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

function AgentCard({
  agent,
  withRefresh,
}: {
  agent: AgentView;
  withRefresh: WithRefresh;
}) {
  const { t } = useI18n();
  return (
    <article className={agent.installed ? "card" : "card dim"}>
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
          {agent.installed ? t("agent.installed") : t("agent.notDetected")}
        </span>
      </div>
      {agent.root_path && <p className="path">{agent.root_path}</p>}
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
  );
}
