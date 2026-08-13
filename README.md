# Agent Backup

同事的电脑硬盘坏了。  
他用各种 Agent 攒了很久的 skills、rules、对话和习惯配置，一夜之间没了。

那种损失很难补回来——不是代码仓库能 `git clone` 的东西，而是散落在 `~/.cursor`、`~/.claude`、`~/.codex` 里、平时几乎不会想到要备份的个人资产。

所以有了这个项目：把本机已经装好的 Agent 配置，**单向备份**到本地目录或 S3 兼容存储。不追求花哨的双向同步，先保证「换机 / 翻车时还有一份」。

目前支持：**Windows + macOS**。内置探测 Cursor、Claude Code、Codex、Workbuddy、Windsurf、Continue、Cline、Kiro、Aider、Gemini CLI；也可以自己加文件夹。

备份时会先做**体积预估**，过程中显示进度（按源打包 / 按目标写入）。

---

## 它能做什么

- 自动探测本机装了哪些 Agent，按组勾选要备份的内容  
- 默认备份配置 / rules / skills；会话默认不勾；密钥与缓存默认排除  
- 每个 Agent（以及自定义源）打成**单独的 zip**；目录名和文件名都带时间戳  
- 按主机名分目录，多台机器共用一个网盘也不会糊在一起  
- 目标可以是本地文件夹（含 OneDrive / iCloud 等已挂载盘），也可以是 S3 / R2 / MinIO  
- 多目标尽力而为：一个挂了，别的照样写  
- 可选 Zip AES-256 加密（默认开，需设密码）  
- 手动备份 + 每日定时；托盘常驻、可登录启动  
- 失败才系统通知；有历史记录，支持整次重跑  
- 备份前体积预估；备份中显示进度条（打包 / 写入目标）  

产物大致长这样：

```text
{目标根}/backups/{hostname}/{timestamp}/
  manifest.json
  cursor_{timestamp}.zip
  claude-code_{timestamp}.zip
  ...
```

更细的配置（含 presets 合并规则）见 [docs/CONFIGURATION.md](docs/CONFIGURATION.md)。

---

## 怎么跑

```bash
cd agent-backup
npm install
npm run tauri dev
```

第一次使用建议顺序：

1. **设置**里设好加密密码（或关掉加密）  
2. **目标**里加一个本地目录  
3. **备份**页勾选源，点「立即备份」  
4. 到**记录**页确认结果  

配置文件在：`~/.agent-backup/`（Windows：`%USERPROFILE%\.agent-backup\`）。  
关窗口会进托盘，不会退出——这是定时备份能跑的前提。真要退出，用托盘菜单。

---

## 给后续开发的 Agent / 协作者

下面这段是写给会改这个仓库的 Agent（和人）看的，尽量把「已经拍板的决策」和「代码落点」说清楚，减少返工。

### 产品边界（不要擅自推翻）

| 已定 | 含义 |
|---|---|
| 单向备份 | 不做双向同步、不做冲突合并 |
| 第一版不做应用内恢复 | 用户自行解压 / 拷回；`manifest.json` 是给以后恢复用的 |
| 仅用户级目录 | 不扫项目内 `.cursor`；特例用「自定义文件夹」 |
| 加密可选、默认开 | 密码在 `config.json` 明文（有意为之，简单优先） |
| 版本无限保留 | 不做自动清理策略 |
| 成功静默、失败才通知 | 定时场景不要天天弹窗 |
| 失败重试 = 整次重跑 | 暂不做「只补传失败目标」 |

### 技术栈与目录

- **壳**：Tauri 2（Rust）+ React + TypeScript  
- **前端**：`src/`（目前基本是单页 `App.tsx`）  
- **后端**：`src-tauri/src/`  
  - `config.rs` — `config.json` 读写、主机名、路径消毒  
  - `presets.rs` — 内置 + 用户 presets 合并、根目录解析  
  - `packager.rs` — 收文件、排除规则、打 zip（可 AES）  
  - `backup.rs` — 编排：打包 → 多目标写入 → manifest → history  
  - `s3.rs` — S3 兼容上传 / 测连  
  - `scheduler.rs` — 每日定时（托盘进程内轮询）  
  - `history.rs` / `notify.rs` / `commands.rs` — 记录、通知、IPC  
- **内置预设**：`src-tauri/resources/agent-presets.default.json`（改探测路径优先改这里，而不是写死在代码里）  
- **用户可覆盖**：`~/.agent-backup/agent-presets.json`（同 `key` 合并；提供了 `groups` 则整表替换）

### 改功能时优先走的路径

1. **加新 Agent / 改备份路径** → 改 `agent-presets.default.json`，必要时更新文档；尽量别在 Rust 里硬编码目录表。  
2. **改打包行为 / 排除规则** → `packager.rs` + `config.exclusions`。  
3. **改备份流程 / 产物结构** → `backup.rs`（保持 zip 命名与 `backups/<hostname>/<timestamp>/` 约定）。  
4. **改 UI** → `src/App.tsx` + `App.css`；新 IPC 加在 `commands.rs` 并挂到 `lib.rs` 的 `invoke_handler`。  
5. **权限** → `src-tauri/capabilities/default.json`。

### 本地验证

```bash
# 前端类型
npx tsc --noEmit

# 后端冒烟（会打一份到临时目录）
cd src-tauri
cargo test presets_parse_and_backup_local -- --nocapture

# 桌面调试
npm run tauri dev
```

### 明确的非目标 / 后续可做（未承诺）

- 应用内恢复向导、单文件浏览恢复  
- 网盘原生 OAuth（百度 / 阿里云盘等）——当前用「本地挂载路径」覆盖  
- Linux  
- 钥匙串存密、备份配额清理 UI  
- 更细的进度条、只重试失败目标  

改代码时：小步、贴合现有模块风格；不要顺手大重构；用户配置目录里的真实密钥不要写进仓库。

---

## 文档

- [配置说明（presets / config / 产物）](docs/CONFIGURATION.md)

---

硬盘会坏，Agent 配置不该跟着一起没。如果你也有类似经历，欢迎直接用，或提 PR 把你常用的 Agent 加进默认 presets。
