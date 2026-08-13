# Agent Backup 配置说明

本文说明如何通过配置文件扩展备份源、调整路径与勾选行为，而无需改代码。

## 配置文件位置

应用配置目录（设置页可查看完整路径）：

| 文件 | 作用 |
|---|---|
| `config.json` | 勾选状态、目标、定时、加密等运行时偏好 |
| `agent-presets.json` | 用户自定义/覆盖 Agent 探测与路径分组（可选） |
| `history.jsonl` | 本地备份运行历史（应用自动写入） |

内置默认预设打包在应用内：`agent-presets.default.json`。  
若用户配置目录没有 `agent-presets.json`，则完全使用内置默认。

## 合并规则

1. 加载内置 `agent-presets.default.json`
2. 若存在用户 `agent-presets.json`，按 `agents[].key` 合并
3. 用户提供了某个 Agent 的 `groups` 时：**整体替换**该 Agent 的 groups（不做数组合并）
4. 用户可为某内置 Agent 设置 `"disabled": true` 以隐藏
5. 用户可新增任意 `key`，用来支持新工具

`config.json` 里的 `sources.agents.<key>.paths.<groupId>` 只表示勾选；路径含义以 presets 为准。

- presets 新增 group、config 尚无该键 → 使用 `default_enabled`
- config 有键、presets 已删除 → 忽略

## agent-presets.json 示例

```json
{
  "version": 1,
  "agents": [
    {
      "key": "my-tool",
      "label": "My Tool",
      "disabled": false,
      "root": {
        "kind": "home_subdir",
        "win": ".my-tool",
        "mac": ".my-tool",
        "env_override": "MY_TOOL_CONFIG_DIR"
      },
      "groups": [
        {
          "id": "config",
          "label": "配置",
          "default_enabled": true,
          "include": ["settings.json", "config.toml"]
        },
        {
          "id": "skills",
          "label": "Skills",
          "default_enabled": true,
          "include": ["skills/"]
        },
        {
          "id": "sessions",
          "label": "会话",
          "default_enabled": false,
          "include": ["sessions/"]
        }
      ],
      "hard_exclude": ["credentials/", "*.key", "cache/"]
    }
  ]
}
```

### 字段说明

| 字段 | 说明 |
|---|---|
| `key` | 稳定 ID，用于 config 勾选与 zip 文件名 |
| `label` | UI 显示名 |
| `disabled` | `true` 时不探测、不显示 |
| `root.kind` | 目前仅支持 `home_subdir`（相对用户主目录） |
| `root.win` / `root.mac` | 相对主目录的子路径 |
| `root.env_override` | 若该环境变量有值，优先作为根目录 |
| `groups[].id` | 勾选键，对应 `config.sources.agents.<key>.paths.<id>` |
| `groups[].include` | 相对根的文件或目录；目录请以 `/` 结尾 |
| `hard_exclude` | 该 Agent 打包时始终排除（先于全局 exclusions） |

## config.json 要点

- `destinations[]`：支持 `local`（本地目录）与 `s3`（S3 兼容：R2 / MinIO / AWS 等）
- 本地目标写入：`{root_path}/backups/{hostname}/{timestamp}/`
- S3 目标写入：`s3://{bucket}/{prefix}/backups/{hostname}/{timestamp}/`（path-style）
- 多目标尽力而为：一个失败不影响其他已成功目标
- `encryption.enabled` 默认 `true`；开启时 zip 使用 **AES-256** 密码保护（WinRAR / 7-Zip 等可解压）
- 密码明文保存在 `config.json`（按产品决策）
- `schedule.enabled` + `schedule.time_local`（本地 `HH:mm`）：托盘常驻时每日到点备份
- `app.start_on_login`：登录时启动
- `sources.custom[]`：自定义文件夹源，不依赖 presets

自定义源示例：

```json
{
  "id": "custom_xxxx",
  "label": "my-extra",
  "path": "D:\\\\notes\\\\agent",
  "enabled": true
}
```

## 备份产物

```
{root}/backups/{hostname}/{timestamp}/
  manifest.json
  cursor_{timestamp}.zip
  claude-code_{timestamp}.zip
  ...
```

- 每个 Agent / 自定义源单独一个 zip
- 文件夹名与文件名都带 UTC 时间戳：`YYYY-MM-DDTHHMMSSZ`
- `manifest.json` 明文，记录源、校验和与各目标结果

## 常见操作

### 只改某个内置 Agent 的路径

在用户 `agent-presets.json` 中写入同名 `key`，并提供完整 `groups`（会替换内置 groups）。

### 临时关掉某个内置 Agent

```json
{
  "version": 1,
  "agents": [
    { "key": "workbuddy", "disabled": true }
  ]
}
```

（仅含 `key` + `disabled` 时，其它字段仍继承默认。）

### 恢复官方预设

删除用户配置目录中的 `agent-presets.json`，或在设置页使用「重置为默认」。

## 安全提示

- 默认排除密钥类文件与缓存；仍请检查自定义 `include`，避免把 `credentials/`、`auth.json`、`.env` 打进备份
- 加密密码与 S3 密钥保存在 `config.json`；请保护好本机用户目录
- 第一版为单向备份，不提供应用内恢复向导；需要时请手工解压 zip
