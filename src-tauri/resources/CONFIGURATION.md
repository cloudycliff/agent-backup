# Agent Backup 配置说明

本文说明如何通过配置文件扩展备份源、调整路径与勾选行为，而无需改代码。

## 配置文件位置

应用配置目录（设置页可查看完整路径）：

| 文件 | 作用 |
|---|---|
| `config.json` | 勾选状态、目标、定时、加密等运行时偏好 |
| `logs/agent-backup.log` | 运行日志（备份开始/结束、失败原因、定时事件），超过 1 MB 轮转为 `agent-backup.1.log` |
| `agent-presets.json` | 用户自定义/覆盖 Agent 探测与路径分组（可选） |
| `history.jsonl` | 本地备份运行历史（应用自动写入） |

内置默认预设打包在应用内：`agent-presets.default.json`。  
若用户配置目录没有 `agent-presets.json`，则完全使用内置默认。

内置覆盖的 Agent（按 `key`）：`cursor`、`cursor-cli`、`claude-code`、`codex`、`workbuddy`、`windsurf`、`continue`、`cline`、`kiro`、`aider`、`gemini`、`shared-agents`，以及编辑器用户设置 `cursor-editor`、`windsurf-editor`、`kiro-editor`。  
`root.win` / `root.mac` 支持多级路径，例如 Windsurf 用 `.codeium/windsurf`；留空表示根目录本身（Aider 的配置文件就直接放在主目录下）。

编辑器用户设置（`settings.json`、`keybindings.json`、`snippets/`、`profiles/` 等）不在 `~/.xxx` 下，而在系统应用数据目录：Windows 为 `%APPDATA%\<产品>\User`，macOS 为 `~/Library/Application Support/<产品>/User`，所以单独作为 `app_data` 类型的预设。

打包时会跟随软链接和目录联接（Junction），链接目录里的内容按链接所在位置写入 zip，循环链接会被跳过。`shared-agents` 对应 `~/.agents/skills`：很多 skills 管理工具把技能放在这里，再链接到各个 Agent 的 `skills/` 下。

`cursor-cli` 与 `cursor` 共用 `~/.cursor` 根目录，只收 CLI 独有的 `cli-config.json`、`agent-cli-state.json` 和会话 `chats/`（默认不勾，体积可能很大）。两者共用的 rules / skills / `mcp.json` / `hooks.json` 由 `cursor` 预设负责，避免重复打包；只用 CLI 时也请保留 `cursor` 的勾选。CLI 程序本体（Windows 的 `%LOCALAPPDATA%\cursor-agent`、macOS 的 `~/.local/share/cursor-agent`）不备份，重装即可；登录凭据在系统钥匙串中，也不会被打包。

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
      "hard_exclude": ["credentials/", "*.key", "cache/"],
      "detect": ["settings.json"]
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
| `root.kind` | `home_subdir`：相对用户主目录；`app_data`：相对系统应用数据目录（Windows `%APPDATA%`，macOS `~/Library/Application Support`） |
| `root.win` / `root.mac` | 相对上述基准目录的子路径；留空即基准目录本身 |
| `root.env_override` | 若该环境变量有值，优先作为根目录 |
| `groups[].id` | 勾选键，对应 `config.sources.agents.<key>.paths.<id>` |
| `groups[].include` | 相对根的文件或目录；目录请以 `/` 结尾 |
| `hard_exclude` | 该 Agent 打包时始终排除（先于全局 exclusions）；以 `/` 结尾的名字会排除任意层级的同名目录 |
| `detect` | 可选。根目录下任一路径存在才算"已安装"；不填则根目录存在即算。多个 Agent 共用根目录时用来区分 |

`groups[].include` 中显式写出的目录不受全局排除名影响（例如 Gemini 的 `tmp/`），但其内部的同名子目录仍会被排除。

## config.json 要点

- `destinations[]`：支持 `local`（本地目录）与 `s3`（S3 兼容：R2 / MinIO / AWS 等）
- 本地目标写入：`{root_path}/backups/{hostname}/{timestamp}/`
- S3 目标写入：`s3://{bucket}/{prefix}/backups/{hostname}/{timestamp}/`（path-style）
- S3"测试写入"会在 `{prefix}/` 下上传一个 `.agent-backup-write-test` 小文件再尝试删除，只有上传权限（PutObject）的密钥也能通过；没有删除权限时测试文件会留下，可手动删除
- 目标可在界面编辑：改名不影响历史；更换本地目录或 S3 的 Endpoint / Bucket / Prefix 视为新目标，下次定时备份即使内容没变也会完整写入新位置。编辑 S3 时 Secret Key 留空表示不修改
- `exclusions.file_name_globs` / `exclusions.dir_name_globs`：全局排除规则，可在设置页编辑（每行一条，保存时校验语法并去重），也可一键恢复默认
- 多目标尽力而为：一个失败不影响其他已成功目标
- `encryption.enabled` 默认 `true`；开启时 zip 使用 **AES-256** 密码保护（WinRAR / 7-Zip 等可解压）
- 密码明文保存在 `config.json`（按产品决策）；首尾空格属于密码的一部分。开启加密但未设密码时不能开始备份
- `sources.custom[].enabled`：自定义源可在界面单独开关，关闭后保留配置但不备份
- `schedule.enabled` + `schedule.time_local`（本地 `HH:mm`）：托盘常驻时每日到点备份；错过的时间点（睡眠、关机、应用未运行）会在下次运行时补跑一次，不会累计补多次
- 配置文件采用"写临时文件再改名"的方式保存；缺失的字段按默认值补齐，旧版配置可直接使用
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
- `manifest.json` 明文，记录源、校验和与各目标结果：
  - `sources[].sha256`：内容摘要，按 zip 内相对路径和每个文件的 SHA-256 计算，与是否加密无关，内容不变则不变
  - `sources[].archive_sha256`：zip 文件本身的 SHA-256，可用来校验下载或拷贝后的归档
- 打包时总会跳过本地目标的 `{root}/backups/` 目录，所以把目标放在某个源里面也不会把旧备份再打包一遍；位于 `backups/` 内的文件夹不能作为自定义源
- 本地目标先写到 `{timestamp}.partial/`，全部写完再改名；出错时删除半成品，目标目录里只会出现完整的备份
- 定时备份时，如果每个源的内容摘要都与最近一次成功备份相同（同一主机、同一组目标、加密设置一致、本地目标里那份备份还在，且不超过 7 天），本次不写入新副本，只在历史里记一条"内容相同"。手动备份总是写入；有源失败或有警告时也总是写入
- 单个文件读不了（被占用、无权限、链接失效、循环链接）不会让整个源失败：该文件被跳过，原因记在 `sources[].warnings`（每个源最多 20 条），整体状态记为 `partial` 并发通知
- 用户 `agent-presets.json` 解析失败时回退到内置预设；单个 Agent 缺 `root` 或 `root.kind` 不支持时只跳过该 Agent。两种情况都会在界面顶部提示

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
