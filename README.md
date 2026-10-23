<div align="center">

# Agent Backup

**把 AI 编程助手的 skills、rules、配置和会话，一键备份到本地目录或 S3。**

[![Release](https://img.shields.io/github/v/release/cloudycliff/agent-backup?label=release)](https://github.com/cloudycliff/agent-backup/releases/latest)
![Platforms](https://img.shields.io/badge/platform-Windows%20%7C%20macOS-blue)
![Built with Tauri](https://img.shields.io/badge/built%20with-Tauri%202-24C8DB)
[![License: MIT](https://img.shields.io/badge/license-MIT-green)](LICENSE)

简体中文 · [English](README.en.md)

</div>

---

## 为什么需要它

同事的硬盘坏了。他用各种 Agent 攒了很久的 skills、rules、对话和习惯配置，一夜之间全没了。

这些东西不在代码仓库里，没法 `git clone` 回来；它们散落在 `~/.cursor`、`~/.claude`、`~/.codex` 这些平时不会想到要备份的目录里。Agent Backup 自动找到它们，按计划打包成加密 zip，写到你指定的地方。它只做**单向备份**，目标很简单：换电脑或出意外时，手里还有一份。

## 功能

- **自动探测**：识别本机装了哪些 Agent，按分组勾选要备份的内容（配置、rules、skills、会话……）
- **安全默认值**：默认备份配置、rules 和 skills；会话体积大，默认不勾；密钥、凭据和缓存始终排除
- **多个目标**：本地文件夹（包括 OneDrive、iCloud、NAS 等已挂载的盘）和 S3 兼容存储（AWS S3、Cloudflare R2、MinIO）可同时启用，一个失败不影响其他
- **加密**：每个源单独打成 AES-256 加密的 zip，用 7-Zip、WinRAR、Keka 等常见工具即可解压
- **定时备份**：托盘常驻，每天定时运行；睡眠或关机错过的时间会在下次启动时补跑一次；内容没有变化时，定时备份不会重复写入
- **结果可查**：备份前预估体积，备份中显示进度；成功静默，失败发系统通知；历史记录可查看详情并一键重跑
- **多台机器**：备份按主机名分目录，多台电脑共用一个网盘也不会混在一起
- **可扩展**：通过 JSON 预设添加新的 Agent 或调整路径，也可以直接添加任意文件夹
- **中文 / English** 界面，默认跟随系统语言

## 支持的 Agent

| Agent | 默认根目录 | 可选内容（带 * 的默认不勾） |
|---|---|---|
| Cursor | `~/.cursor` | 配置、Rules、Skills、Agents、Hooks、Plugins\*、会话与项目状态\* |
| Cursor CLI | `~/.cursor` | 配置、会话\* |
| Claude Code | `~/.claude` | 配置、Rules、Skills、Hooks、Agents、会话与项目状态\* |
| Codex | `~/.codex` | 配置、Skills、Rules、Plugins\*、Memories\*、会话\* |
| Workbuddy | `~/.workbuddy` | 配置、Skills、Experts、Plugins / Connectors\*、Memory\*、会话与项目状态\* |
| Windsurf | `~/.codeium/windsurf` | 配置、Rules、Skills、Workflows、Memories\* |
| Continue | `~/.continue` | 配置、Skills / Prompts、Assistants、会话\* |
| Cline | `~/.cline` | 配置与数据、任务历史\* |
| Kiro | `~/.kiro` | 配置、Skills、Steering、Extensions\* |
| Gemini CLI | `~/.gemini` | 配置、Skills / Commands、历史（会话与检查点）\* |
| Aider | `~`（主目录下的配置文件） | 配置 |
| 共享 Skills | `~/.agents` | Skills（很多 skills 管理工具把技能放在这里再链接到各 Agent） |
| Cursor / Windsurf / Kiro 编辑器设置 | 系统应用数据目录下的 `<产品>/User` | 设置、快捷键、代码片段、配置档 |

没有列出的工具，可以用「自定义文件夹」直接添加，或者写一个预设，见[配置说明](docs/CONFIGURATION.md)。

## 下载安装

从 [Releases](https://github.com/cloudycliff/agent-backup/releases/latest) 下载对应安装包：

| 系统 | 文件 |
|---|---|
| Windows 10 / 11（x64） | `Agent.Backup_<版本>_x64-setup.exe`（推荐）或 `.msi` |
| macOS（Apple 芯片） | `Agent.Backup_<版本>_aarch64.dmg` |
| macOS（Intel） | `Agent.Backup_<版本>_x64.dmg` |

安装包目前没有代码签名：

- **Windows**：SmartScreen 提示时，点「更多信息」→「仍要运行」。
- **macOS**：首次打开时在 Finder 里右键应用 →「打开」。如果提示「已损坏」，执行：
  ```bash
  xattr -dr com.apple.quarantine "/Applications/Agent Backup.app"
  ```

## 快速开始

1. **设置**：设一个加密密码（或者关掉加密）。
2. **目标**：添加一个本地文件夹，或配置 S3 并点「测试写入」。
3. **备份**：确认要备份的 Agent 和分组，点「立即备份」。
4. **记录**：查看结果；需要的话在设置里开启每日定时备份和登录时启动。

关闭窗口时应用会留在托盘里，这样定时备份才能运行；要完全退出，请用托盘菜单的「退出」。

## 备份产物与恢复

```text
<目标>/backups/<主机名>/<时间戳>/
  manifest.json                  # 源列表、校验和、各目标结果（明文）
  cursor_<时间戳>.zip
  claude-code_<时间戳>.zip
  ...
```

- 每个 Agent 和自定义源各一个 zip，时间戳为 UTC 时间 `YYYY-MM-DDTHHMMSSZ`。
- 本地目标先写入 `.partial` 临时目录，全部完成后再改名，所以不会留下半份备份。
- 备份会一直保留，不会自动清理。

目前没有应用内恢复。需要时，用 7-Zip、WinRAR 或 Keka 输入密码解压对应的 zip，再把文件拷回原目录。Windows 资源管理器和 macOS 自带的归档工具不支持 AES 加密的 zip。

## 配置与数据

所有配置都在 `~/.agent-backup/`（Windows 为 `%USERPROFILE%\.agent-backup\`）：

| 文件 | 内容 |
|---|---|
| `config.json` | 勾选状态、目标、定时、加密、语言等设置 |
| `agent-presets.json` | 可选，自定义或覆盖 Agent 预设 |
| `history.jsonl` | 备份历史 |
| `logs/agent-backup.log` | 运行日志 |

预设格式、合并规则和所有配置项见[配置说明](docs/CONFIGURATION.md)。

> [!IMPORTANT]
> 加密密码和 S3 密钥以明文保存在 `config.json` 中，请保护好本机用户目录。

## 从源码构建

需要 [Node.js](https://nodejs.org/) LTS、[Rust](https://www.rust-lang.org/tools/install) stable，以及 [Tauri 的系统依赖](https://v2.tauri.app/start/prerequisites/)。

```bash
git clone https://github.com/cloudycliff/agent-backup.git
cd agent-backup
npm install
npm run tauri dev      # 开发模式
npm run tauri build    # 打包安装包
```

运行检查：

```bash
npx tsc --noEmit                 # 前端类型检查
cd src-tauri && cargo test       # 后端测试（含一次读取本机 Agent 目录的冒烟备份，产物写到临时目录）
```

代码结构、设计约束和发版流程见 [CONTRIBUTING.md](CONTRIBUTING.md)。

## 路线图

已经确定不做或暂缓的方向，以免重复讨论：

- 双向同步与冲突合并：不做，定位就是单向备份
- 应用内恢复向导、按文件浏览恢复：暂缓
- 网盘原生登录（百度网盘、阿里云盘等）：用本地挂载目录代替
- Linux 支持、系统钥匙串存密码、备份配额与自动清理：暂缓

## 参与贡献

欢迎提 Issue 和 PR，尤其是把你常用的 Agent 加进内置预设。动手前请先读 [CONTRIBUTING.md](CONTRIBUTING.md)。

## 许可证

[MIT](LICENSE)

---

<div align="center">
硬盘会坏，Agent 配置不该跟着一起没。
</div>
