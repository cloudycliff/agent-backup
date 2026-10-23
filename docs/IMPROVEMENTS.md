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
| 1 | 打包跟随链接；新增 `~/.agents` 预设；测试使用隔离的配置目录 | A1 A2 C6（部分） | 已完成 |
| 2 | 耗时命令改为异步执行；进度按原始字节统一计算并在源内更新 | B1 B2 B3 | 已完成 |
| 3 | 新增 `app_data` 根类型与 `detect` 字段；修正 Aider / Gemini / Continue；补 Cursor / Kiro / Windsurf 编辑器用户设置；修正深度 0 过滤 | A3 A4 A5 A6 | 已完成 |
| 4 | 单文件失败降级为警告；配置原子写入 + `serde(default)`；预设出错回退内置；定时补跑 | A7 B4 B5 B6 | 已完成 |
| 5 | 流式压缩 + 归档哈希；单次遍历；定时备份内容无变化时跳过；去重与类型统一；S3 运行时复用；移除未用依赖 | C1 C2 C3 C7 D1 D2 D3 D4 D5 | 待处理 |

未纳入计划、留作后续：C4 C5 D6，以及第 5 节的易用性与可扩展性条目。

## 三、执行记录

### 步骤 1：链接跟随与 `~/.agents` 预设

改动：
- `packager.rs`：`collect_files` 和 `collect_files_tree` 的 `WalkDir` 开启 `follow_links(true)`。zip 内路径仍按链接所在位置记录，循环链接由 walkdir 检测并跳过。
- 预设新增 `shared-agents`，备份 `~/.agents/skills/` 和 `.skill-lock.json`。
- `config.rs`：`config_dir()` 支持环境变量 `AGENT_BACKUP_CONFIG_DIR`；冒烟测试改用临时配置目录，不再往真实 `history.jsonl` 写记录。
- 新增单元测试：链接目录被收录（`collect_files` 与 `collect_files_tree` 都覆盖）、循环链接不会无限展开。Windows 用 `mklink /J` 建联接，Unix 用软链接。

验证：
- `cargo test` 3 个测试通过。
- 本机冒烟产物：`claude-code` zip 中 skills 从 17 个变为 84 个（全部）；新增 `shared-agents` zip，含 79 个技能。
- 真实 `~/.agent-backup/history.jsonl` 的修改时间未变化。

影响：链接到其他 Agent 目录的技能会在多个 zip 中各存一份，体积有所增加，换来的是每个 zip 单独解压即可恢复。

### 步骤 2：耗时命令异步化与进度修正

改动：
- `commands.rs`：`run_backup_now`、`retry_backup`、`estimate_backup_size`、`test_s3_destination` 改为 `async`，实际工作放进 `spawn_blocking`，不再占用主线程。
- 抽出 `run_and_notify`，两个备份命令和托盘菜单共用（原来三份重复代码）。
- `packager.rs`：`zip_files` / `pack_agent` / `pack_custom` 增加 `on_file` 回调，每写完一个文件上报其原始字节数。
- `backup.rs`：打包阶段按"已读原始字节 / 估算原始字节"计算，占 0–90%，每 150 ms 最多发一次事件；写入目标占 90–99%；写清单 99%；完成 100%。写入阶段不再显示字节数（之前混用了压缩后体积）。
- `App.tsx`：体积估算加请求序号，避免慢的旧请求覆盖新结果。
- 新增测试：通过 `test_s3_destination` 命令走后台线程调用 S3（内部会自建 tokio 运行时），确认返回错误而不是因运行时嵌套而 panic。

验证：
- `cargo test` 4 个测试通过；`npx tsc --noEmit` 通过；无新增编译警告。
- 未做界面实操验证（需要桌面环境手动点一次"立即备份"确认进度条连续更新、窗口不卡顿）。

### 步骤 3：预设修正与新根类型

