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
| 5 | 流式压缩 + 归档哈希；单次遍历；定时备份内容无变化时跳过；去重与类型统一；S3 运行时复用；移除未用依赖 | C1 C2 C3 C7 D1 D2 D3 D4 D5 | 已完成 |

未纳入计划、留作后续：C4 C5 D6，D3 中"磁盘写入翻倍"、D4 中的分片上传，以及第 5 节的易用性与可扩展性条目。

### 第二轮

| 步骤 | 内容 | 覆盖问题 | 状态 |
|---|---|---|---|
| 6 | 首页显示上次成功备份与下次定时；本地目标位于源内时不再递归打包 | 第 5 节前两条 | 已完成 |
| 7 | release 日志写入文件，界面可打开日志目录 | C5 | 已完成 |
| 8 | 前端拆分为组件 | C4 | 已完成 |
| 9 | S3 测连改为写入测试；目标可编辑；排除规则可在界面修改；加密未设密码时禁止备份；密码不再 trim；自定义源可单独开关 | 第 5 节第 3、4 条 | 已完成 |
| 10 | `open_path` 只允许打开备份目标与配置目录；设置 CSP | 第 5 节最后一条 | 待处理 |
| 11 | 预设合并保留内置顺序、检查 `version`；目标类型抽象为 `Target` trait | 第 5 节第 5、6 条 | 待处理 |
| 12 | 各源并行打包 | D6 | 待处理 |

`app.language`（界面多语言）不在本轮：需要先确定支持哪些语言，且后端错误信息也都是中文，单独排期。

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

### 步骤 5：流式压缩、单次遍历、无变化跳过与类型统一

改动：
- `packager.rs`：
  - 新增 `SourcePlan`：一次遍历得到文件列表和原始字节数，估算（`estimate`）和打包（`pack`）都基于它，备份时不再遍历两遍（D1）。原来的 `pack_agent` / `pack_custom` / `estimate_agent` / `estimate_custom` 合并为 `plan_agent` / `plan_custom`。
  - `zip_files` 按 256 KB 分块读写，不再把整个文件读进内存；单个文件 ≥ 4 GB 时开启 `large_file`；读到一半出错时用 `abort_file` 撤掉该条目并记警告（D2）。
  - `sha256` 改为"相对路径 + 每个文件内容的 SHA-256"的内容摘要，与加密无关；新增 `archive_sha256`，即 zip 文件本身的哈希（C3）。
- `backup.rs`：
  - 状态改为枚举 `Status`（`ok` / `partial` / `failed` / `skipped`），源与目标结果改为 `SourceRecord`、`DestResult` 结构，不再用裸字符串和 `serde_json::Value`；`notify_error` 用 `BackupRunResult::failure` 构造结果（C2）。
  - 选择待备份源的逻辑只剩 `plan_sources` 一处（C1 剩余部分）。
  - 临时工作目录由 `WorkDir` 守卫在任何返回路径上删除；本地目标先写 `.partial` 目录再改名，失败时删除半成品（D3 的清理部分）。
  - 定时备份时，若内容摘要、主机、目标集合、加密设置都与最近一次真正写入的成功备份一致，且该备份不超过 7 天、本地目标中的 `manifest.json` 仍在，就跳过写入，历史中记"内容相同"（D5）。手动备份、有失败或警告时总是写入；7 天上限保证即使远端副本被删，最迟一周会重新写一份。
- `history.rs`：`HistoryEntry` 改为强类型，兼容旧版本写下的记录（新增字段带默认值）。
- `s3.rs`：改用 Tauri 共享的 tokio 运行时（`tauri::async_runtime::block_on`），不再每次调用新建多线程运行时（D4 的运行时部分）。
- `Cargo.toml`：移除未使用的 `aws-config` 和直接依赖 `tokio`，`Cargo.lock` 少了 144 行（C7）。
- 顺带：删除未使用的 `default_agent_state`；`open_presets_file` 复用 `write_user_presets_template_if_missing`（原子写）。
- 新增测试：内容摘要不受加密影响且归档哈希与文件一致；无变化跳过的各项条件（内容、目标集合、超过 7 天、加密切换、有警告、本地副本被删、跳过记录不作基准）；本地写入失败不留目录；旧版历史记录可解析。

