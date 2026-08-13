# Agent Backup

Windows / macOS 上的 Tauri 桌面应用：检测本机 Agent 配置，单向备份到本地目录（及后续 S3）。

## 当前进度

**Phase 1–4 完成**

- Agent 探测与可配置 presets、本地备份、托盘、加密、定时、登录启动
- 记录页、失败通知、整次重跑
- S3 兼容目标 + 多目标尽力而为

## 开发

```bash
npm install
npm run tauri dev
```

配置目录：`~/.agent-backup/`（Windows 为 `%USERPROFILE%\.agent-backup\`）

## 文档

- [配置说明](docs/CONFIGURATION.md)
