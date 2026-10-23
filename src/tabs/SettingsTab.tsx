import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useI18n, withCode } from "../i18n";
import type { Bootstrap, WithRefresh } from "../types";

type Props = { boot: Bootstrap; withRefresh: WithRefresh };

const REPO_URL = "https://github.com/cloudycliff/agent-backup";

export default function SettingsTab({ boot, withRefresh }: Props) {
  const { t } = useI18n();
  const { config } = boot;
  const [passwordDraft, setPasswordDraft] = useState("");
  const [hostnameDraft, setHostnameDraft] = useState(config.app.hostname_override ?? "");
  const [scheduleTime, setScheduleTime] = useState(config.schedule.time_local || "21:00");
  const [docs, setDocs] = useState("");
  const [version, setVersion] = useState("");

  useEffect(() => {
    invoke<string>("get_app_version").then(setVersion).catch(() => undefined);
  }, []);

  useEffect(() => {
    invoke<string>("get_docs_hint").then(setDocs).catch(() => undefined);
  }, [boot.language]);

  const saveSchedule = (enabled: boolean) =>
    withRefresh(() => invoke("update_schedule", { enabled, timeLocal: scheduleTime }));

  return (
    <section className="panel">
      <h2>{t("settings.title")}</h2>
      <div className="settings-grid">
        <div className="card">
          <h3>{t("settings.policy")}</h3>
          <label className="check">
            <input
              type="checkbox"
              checked={config.schedule.enabled}
              onChange={(e) => saveSchedule(e.target.checked)}
            />
            {t("settings.daily")}
          </label>
          <div className="row-gap" style={{ marginTop: 10 }}>
            <label className="muted">{t("settings.timeLocal")}</label>
            <input
              type="time"
              value={scheduleTime}
              onChange={(e) => setScheduleTime(e.target.value)}
            />
            <button onClick={() => saveSchedule(config.schedule.enabled)}>
              {t("settings.saveTime")}
            </button>
          </div>
          <p className="muted" style={{ marginTop: 8 }}>
            {t("settings.scheduleHint")}
          </p>

          <hr className="sep" />

          <label className="check">
            <input
              type="checkbox"
              checked={config.encryption.enabled}
              onChange={(e) =>
                withRefresh(() =>
                  invoke("update_encryption", {
                    enabled: e.target.checked,
                    password: passwordDraft || config.encryption.password,
                  }),
                )
              }
            />
            {t("settings.encryption")}
          </label>
          <div className="row-gap" style={{ marginTop: 10 }}>
            <input
              type="password"
              placeholder={
                config.encryption.password
                  ? t("settings.passwordSet")
                  : t("settings.passwordNew")
              }
              value={passwordDraft}
              onChange={(e) => setPasswordDraft(e.target.value)}
            />
            <button
              onClick={() =>
                withRefresh(() =>
                  invoke("update_encryption", {
                    enabled: config.encryption.enabled,
                    password: passwordDraft,
                  }),
                )
              }
            >
              {t("settings.savePassword")}
            </button>
          </div>

          <hr className="sep" />

          <label className="muted">{t("settings.hostname")}</label>
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
              {t("common.save")}
            </button>
          </div>
          <p className="path">{t("settings.hostnameInUse", { host: boot.hostname })}</p>
        </div>

        <ExclusionsCard boot={boot} withRefresh={withRefresh} />

        <div className="card">
          <h3>{t("settings.app")}</h3>
          <div className="row-gap">
            <label className="muted">{t("settings.language")}</label>
            <select
              value={config.app.language}
              onChange={(e) =>
                withRefresh(() => invoke("set_language", { language: e.target.value }))
              }
            >
              <option value="auto">{t("settings.languageAuto")}</option>
              <option value="zh-CN">中文</option>
              <option value="en">English</option>
            </select>
          </div>
          <label className="check" style={{ marginTop: 10 }}>
            <input
              type="checkbox"
              checked={config.app.start_on_login}
              onChange={(e) =>
                withRefresh(() =>
                  invoke("set_start_on_login", { enabled: e.target.checked }),
                )
              }
            />
            {t("settings.startOnLogin")}
          </label>
          <p className="muted" style={{ marginTop: 8 }}>
            {t("settings.trayHint")}
          </p>
          <p className="muted">
            {t("settings.version", { version })} ·{" "}
            <a
              href={REPO_URL}
              onClick={(e) => {
                e.preventDefault();
                openUrl(REPO_URL);
              }}
            >
              {t("settings.github")}
            </a>
          </p>

          <hr className="sep" />

          <h3>{t("settings.configLocation")}</h3>
          <p className="path">{boot.config_dir}</p>
          <div className="row-gap">
            <button onClick={() => invoke("open_config_dir")}>{t("settings.openConfig")}</button>
            <button onClick={() => invoke("open_log_dir")}>{t("settings.openLogs")}</button>
            <button onClick={() => invoke("open_presets_file")}>
              {t("settings.openPresets")}
            </button>
            <button onClick={() => withRefresh(() => invoke("reset_presets"))}>
              {t("settings.resetPresets")}
            </button>
          </div>
        </div>

        <div className="card">
          <h3>{t("settings.docs")}</h3>
          <pre className="docs">{docs || t("settings.docsLoading")}</pre>
        </div>
      </div>
    </section>
  );
}

const toLines = (list: string[]) => list.join("\n");
const fromLines = (text: string) => text.split(/\r?\n/);

function ExclusionsCard({ boot, withRefresh }: Props) {
  const { t } = useI18n();
  const savedFiles = toLines(boot.config.exclusions.file_name_globs);
  const savedDirs = toLines(boot.config.exclusions.dir_name_globs);
  const [files, setFiles] = useState(savedFiles);
  const [dirs, setDirs] = useState(savedDirs);

  useEffect(() => {
    setFiles(savedFiles);
    setDirs(savedDirs);
  }, [savedFiles, savedDirs]);

  return (
    <div className="card">
      <h3>{t("exclusions.title")}</h3>
      <p className="muted">{withCode(t("exclusions.hint"))}</p>
      <div className="form-grid">
        <label className="field">
          <span>{t("exclusions.files")}</span>
          <textarea rows={7} value={files} onChange={(e) => setFiles(e.target.value)} />
        </label>
        <label className="field">
          <span>{t("exclusions.dirs")}</span>
          <textarea rows={7} value={dirs} onChange={(e) => setDirs(e.target.value)} />
        </label>
      </div>
      <div className="row-gap" style={{ marginTop: 10 }}>
        <button
          className="primary"
          onClick={() =>
            withRefresh(() =>
              invoke("update_exclusions", {
                fileNameGlobs: fromLines(files),
                dirNameGlobs: fromLines(dirs),
              }),
            )
          }
        >
          {t("exclusions.save")}
        </button>
        <button onClick={() => withRefresh(() => invoke("reset_exclusions"))}>
          {t("exclusions.reset")}
        </button>
      </div>
    </div>
  );
}
