# Agent Backup configuration

This page explains how to add backup sources, adjust paths and change what is selected by default through config files, without changing code.

## Config files

App config folder (the full path is shown in Settings):

| File | Purpose |
|---|---|
| `config.json` | Selections, destinations, schedule, encryption and other preferences |
| `logs/agent-backup.log` | Run log (backup start/finish, failure reasons, schedule events); rotated to `agent-backup.1.log` past 1 MB |
| `agent-presets.json` | Optional user overrides and additions for agent detection and path groups |
| `history.jsonl` | Local backup history (written by the app) |

The built-in presets ship inside the app as `agent-presets.default.json`.  
If the config folder has no `agent-presets.json`, only the built-in presets are used.

Built-in agents (by `key`): `cursor`, `cursor-cli`, `claude-code`, `codex`, `workbuddy`, `windsurf`, `continue`, `cline`, `kiro`, `aider`, `gemini`, `shared-agents`, plus the editor user settings `cursor-editor`, `windsurf-editor` and `kiro-editor`.  
`root.win` / `root.mac` can be nested paths, e.g. `.codeium/windsurf` for Windsurf; an empty value means the base folder itself (Aider keeps its config files directly in the home folder).

Editor user settings (`settings.json`, `keybindings.json`, `snippets/`, `profiles/` and so on) don't live under `~/.xxx` but in the system app data folder: `%APPDATA%\<product>\User` on Windows and `~/Library/Application Support/<product>/User` on macOS. That's why they are separate presets of kind `app_data`.

Packing follows symlinks and directory junctions. Linked content is written into the zip at the link's location, and link cycles are skipped. `shared-agents` covers `~/.agents/skills`: many skill managers keep skills there and link them into each agent's `skills/` folder.

`cursor-cli` shares the `~/.cursor` root with `cursor` and only collects the CLI's own `cli-config.json`, `agent-cli-state.json` and the `chats/` sessions (off by default; they can be large). The shared rules / skills / `mcp.json` / `hooks.json` belong to the `cursor` preset so they aren't packed twice; keep `cursor` selected even if you only use the CLI. The CLI program itself (`%LOCALAPPDATA%\cursor-agent` on Windows, `~/.local/share/cursor-agent` on macOS) isn't backed up, since reinstalling restores it; login credentials live in the system keychain and are never packed.

## Merge rules

1. Load the built-in `agent-presets.default.json`
2. If a user `agent-presets.json` exists, merge it by `agents[].key`
3. If the user gives `groups` for an agent, they **replace** that agent's groups entirely (arrays aren't merged)
4. Set `"disabled": true` on a built-in agent to hide it
5. Add any new `key` to support another tool

`sources.agents.<key>.paths.<groupId>` in `config.json` only records selection; what the paths mean comes from the presets.

- A preset adds a group that config doesn't know yet → `default_enabled` is used
- Config has a key the presets no longer define → ignored

## agent-presets.json example

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
          "label": "Config",
          "labels": { "zh": "配置" },
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
          "label": "Sessions",
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

### Fields

| Field | Meaning |
|---|---|
| `key` | Stable ID, used for selections in config and in zip file names |
| `label` | Name shown in the UI |
| `labels` | Optional translations of `label` by language code, e.g. `{"zh": "配置"}`; `label` is shown when the current language has none. `groups[].labels` works the same way. Overriding a built-in agent's `label` drops its built-in translations |
| `disabled` | When `true`, the agent isn't detected or shown |
| `root.kind` | `home_subdir`: relative to the home folder; `app_data`: relative to the system app data folder (`%APPDATA%` on Windows, `~/Library/Application Support` on macOS) |
| `root.win` / `root.mac` | Subpath under that base folder; empty means the base folder itself |
| `root.env_override` | If this environment variable is set, its value is used as the root |
| `groups[].id` | Selection key, matching `config.sources.agents.<key>.paths.<id>` |
| `groups[].include` | Files or folders relative to the root; end folders with `/` |
| `hard_exclude` | Always excluded for this agent (applied before the global exclusions); names ending in `/` exclude folders with that name at any depth |
| `detect` | Optional. The agent counts as "installed" only if one of these paths exists under the root; without it, an existing root is enough. Useful when several agents share a root |

Folders listed explicitly in `groups[].include` aren't affected by the global exclusion names (Gemini's `tmp/`, for example), but subfolders with excluded names inside them still are.

## config.json notes

