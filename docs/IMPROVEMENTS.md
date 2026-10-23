# 实现分析与优化记录

本文记录 2026-10-23 对 v0.1.1 代码的整体分析结论，以及之后按步骤落地的改动。每一步对应一次独立提交，提交信息以 `[优化 N]` 开头，可用 `git log --grep "优化"` 查看。

## 一、分析结论

### 1. 备份完整性（最高优先级）

| # | 问题 | 位置 | 本机验证 |
|---|---|---|---|
| A1 | 遍历不跟随软链接 / 目录联接（Junction），链接目录被静默跳过 | `packager.rs` `collect_files` / `collect_files_tree` | `~/.claude/skills` 共 84 个技能，zip 中仅 17 个；67 个联接全部缺失 |
| A2 | 被大量联接引用的 `~/.agents/skills`（79 个技能）不在任何预设中，等于完全没备份 | `agent-presets.default.json` | 已确认 |
| A3 | Aider 预设在 `~/.aider/` 下找 `.aider.conf.yml`，实际该文件在主目录根下 | 预设 | — |
| A4 | Cursor IDE 的 `settings.json` / `keybindings.json` / `snippets/` 位于 `%APPDATA%\Cursor\User`，未覆盖；Kiro、Windsurf 同理 | 预设 / `presets.rs` 仅支持 `home_subdir` | Cursor、Kiro 的 User 目录存在 |
| A5 | Gemini 的 `tmp/`、Continue 的 `index/` 同时出现在分组 include 和 `hard_exclude`，分组勾上也永远为空 | 预设 | — |
| A6 | 全局排除名（如 `tmp`、`logs`）在遍历深度 0 也生效，include 指定的目录本身会被剔除；`collect_files_tree` 处理了深度 0，两处不一致 | `packager.rs` | — |
| A7 | 单个文件读取失败会让整个 Agent 失败；遍历错误又被 `filter_map(e.ok())` 静默吞掉 | `packager.rs` | — |

### 2. 可用性与稳定性

| # | 问题 | 位置 |
|---|---|---|
| B1 | 所有命令都是同步函数，Tauri 2 下在主线程执行：手动备份期间窗口无响应，进度事件到结束才送达 | `commands.rs` |
| B2 | 每次勾选都会触发一次完整目录遍历的体积估算，同样在主线程 | `App.tsx` `withRefresh` → `estimate_backup_size` |
| B3 | 进度分母是未压缩估算体积，分子是压缩后 zip 体积，永远到不了 100%；上传阶段改按目标个数计算，进度条回跳 | `backup.rs` |
| B4 | 定时只在设定的那一分钟触发，睡眠 / 关机错过当天即不补跑；返回 `Err` 时不记录，同一分钟可能重复失败通知 | `scheduler.rs` |
| B5 | 用户预设解析失败或某 Agent 缺 `root`，`get_bootstrap` 直接报错，界面停在"加载中" | `presets.rs` / `commands.rs` |
| B6 | 配置结构没有 `#[serde(default)]`，以后加字段会让旧配置解析失败；配置非原子写入；`get_bootstrap` 每次读取都写一次配置 | `config.rs` / `commands.rs` |

### 3. 代码质量

| # | 问题 |
|---|---|
| C1 | 选择待备份 Agent 的逻辑在 `backup.rs` 写了三遍；`PackedSource` 构造重复 8 次；整体状态计算重复 2 次；"备份并通知"在两个命令和托盘里各一份 |
| C2 | 状态是裸字符串，目标结果是 `serde_json::Value`，`notify_error` 手工拼结果 JSON |
| C3 | manifest 的 `sha256` 是"相对路径 + 原文件内容"的哈希，不是 zip 本身的哈希，无法用来校验归档 |
| C4 | 前端是 956 行的单组件 |
| C5 | release 版隐藏控制台，`eprintln!` 日志全部丢失 |
| C6 | 冒烟测试读写真实用户目录（会往真实 `history.jsonl` 追加记录），无 Agent 的 CI 机器上会失败 |
| C7 | `aws-config` 依赖未被使用 |

### 4. 性能

| # | 问题 |
|---|---|
| D1 | 一次备份遍历两遍目录树（估算 + 打包） |
| D2 | `zip_files` 对每个文件 `read_to_end`，大文件（如数百 MB 的 `store.db`）整块进内存；未开启 large_file |
| D3 | 先打包到临时目录再拷贝到目标，磁盘写入翻倍；部分错误路径不清理临时目录；目标目录可能留下半成品 |
| D4 | S3 每次调用新建多线程 tokio 运行时；无分片上传（单次 PUT 上限 5 GB） |
| D5 | 每次全量写入，内容未变化也会产生一份新备份 |
| D6 | 各源串行打包 |

### 5. 易用性与可扩展性（暂列待办）

- 首页缺"上次成功备份时间 / 下次定时时间"
- 本地目标位于自定义源内部时会递归膨胀，未做检查
- S3 测连用 `head_bucket`，最小权限（仅 PutObject）的密钥会误报失败
- 目标不能编辑、排除规则不能在界面修改、开启加密未设密码时仍可点备份、密码被 `trim`
- 预设合并后按 key 排序丢失内置顺序；预设 `version` 未使用
- 新增目标类型需改 `backup.rs` 两处 match，可抽象 `Target` trait
- `app.language` 存在但文案写死中文
- `open_path` 接收任意路径交给 `explorer`，CSP 为 `null`

## 二、处理计划

| 步骤 | 内容 | 覆盖问题 | 状态 |
|---|---|---|---|
| 1 | 打包跟随链接；新增 `~/.agents` 预设；测试使用隔离的配置目录 | A1 A2 C6（部分） | 待处理 |
| 2 | 耗时命令改为异步执行；进度按原始字节统一计算并在源内更新 | B1 B2 B3 | 待处理 |
| 3 | 新增 `app_data` 根类型与 `detect` 字段；修正 Aider / Gemini / Continue；补 Cursor / Kiro / Windsurf 编辑器用户设置；修正深度 0 过滤 | A3 A4 A5 A6 | 待处理 |
| 4 | 单文件失败降级为警告；配置原子写入 + `serde(default)`；预设出错回退内置；定时补跑 | A7 B4 B5 B6 | 待处理 |
| 5 | 流式压缩 + 归档哈希；单次遍历；定时备份内容无变化时跳过；去重与类型统一；S3 运行时复用；移除未用依赖 | C1 C2 C3 C7 D1 D2 D3 D4 D5 | 待处理 |

未纳入计划、留作后续：C4 C5 D6，以及第 5 节的易用性与可扩展性条目。

## 三、执行记录
