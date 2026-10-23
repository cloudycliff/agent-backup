# 参与开发

本文写给要修改这个仓库的人和 Agent：哪些产品决策已经定了、代码在哪里、改功能该从哪下手、怎么验证和发版。

## 产品边界

以下决策已经确定，改动前请先讨论，不要在实现中顺手推翻：

| 决策 | 含义 |
|---|---|
| 单向备份 | 不做双向同步，不做冲突合并 |
| 暂不做应用内恢复 | 用户自行解压、拷回；`manifest.json` 为以后的恢复功能预留 |
| 只备份用户级目录 | 不扫描项目内的 `.cursor` 等目录；特殊需求用「自定义文件夹」 |
| 加密可选、默认开启 | 密码明文存在 `config.json`，有意保持简单 |
| 备份无限保留 | 不做自动清理 |
| 成功静默、失败通知 | 定时备份不能天天弹窗 |
| 重试即整次重跑 | 暂不做只补传失败的目标 |

## 代码结构

技术栈：Tauri 2（Rust）+ React + TypeScript + Vite。

```text
src/                          前端
  App.tsx                     外壳：页签、顶部提示、备份进度与结果
  tabs/                       四个页面：BackupTab、DestinationsTab、HistoryTab、SettingsTab
  components/                 ProgressCard、StatusLine
  i18n.tsx                    翻译函数、I18nProvider、useI18n
  locales/zh.ts, en.ts        界面文案
  format.ts, types.ts         格式化工具与 IPC 类型

src-tauri/src/                后端
  lib.rs                      启动流程、插件、命令注册
  commands.rs                 前端调用的 IPC 命令
  config.rs                   config.json 读写、主机名、原子写入
  presets.rs                  内置与用户预设合并、根目录解析、标签翻译
  packager.rs                 收集文件、排除规则、打 zip（可 AES）
  backup.rs                   备份编排：规划 → 并行打包 → 写入目标 → manifest → 历史
  targets.rs                  Target trait：本地目录与 S3 的写入实现
  s3.rs                       S3 兼容上传与写入测试
  scheduler.rs                每日定时（托盘进程内轮询，错过补跑）
  history.rs, notify.rs       历史记录、系统通知
  tray.rs                     托盘菜单
  i18n.rs                     当前语言与 t! 宏
  logging.rs                  日志文件与轮转
  smoke.rs                    读取本机真实目录的冒烟测试

src-tauri/resources/
  agent-presets.default.json  内置预设
  CONFIGURATION.md, .en.md    设置页显示的配置说明（docs/ 下同名文件的副本）
```

## 改功能从哪下手

1. **新增 Agent 或调整备份路径**：改 `src-tauri/resources/agent-presets.default.json`，不要在 Rust 里写死目录。中文名要同时提供 `labels.en`（有测试检查）。
2. **打包与排除规则**：`packager.rs` 和 `config.exclusions`。
3. **备份流程或产物结构**：`backup.rs`。保持 `backups/<hostname>/<timestamp>/` 布局和 zip 命名不变，已有用户的备份和"内容无变化"判断依赖它们。
4. **新的目标类型**：在 `config.rs` 的 `DestinationKind` 加一个变体，在 `targets.rs` 实现 `Target`，再加前端表单；`backup.rs` 不需要改。
5. **界面**：页面在 `src/tabs/`；新的 IPC 命令写在 `commands.rs`，并注册到 `lib.rs` 的 `invoke_handler`。
6. **权限与安全**：`src-tauri/capabilities/default.json` 和 `tauri.conf.json` 的 CSP。

### 文案与多语言

- 前端文案写进 `src/locales/zh.ts`，并在 `en.ts` 补上同一个键；缺键或多键都过不了 `tsc`。组件里用 `const { t } = useI18n()`。
- 后端面向用户的文字（错误、进度、通知）用 `t!("中文", "English", 参数…)`；`log::` 日志的措辞保持英文，不需要翻译。
- 修改 `docs/CONFIGURATION.md` 时同步修改 `docs/CONFIGURATION.en.md`，然后把两个文件复制到 `src-tauri/resources/`。

## 验证

```bash
npx tsc --noEmit
cd src-tauri && cargo test
```

- 测试不会碰真实配置：配置和历史写到临时目录。冒烟测试会读取本机的 Agent 目录，产物写到临时目录。
- 手动调试时，设置环境变量 `AGENT_BACKUP_CONFIG_DIR` 可以把配置目录指向别处，避免影响正在使用的配置。
- 涉及界面的改动，请在 `npm run tauri dev` 里切换中英文各看一遍。

## 发版

发布由 GitHub Actions 的 `Build Release Assets` 工作流完成，它只能手动触发，会构建 Windows、macOS（Apple 芯片和 Intel）安装包并上传到对应的 Release。

1. 修改版本号：`package.json`、`package-lock.json`（两处）、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock` 中 `agent-backup` 的条目。
2. 提交，打标签并推送：
   ```bash
   git tag -a vX.Y.Z -m "vX.Y.Z"
   git push origin master vX.Y.Z
   ```
3. 触发构建：
   ```bash
   gh workflow run release-assets.yml -f tag=vX.Y.Z
   ```
   或在 GitHub 的 Actions 页面点 "Run workflow"。构建约 10 到 15 分钟。

## 代码风格

- 小步提交，贴合现有模块的写法，不要顺手大重构。
- 不要把真实的密码、S3 密钥或本机配置写进仓库。
