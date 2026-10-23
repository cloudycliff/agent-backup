import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import type { Bootstrap, WithRefresh } from "../types";

const EMPTY_S3_FORM = {
  name: "S3",
  endpoint: "",
  region: "auto",
  bucket: "",
  prefix: "",
  accessKey: "",
  secretKey: "",
};

const S3_FIELDS = [
  ["name", "显示名", "text"],
  ["endpoint", "Endpoint", "text"],
  ["region", "Region", "text"],
  ["bucket", "Bucket", "text"],
  ["prefix", "Prefix（可选）", "text"],
  ["accessKey", "Access Key", "text"],
  ["secretKey", "Secret Key", "password"],
] as const;

type Props = { boot: Bootstrap; withRefresh: WithRefresh };

export default function DestinationsTab({ boot, withRefresh }: Props) {
  const [showS3Form, setShowS3Form] = useState(false);
  const [s3TestMsg, setS3TestMsg] = useState<string | null>(null);
  const [s3Form, setS3Form] = useState(EMPTY_S3_FORM);

  async function addLocal() {
    const selected = await open({ directory: true, multiple: false });
    if (!selected || Array.isArray(selected)) return;
    await withRefresh(() =>
      invoke("add_local_destination", { name: "本地目录", rootPath: selected }),
    );
  }

  async function testS3() {
    setS3TestMsg(null);
    try {
      const { name: _name, ...args } = s3Form;
      setS3TestMsg(await invoke<string>("test_s3_destination", args));
    } catch (e) {
      setS3TestMsg(String(e));
    }
  }

  async function saveS3() {
    setS3TestMsg(null);
    await withRefresh(() => invoke("add_s3_destination", s3Form));
    setShowS3Form(false);
    setS3Form(EMPTY_S3_FORM);
  }

  return (
    <section className="panel">
      <div className="row-between">
        <div>
          <h2>备份目标</h2>
          <p className="muted">
            支持多个目标：本地目录 + S3 兼容存储。一次备份会尽力写入全部启用目标。
          </p>
        </div>
        <div className="row-gap">
          <button className="primary" onClick={addLocal}>
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
            {S3_FIELDS.map(([key, label, type]) => (
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
            <button onClick={testS3}>测试连接</button>
            <button className="primary" onClick={saveS3}>
              保存 S3 目标
            </button>
          </div>
          {s3TestMsg && (
            <p className="muted" style={{ marginTop: 8 }}>
              {s3TestMsg}
            </p>
          )}
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
                    withRefresh(() => invoke("remove_destination", { id: d.id }))
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
  );
}