验证：
- `cargo test` 19 个测试通过；`npx tsc --noEmit` 通过；无编译警告。
- 本机冒烟产物：12 个源全部成功，每个 zip 的实际 SHA-256 与 manifest 中的 `archive_sha256` 一致；目标目录无 `.partial` 残留，临时工作目录已删除。
- 未做界面实操验证（进度条、托盘定时跳过的提示文案需要在桌面环境确认）。

仍未处理：
- D3：仍是先打包到临时目录再拷贝/上传，本地目标的磁盘写入量没有减少。直接写到本地目标会让"多目标共用一份归档"和"无变化跳过"变复杂，收益有限，暂不做。
- D4：S3 仍是单次 `PutObject`，单个归档超过 5 GB 会失败，需要改分片上传。
- C4（前端拆分）、C5（release 日志落盘）、D6（并行打包），以及第一部分第 5 节的易用性与可扩展性条目。

### 步骤 6：首页状态与递归防护

改动：
- 首页（备份页）新增状态行：上次成功备份的时间（本地时间）、触发方式，以及是否部分成功或内容无变化；下次定时的时间，错过时显示"即将补跑"，未开启时显示"未开启"。
  - `history.rs` 新增 `latest_success`（最近一次 `ok` 或 `partial`）。
  - `scheduler.rs` 新增 `next_run`，与调度器共用"最近应执行时间点"的计算。
  - 新命令 `get_status`；前端在每次备份结束、配置变更后刷新。
- 记录列表的时间改为本地时间显示，触发方式显示为"手动 / 定时 / 重跑"。
- 递归防护：
  - 打包时跳过所有本地目标的 `backups/` 目录（按真实路径比较，经过链接也能识别）。只对名为 `backups` 的目录做路径解析，不拖慢普通遍历。
  - 自定义源本身位于某个目标的 `backups/` 内时，该源判为失败并说明原因；在界面添加这样的源会直接被拒绝。
  - `packager.rs` 的排除规则（globs、预设 hard_exclude、要跳过的目录）合并为 `Rules`，遍历函数参数从 6 个降到 4 个。
- 新增测试：`backups/` 目录不被打包而其他同名目录正常收录、源位于 `backups/` 内时失败、下次定时时间计算。

验证：
- `cargo test` 21 个测试通过；`npx tsc --noEmit` 通过；无编译警告。
- 未做界面实操验证。

### 步骤 7：日志写入文件

改动：
- 新增 `logging.rs`：基于 `log` 门面的文件日志，写入 `~/.agent-backup/logs/agent-backup.log`（跟随 `AGENT_BACKUP_CONFIG_DIR`）。超过 1 MB 时改名为 `agent-backup.1.log`，只保留这一份旧文件，总占用不超过约 2 MB。本应用记录 info 及以上，依赖库只记录 warn 及以上。debug 构建同时输出到控制台。
- 启动时安装日志并接管 panic，panic 信息也写入日志。
- 记录的事件：启动；每次备份开始、结束（状态、耗时、摘要）或失败原因；正在进行时被跳过；源失败或有警告（首条警告）；目标写入失败；manifest 写入失败；定时到期、调度状态写入失败、配置读取失败；系统通知发送失败。原来的 5 处 `eprintln!` 全部替换。
- 新命令 `open_log_dir`，设置页新增"打开日志目录"按钮。
- `Cargo.toml` 新增 `log`（已是间接依赖，只多开 `std` 特性）。
- 新增测试：日志超过上限时轮转，目录里只保留当前文件和一份旧文件。

