import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import type { Bootstrap, Destination, WithRefresh } from "../types";

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

type LocalEdit = { id: string; name: string; rootPath: string };

type Props = { boot: Bootstrap; withRefresh: WithRefresh };

async function pickDirectory() {
  const selected = await open({ directory: true, multiple: false });
  return !selected || Array.isArray(selected) ? null : selected;
}

export default function DestinationsTab({ boot, withRefresh }: Props) {
  const [showS3Form, setShowS3Form] = useState(false);
  /** Set while the S3 form edits an existing destination instead of adding one. */
  const [editingS3Id, setEditingS3Id] = useState<string | null>(null);
  const [s3TestMsg, setS3TestMsg] = useState<string | null>(null);
  const [s3Form, setS3Form] = useState(EMPTY_S3_FORM);
  const [localEdit, setLocalEdit] = useState<LocalEdit | null>(null);

  async function addLocal() {
    const rootPath = await pickDirectory();
    if (!rootPath) return;
    await withRefresh(() => invoke("add_local_destination", { name: "本地目录", rootPath }));
  }

  function closeS3Form() {
    setShowS3Form(false);
    setEditingS3Id(null);
    setS3Form(EMPTY_S3_FORM);
    setS3TestMsg(null);
  }

  function editLocal(d: Extract<Destination, { type: "local" }>) {
    closeS3Form();
    setLocalEdit({ id: d.id, name: d.name, rootPath: d.local.root_path });
  }

  function editS3(d: Extract<Destination, { type: "s3" }>) {
    setLocalEdit(null);
    setEditingS3Id(d.id);
    setS3TestMsg(null);
    setS3Form({
      name: d.name,
      endpoint: d.s3.endpoint ?? "",
      region: d.s3.region ?? "",
      bucket: d.s3.bucket ?? "",
      prefix: d.s3.prefix ?? "",
      accessKey: d.s3.access_key ?? "",
      secretKey: "",
    });
    setShowS3Form(true);
  }

  async function testS3() {
    setS3TestMsg(null);
    try {
      const { name: _name, ...args } = s3Form;
      setS3TestMsg(
        await invoke<string>("test_s3_destination", { id: editingS3Id, ...args }),
      );
    } catch (e) {
      setS3TestMsg(String(e));
    }
  }

  async function saveS3() {
    setS3TestMsg(null);
    await withRefresh(() =>
      editingS3Id
        ? invoke("update_s3_destination", { id: editingS3Id, ...s3Form })
        : invoke("add_s3_destination", s3Form),
    );
    closeS3Form();
  }

  async function saveLocal(edit: LocalEdit) {
    await withRefresh(() => invoke("update_local_destination", edit));
    setLocalEdit(null);
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
          <button onClick={() => (showS3Form ? closeS3Form() : setShowS3Form(true))}>
            {showS3Form ? "收起 S3 表单" : "添加 S3"}
          </button>
        </div>
      </div>

      {showS3Form && (
        <div className="card" style={{ marginTop: 14 }}>
          <h3>{editingS3Id ? "编辑 S3 目标" : "S3 兼容目标"}</h3>
          <p className="muted">
            适用于 AWS S3、Cloudflare R2、MinIO、部分 OSS。默认 path-style。
            {editingS3Id && "修改 Endpoint / Bucket / Prefix 后，下一次定时备份会完整写入新位置。"}
          </p>
          <div className="form-grid">
            {S3_FIELDS.map(([key, label, type]) => (
              <label key={key} className="field">
                <span>{label}</span>
                <input
                  type={type}
                  value={s3Form[key]}
                  placeholder={
                    key === "secretKey" && editingS3Id ? "留空表示不修改" : undefined
                  }
                  onChange={(e) =>
                    setS3Form((prev) => ({ ...prev, [key]: e.target.value }))
                  }
                />
              </label>
            ))}
          </div>
          <div className="row-gap" style={{ marginTop: 12 }}>
            <button onClick={testS3}>测试写入</button>
            <button className="primary" onClick={saveS3}>
              {editingS3Id ? "保存修改" : "保存 S3 目标"}
            </button>
            {editingS3Id && <button onClick={closeS3Form}>取消</button>}
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
          {boot.config.destinations.map((d) =>
            localEdit?.id === d.id ? (
              <li key={d.id}>
                <div style={{ flex: 1 }}>
                  <div className="row-gap">
                    <input
                      type="text"
                      value={localEdit.name}
                      onChange={(e) => setLocalEdit({ ...localEdit, name: e.target.value })}
                    />
                    <button
                      onClick={async () => {
                        const rootPath = await pickDirectory();
                        if (rootPath) setLocalEdit({ ...localEdit, rootPath });
                      }}
                    >
                      更换目录
                    </button>
                  </div>
                  <div className="path">{localEdit.rootPath}</div>
                </div>
                <div className="row-gap">
                  <button className="primary" onClick={() => saveLocal(localEdit)}>
                    保存
                  </button>
                  <button onClick={() => setLocalEdit(null)}>取消</button>
                </div>
              </li>
            ) : (
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
                  <button onClick={() => (d.type === "local" ? editLocal(d) : editS3(d))}>
                    编辑
                  </button>
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
            ),
          )}
        </ul>
      )}
    </section>
  );
}
