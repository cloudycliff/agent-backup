<div align="center">

# Agent Backup

**Back up your AI coding assistants' skills, rules, settings and sessions to a local folder or S3, in one click.**

[![Release](https://img.shields.io/github/v/release/cloudycliff/agent-backup?label=release)](https://github.com/cloudycliff/agent-backup/releases/latest)
![Platforms](https://img.shields.io/badge/platform-Windows%20%7C%20macOS-blue)
![Built with Tauri](https://img.shields.io/badge/built%20with-Tauri%202-24C8DB)

[简体中文](README.md) · English

</div>

---

## Why

A colleague's disk died. The skills, rules, conversations and settings he had built up across his AI agents were gone overnight.

None of that lives in a code repository you can `git clone` back. It sits in folders like `~/.cursor`, `~/.claude` and `~/.codex` that nobody thinks to back up. Agent Backup finds them, packs them into encrypted zips on a schedule, and writes them where you choose. It is deliberately **one-way**: the goal is simply that you still have a copy when you switch machines or something breaks.

## Features

- **Automatic detection**: finds the agents installed on this machine; pick what to back up per group (config, rules, skills, sessions…)
- **Safe defaults**: config, rules and skills are on by default; sessions can be large and start unselected; keys, credentials and caches are always excluded
- **Multiple destinations**: local folders (including mounted OneDrive, iCloud or NAS drives) and S3-compatible storage (AWS S3, Cloudflare R2, MinIO) side by side; one failing doesn't stop the others
- **Encryption**: each source becomes its own AES-256 encrypted zip that 7-Zip, WinRAR, Keka and similar tools can open
- **Scheduled backups**: runs daily from the tray; a time missed during sleep or shutdown is caught up once on the next start; scheduled runs skip writing when nothing has changed
- **Visibility**: size estimate before a backup and progress during it; silent on success, a system notification on failure; history with details and one-click re-run
- **Several machines**: backups are grouped by host name, so many computers can share one drive without mixing
- **Extensible**: add agents or adjust paths with JSON presets, or add any folder directly
- **Chinese / English** UI, following the system language by default

## Supported agents

| Agent | Default root | Groups (* = off by default) |
|---|---|---|
| Cursor | `~/.cursor` | Config, Rules, Skills, Agents, Hooks, Plugins\*, Sessions & project state\* |
| Cursor CLI | `~/.cursor` | Config, Sessions\* |
| Claude Code | `~/.claude` | Config, Rules, Skills, Hooks, Agents, Sessions & project state\* |
| Codex | `~/.codex` | Config, Skills, Rules, Plugins\*, Memories\*, Sessions\* |
| Workbuddy | `~/.workbuddy` | Config, Skills, Experts, Plugins / Connectors\*, Memory\*, Sessions & project state\* |
| Windsurf | `~/.codeium/windsurf` | Config, Rules, Skills, Workflows, Memories\* |
| Continue | `~/.continue` | Config, Skills / Prompts, Assistants, Sessions\* |
| Cline | `~/.cline` | Config & data, Task history\* |
| Kiro | `~/.kiro` | Config, Skills, Steering, Extensions\* |
| Gemini CLI | `~/.gemini` | Config, Skills / Commands, History (sessions & checkpoints)\* |
| Aider | `~` (config files in the home folder) | Config |
| Shared skills | `~/.agents` | Skills (many skill managers keep skills here and link them into each agent) |
| Cursor / Windsurf / Kiro editor settings | `<product>/User` in the system app data folder | Settings, keybindings, snippets, profiles |

For anything not listed, add it as a custom folder or write a preset; see the [configuration guide](docs/CONFIGURATION.en.md).

## Install

Download the installer from [Releases](https://github.com/cloudycliff/agent-backup/releases/latest):

| System | File |
|---|---|
| Windows 10 / 11 (x64) | `Agent.Backup_<version>_x64-setup.exe` (recommended) or `.msi` |
| macOS (Apple silicon) | `Agent.Backup_<version>_aarch64.dmg` |
| macOS (Intel) | `Agent.Backup_<version>_x64.dmg` |

The installers aren't code-signed yet:

- **Windows**: when SmartScreen appears, choose "More info" → "Run anyway".
- **macOS**: the first time, right-click the app in Finder → "Open". If macOS says the app is damaged, run:
  ```bash
  xattr -dr com.apple.quarantine "/Applications/Agent Backup.app"
  ```

## Quick start

1. **Settings**: set an encryption password (or turn encryption off).
2. **Destinations**: add a local folder, or configure S3 and click "Test write".
3. **Backup**: check the agents and groups to include, then click "Back up now".
4. **History**: check the result; optionally turn on the daily schedule and start on login in Settings.

Closing the window keeps the app in the tray so scheduled backups can run. To quit completely, use "Quit" in the tray menu.

## Output and restore

```text
<destination>/backups/<host name>/<timestamp>/
  manifest.json                  # sources, checksums and per-destination results (plain text)
  cursor_<timestamp>.zip
  claude-code_<timestamp>.zip
  ...
```

- One zip per agent and custom source; timestamps are UTC, `YYYY-MM-DDTHHMMSSZ`.
- Local destinations are written to a `.partial` folder first and renamed when complete, so a half-written backup never remains.
- Backups are kept indefinitely; nothing is cleaned up automatically.

There is no in-app restore yet. To restore, open the zip with 7-Zip, WinRAR or Keka using your password and copy the files back. Windows Explorer and the macOS Archive Utility can't open AES-encrypted zips.

## Configuration and data

Everything lives in `~/.agent-backup/` (`%USERPROFILE%\.agent-backup\` on Windows):

| File | Contents |
|---|---|
| `config.json` | Selections, destinations, schedule, encryption, language and other settings |
| `agent-presets.json` | Optional custom or overriding agent presets |
| `history.jsonl` | Backup history |
| `logs/agent-backup.log` | Run log |

The preset format, merge rules and every setting are described in the [configuration guide](docs/CONFIGURATION.en.md).

> [!IMPORTANT]
> The encryption password and S3 keys are stored in plain text in `config.json`. Protect your user folder.

## Build from source

Requires [Node.js](https://nodejs.org/) LTS, [Rust](https://www.rust-lang.org/tools/install) stable and the [Tauri system prerequisites](https://v2.tauri.app/start/prerequisites/).

```bash
git clone https://github.com/cloudycliff/agent-backup.git
cd agent-backup
npm install
npm run tauri dev      # development mode
npm run tauri build    # build installers
```

Checks:

```bash
npx tsc --noEmit                 # front-end type check
cd src-tauri && cargo test       # backend tests (includes a smoke backup of this machine's agent folders into a temp folder)
```

Code layout, design constraints and the release process are in [CONTRIBUTING.md](CONTRIBUTING.md) (Chinese).

## Roadmap

Directions already decided against or deferred, to save repeated discussion:

- Two-way sync and conflict merging: out of scope; this is a one-way backup tool
- In-app restore wizard and per-file restore: deferred
- Native cloud drive sign-in (Baidu, Aliyun Drive and others): use a mounted local folder instead
- Linux, keychain password storage, backup quotas and automatic cleanup: deferred

## Contributing

Issues and PRs are welcome, especially ones adding the agents you use to the built-in presets. Please read [CONTRIBUTING.md](CONTRIBUTING.md) first.

---

<div align="center">
Disks fail. Your agent setup shouldn't go down with them.
</div>