改动：
- `presets.rs`：`root.kind` 新增 `app_data`（Windows `%APPDATA%`，macOS `~/Library/Application Support`，其他平台 `XDG_CONFIG_HOME` 或 `~/.config`）；子路径留空表示基准目录本身。
- `presets.rs`：预设新增可选 `detect` 字段，根目录下任一标志路径存在才算已安装；用户预设可覆盖。
- `packager.rs`：`collect_files` 在遍历深度 0 不再套用目录排除名，显式 include 的 `tmp/` 等目录能正常收录，内部同名子目录仍排除。
- 预设调整：
  - Aider：根目录改为主目录，收 `.aider.conf.yml`、`.aider.model.settings.yml`、`.aider.model.metadata.json`；去掉无效的"历史"分组（Aider 历史在各项目目录里，属于产品边界外）；用 `detect` 判断是否安装。
  - Gemini：`hard_exclude` 去掉 `tmp/`，"历史"分组勾选后可以收到 `tmp/` 下的会话与检查点。
  - Continue："会话"分组去掉与 `hard_exclude` 冲突的 `index/`（可重建的索引）。
  - Cursor CLI：加 `detect`（`cli-config.json` / `chats/`），只有 IDE 时不再显示为已安装。
  - 新增 `cursor-editor`、`windsurf-editor`、`kiro-editor`，备份编辑器 `User` 目录下的设置、快捷键、任务、代码片段和 profiles，排除 `globalStorage/`、`workspaceStorage/`、`History/`。
- 新增测试：空子路径解析为主目录、`app_data` 解析与未知类型报错、`detect` 判断、内置预设全部可解析、显式 include 不被全局排除名剔除。

验证：
- `cargo test` 9 个测试通过。
- 本机冒烟产物：`cursor-editor` zip 含 `settings.json`、`keybindings.json`；`kiro-editor` zip 含 `settings.json`；Windsurf 与 Aider 本机未安装，未生成归档。

兼容性：已有 `config.json` 里 Aider 的 `history`、Continue 分组状态等旧键会被忽略，不影响加载。

### 步骤 4：容错与定时补跑

改动：
- `packager.rs`：
  - 收集阶段的遍历错误（链接失效、循环链接、无权限）和打包阶段的读文件错误都改为警告，跳过该文件继续；全部文件都读不了才判为失败。每个源最多保留 20 条警告。
  - 先读文件再写 zip 条目，避免读失败时留下空条目。
  - 顺带消除了 `PackedSource` 的 8 处重复构造（`PackedSource::new` / `failed` / `pack`），以及估算与打包两套重复的 include 拼接（C1 的一部分）。
- `backup.rs`：
  - 抽出 `Summary`，结果与 manifest 共用（原来整体状态算两遍）。
  - 有警告的源计入 `sources_warned`，整体状态记为 `partial`，消息里写明"N 个源有文件被跳过"；`sources[].warnings` 写入结果、历史和 manifest。
  - 备份锁中毒（上次备份 panic）时继续使用，不再永久报"正在进行"；"正在进行"的提示改为常量 `BACKUP_BUSY`，调度器按常量判断。
- `presets.rs`：`load_merged_presets` + `resolve_agents` 合并为 `load_presets`，返回预设、解析后的 Agent 和警告。用户预设读不了或解析失败时回退内置预设；单个 Agent 缺 `root`、`root.kind` 不支持时跳过该 Agent。
- `config.rs`：
  - 所有配置结构加 `#[serde(default)]`，`AgentSourceState.enabled` / `CustomSource.enabled` 缺省为 `true`；新增字段或缺字段的旧配置都能加载。
  - 新增 `write_atomic`（写 `.tmp` 再改名），用于配置和定时状态。
  - `ensure_agent_defaults` 返回是否有变化，`get_bootstrap` 只在有变化时写配置。
- `scheduler.rs`：状态改记 `last_run_at`（兼容旧的 `last_run_date`）。只要"最近一个应执行时间点"之后没有跑过就执行，睡眠、关机后会补跑一次；首次看到定时配置时从当前时间开始计，不会立刻触发。除"正在进行"外，成功或失败都记录本次尝试，不再同一分钟内重复失败通知；"正在进行"时下个周期（20 秒后）再试。
- `App.tsx`：顶部显示预设警告；记录详情里显示每个源的警告；定时说明补充补跑行为。
- 新增测试：失效联接与被独占锁定的文件变为警告且其余文件正常打包、循环链接产生警告、坏 Agent 被跳过、部分字段的配置能补默认值、原子写覆盖已有文件且不留临时文件、定时到期判断（含隔天补跑）、旧状态格式兼容。

验证：
- `cargo test` 15 个测试通过；`npx tsc --noEmit` 通过；无新增编译警告。
- 本机冒烟产物 manifest：12 个源全部成功，`sources_warned` 为 0。

行为变化：以前单个文件读失败会让整个源失败（`failed`），现在是跳过该文件、整体 `partial`。如果某个文件长期被占用，每次备份都会提示一次，直到在排除规则里排除它。
