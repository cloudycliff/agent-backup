import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { Bootstrap, WithRefresh } from "../types";

type Props = { boot: Bootstrap; withRefresh: WithRefresh };

const REPO_URL = "https://github.com/cloudycliff/agent-backup";

export default function SettingsTab({ boot, withRefresh }: Props) {
  const { config } = boot;
  const [passwordDraft, setPasswordDraft] = useState("");
  const [hostnameDraft, setHostnameDraft] = useState(config.app.hostname_override ?? "");
  const [scheduleTime, setScheduleTime] = useState(config.schedule.time_local || "21:00");
  const [docs, setDocs] = useState("");
  const [version, setVersion] = useState("");

  useEffect(() => {
    invoke<string>("get_docs_hint").then(setDocs).catch(() => undefined);
    invoke<string>("get_app_version").then(setVersion).catch(() => undefined);
  }, []);

  const saveSchedule = (enabled: boolean) =>
    withRefresh(() => invoke("update_schedule", { enabled, timeLocal: scheduleTime }));

  return (
    <section className="panel">
      <h2>设置</h2>
      <div className="settings-grid">
        <div className="card">
          <h3>备份策略</h3>
          <label className="check">
            <input
              type="checkbox"
              checked={config.schedule.enabled}
              onChange={(e) => saveSchedule(e.target.checked)}
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
            <button onClick={() => saveSchedule(config.schedule.enabled)}>保存时间</button>
          </div>
          <p className="muted" style={{ marginTop: 8 }}>
            托盘常驻时到点自动备份；错过时间（睡眠或关机）会在下次运行时补跑一次。与手动备份互斥，冲突时稍后再试。
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
            可选加密（默认开启，AES-256 Zip）
          </label>
          <div className="row-gap" style={{ marginTop: 10 }}>
            <input
              type="password"
              placeholder={
                config.encryption.password
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
                    enabled: config.encryption.enabled,
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

        <ExclusionsCard boot={boot} withRefresh={withRefresh} />

        <div className="card">
          <h3>应用</h3>
          <label className="check">
            <input
              type="checkbox"
              checked={config.app.start_on_login}
              onChange={(e) =>
                withRefresh(() =>
                  invoke("set_start_on_login", { enabled: e.target.checked }),
                )
              }
            />
            登录时启动
          </label>
          <p className="muted" style={{ marginTop: 8 }}>
            关闭窗口会最小化到托盘，以支持每日定时。
          </p>
          <p className="muted">
            版本 {version} ·{" "}
            <a
              href={REPO_URL}
              onClick={(e) => {
                e.preventDefault();
                openUrl(REPO_URL);
              }}
            >
              GitHub 主页
            </a>
          </p>

          <hr className="sep" />

          <h3>配置位置</h3>
          <p className="path">{boot.config_dir}</p>
          <div className="row-gap">
            <button onClick={() => invoke("open_config_dir")}>打开配置目录</button>
            <button onClick={() => invoke("open_log_dir")}>打开日志目录</button>
            <button onClick={() => invoke("open_presets_file")}>打开预设目录</button>
            <button onClick={() => withRefresh(() => invoke("reset_presets"))}>
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
  );
}

const toLines = (list: string[]) => list.join("\n");
const fromLines = (text: string) => text.split(/\r?\n/);

function ExclusionsCard({ boot, withRefresh }: Props) {
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
      <h3>排除规则</h3>
      <p className="muted">
        对所有源生效，每行一条，支持 <code>*</code> 通配。文件规则匹配文件名或相对路径，不含 <code>/</code> 的文件规则也会排除同名目录；目录规则匹配目录名，命中后整个目录跳过。各 Agent 预设里的排除项另外生效。
      </p>
      <div className="form-grid">
        <label className="field">
          <span>文件</span>
          <textarea rows={7} value={files} onChange={(e) => setFiles(e.target.value)} />
        </label>
        <label className="field">
          <span>目录</span>
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
          保存规则
        </button>
        <button onClick={() => withRefresh(() => invoke("reset_exclusions"))}>
          恢复默认
        </button>
      </div>
    </div>
  );
}