- `destinations[]`: `local` (a local folder) and `s3` (S3-compatible: R2 / MinIO / AWS and others)
- Local destinations write to `{root_path}/backups/{hostname}/{timestamp}/`
- S3 destinations write to `s3://{bucket}/{prefix}/backups/{hostname}/{timestamp}/` (path-style)
- The S3 "Test write" uploads a small `.agent-backup-write-test` file under `{prefix}/` and then tries to delete it, so keys with only upload (PutObject) permission pass; without delete permission the test file stays behind and can be removed manually
- Destinations can be edited in the UI. Renaming doesn't affect history; changing a local folder or an S3 Endpoint / Bucket / Prefix counts as a new destination, so the next scheduled backup writes a full copy there even if nothing changed. When editing S3, leave Secret Key empty to keep the current one
- `exclusions.file_name_globs` / `exclusions.dir_name_globs`: global exclusion rules, editable in Settings (one per line; syntax is checked and duplicates removed on save) and resettable to the defaults
- Multiple destinations are best effort: one failing doesn't affect the others
- `encryption.enabled` defaults to `true`; zips are then protected with an **AES-256** password (WinRAR, 7-Zip and others can extract them)
- The password is stored in plain text in `config.json` (a product decision); leading and trailing spaces are part of it. Backups can't start while encryption is on without a password
- `sources.custom[].enabled`: custom sources can be switched off individually; they keep their settings but aren't backed up
- `schedule.enabled` + `schedule.time_local` (local `HH:mm`): backs up daily while the app runs in the tray. A missed time (sleep, shutdown, app not running) is caught up once on the next run, never several times
- Config files are saved by writing a temp file and renaming it; missing fields are filled with defaults, so older configs keep working
- `app.start_on_login`: start when you log in
- `app.language`: UI language, `auto` (follow the system, default), `zh-CN` or `en`; also switchable in Settings. The tray menu, notifications and error messages switch too; existing history entries keep the language they were written in
- `sources.custom[]`: custom folder sources, independent of presets

Custom source example:

```json
{
  "id": "custom_xxxx",
  "label": "my-extra",
  "path": "D:\\\\notes\\\\agent",
  "enabled": true
}
```

## Backup output

```
{root}/backups/{hostname}/{timestamp}/
  manifest.json
  cursor_{timestamp}.zip
  claude-code_{timestamp}.zip
  ...
```

- One zip per agent / custom source; up to 4 sources are packed at the same time
- Custom source file names come from their names; duplicates get `-2`, `-3` suffixes
- Folder and file names carry a UTC timestamp: `YYYY-MM-DDTHHMMSSZ`
- `manifest.json` is plain text and records the sources, checksums and per-destination results:
  - `sources[].sha256`: content digest over the relative paths and SHA-256 of every file in the zip; independent of encryption and unchanged while the content is unchanged
  - `sources[].archive_sha256`: SHA-256 of the zip file itself, for checking downloaded or copied archives
- Packing always skips local destinations' `{root}/backups/` folders, so a destination inside a source never re-packs old backups; folders inside `backups/` can't be custom sources
- Local destinations are written to `{timestamp}.partial/` first and renamed when complete; on errors the partial copy is deleted, so destination folders only ever contain complete backups
- For scheduled backups, if every source's content digest matches the latest successful backup (same host, same destinations, same encryption setting, that backup still present in the local destination, and no older than 7 days), no new copy is written and history records "content unchanged". Manual backups always write; so do runs with failed sources or warnings
- A single unreadable file (locked, no permission, broken or cyclic link) doesn't fail the whole source: the file is skipped, the reason goes into `sources[].warnings` (up to 20 per source), and the run is marked `partial` with a notification
- If the user `agent-presets.json` can't be parsed, the built-in presets are used; an agent without `root` or with an unsupported `root.kind` is skipped on its own. Both cases are shown at the top of the window

## Common tasks

### Change the paths of a built-in agent

Add the same `key` to the user `agent-presets.json` with complete `groups` (they replace the built-in groups).

### Turn off a built-in agent

```json
{
  "version": 1,
  "agents": [
    { "key": "workbuddy", "disabled": true }
  ]
}
```

(With only `key` + `disabled`, every other field is still inherited.)

### Restore the official presets

Delete `agent-presets.json` from the config folder, or use "Reset agent presets" in Settings.

## Security

- Key files and caches are excluded by default; still check custom `include` entries so `credentials/`, `auth.json` or `.env` don't end up in backups
- The encryption password and S3 keys are stored in `config.json`; protect your user folder
- The UI can only open folders inside local destinations or the config folder, never arbitrary files
- This version backs up one way and has no in-app restore; extract the zips manually when needed