验证：
- `cargo test` 22 个测试通过；`npx tsc --noEmit` 通过；无编译警告。
- 用临时配置目录启动 debug 版应用，日志文件生成在 `logs/` 下并写入启动记录。

### 步骤 8：前端拆分

改动：
- `App.tsx` 从 977 行降到 217 行，只保留跨页共享的状态（启动数据、忙碌、进度、估算、状态行、最近失败）、两个备份入口和顶部提示条。
- 新文件：
  - `types.ts`：所有前后端数据类型，以及 `WithRefresh`（执行配置命令并应用返回的启动数据）。
  - `format.ts`：`formatBytes`、`formatRunTime`、触发方式文案。
  - `components/StatusLine.tsx`、`components/ProgressCard.tsx`。
  - `tabs/BackupTab.tsx`（含 `AgentCard`）、`tabs/DestinationsTab.tsx`、`tabs/HistoryTab.tsx`（含 `HistoryItem`）、`tabs/SettingsTab.tsx`。
- 各页自己的表单草稿和列表状态移进对应组件：S3 表单只在目标页，记录列表与"仅失败"只在记录页，密码/主机名/定时时间草稿只在设置页。记录页通过父组件递增的 `version` 在备份结束后重新加载。
- 手动备份与整次重跑合并为一个 `startBackup`，结束后统一刷新记录、最近失败、状态行和估算。
- 修正：`get_latest_failure` 以前返回最近 20 条里的最近一条失败记录，即使之后已经备份成功，重启后"最近一次备份未完全成功"提示仍会出现。现在只看最新一条记录。
- 行为差异：设置页的草稿在每次进入设置页时按当前配置重新初始化（以前只在启动时初始化一次）；"仅失败"勾选在切换页面后会重置。

验证：
- `npx tsc --noEmit` 通过；`vite build` 成功；`cargo test` 22 个测试通过。
- 未做界面实操验证。

### 步骤 9：易用性修正

改动：
- S3 测连：
  - 由 `head_bucket` 改为在 `{prefix}/` 下 `PutObject` 一个小文件，再尽力删除。只有上传权限的密钥不再误报失败；删除失败只提示，不算测试失败。按钮改名"测试写入"。
  - 以前测试时忽略了 prefix，现在按实际写入位置测试。
  - S3 错误改用 `DisplayErrorContext` 展开完整原因链，不再只显示"dispatch failure"之类的顶层信息。
- 目标可编辑：
  - 新命令 `update_local_destination`、`update_s3_destination`。本地目标可改名、更换目录；S3 目标复用添加表单，Secret Key 留空表示不修改，测试写入时也会用已保存的 Secret Key。
  - 位置变化（本地目录，或 S3 的 Endpoint / Bucket / Prefix）时分配新的目标 id。否则"无变化跳过"会拿旧位置的历史当作新位置已有副本，新位置永远拿不到备份。
- 排除规则可在设置页编辑：两个文本框（文件 / 目录，每行一条），保存时去空行、去重并逐条用 globset 校验，出错时指出是哪一条；支持恢复默认。新命令 `update_exclusions`、`reset_exclusions`。
- 加密开启但未设密码时，"立即备份"按钮禁用（后端原本就会拒绝）。
- 密码不再 `trim`：保存和打包时都保留首尾空格，只有全空白才视为未设置。已保存的旧密码当时已被 trim，不受影响。
- 自定义源可单独开关（配置里原本就有 `enabled`，只是界面没有入口）。新命令 `set_custom_source_enabled`。
- 新增测试：排除规则的清理与非法规则报错。

验证：
- `cargo test` 23 个测试通过；`npx tsc --noEmit` 通过；无编译警告。
- S3 写入测试只验证了不可达地址返回"写入测试失败"，没有真实 S3 / R2 环境验证上传和删除路径。
- 未做界面实操验证。
