use serde::Serialize;
use std::path::PathBuf;

use crate::backup::{
    backup_output_dirs, estimate_backup, run_backup_with_app, BackupEstimate, BackupRunResult,
};
use crate::history::HistoryEntry;
use crate::scheduler::NextRun;
use crate::config::{
    config_dir, config_path, ensure_agent_defaults, load_config, open_path_in_explorer, save_config,
    user_presets_path, AppConfig, CustomSource, Destination, LocalDestination, S3Destination,
};
use crate::presets::{
    load_presets, reset_user_presets, write_user_presets_template_if_missing, ResolvedAgent,
};

#[derive(Debug, Serialize)]
pub struct BootstrapState {
    pub config: AppConfig,
    pub agents: Vec<AgentView>,
    pub config_dir: String,
    pub config_path: String,
    pub presets_path: String,
    pub hostname: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct AgentView {
    pub key: String,
    pub label: String,
    pub installed: bool,
    pub disabled: bool,
    pub root_path: Option<String>,
    pub groups: Vec<GroupView>,
    pub enabled: bool,
    pub paths: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct GroupView {
    pub id: String,
    pub label: String,
    pub default_enabled: bool,
}

fn to_agent_views(cfg: &AppConfig, agents: Vec<ResolvedAgent>) -> Vec<AgentView> {
    agents
        .into_iter()
        .filter(|a| !a.disabled)
        .map(|a| {
            let state = cfg.sources.agents.get(&a.key);
            let enabled = state.map(|s| s.enabled).unwrap_or(true);
            let mut paths = serde_json::Map::new();
            for g in &a.groups {
                let on = state
                    .and_then(|s| s.paths.get(&g.id).copied())
                    .unwrap_or(g.default_enabled);
                paths.insert(g.id.clone(), serde_json::Value::Bool(on));
            }
            AgentView {
                key: a.key,
                label: a.label,
                installed: a.installed,
                disabled: a.disabled,
                root_path: a.root_path.map(|p| p.to_string_lossy().to_string()),
                groups: a
                    .groups
                    .into_iter()
                    .map(|g| GroupView {
                        id: g.id,
                        label: g.label,
                        default_enabled: g.default_enabled,
                    })
                    .collect(),
                enabled,
                paths,
            }
        })
        .collect()
}

#[tauri::command]
pub fn get_bootstrap() -> Result<BootstrapState, String> {
    let loaded = load_presets()?;
    let mut cfg = load_config()?;
    if ensure_agent_defaults(&mut cfg, &loaded.presets) {
        save_config(&cfg)?;
    }
    let hostname = crate::config::resolve_hostname(&cfg);
    Ok(BootstrapState {
        agents: to_agent_views(&cfg, loaded.agents),
        config: cfg,
        config_dir: config_dir()?.to_string_lossy().to_string(),
        config_path: config_path()?.to_string_lossy().to_string(),
        presets_path: user_presets_path()?.to_string_lossy().to_string(),
        hostname,
        warnings: loaded.warnings,
    })
}

#[tauri::command]
pub fn save_app_config(config: AppConfig) -> Result<BootstrapState, String> {
    save_config(&config)?;
    get_bootstrap()
}

#[tauri::command]
pub fn add_local_destination(name: String, root_path: String) -> Result<BootstrapState, String> {
    let mut cfg = load_config()?;
    cfg.destinations.push(Destination::Local {
        id: format!("dest_{}", uuid::Uuid::new_v4()),
        name: if name.trim().is_empty() {
            "本地目录".into()
        } else {
            name
        },
        enabled: true,
        local: LocalDestination { root_path },
    });
    save_config(&cfg)?;
    get_bootstrap()
}

#[tauri::command]
pub fn add_s3_destination(
    name: String,
    endpoint: String,
    region: String,
    bucket: String,
    prefix: String,
    access_key: String,
    secret_key: String,
) -> Result<BootstrapState, String> {
    if endpoint.trim().is_empty() || bucket.trim().is_empty() {
        return Err("Endpoint 与 Bucket 必填".into());
    }
    if access_key.trim().is_empty() || secret_key.trim().is_empty() {
        return Err("Access Key / Secret Key 必填".into());
    }
    let mut cfg = load_config()?;
    cfg.destinations.push(Destination::S3 {
        id: format!("dest_{}", uuid::Uuid::new_v4()),
        name: if name.trim().is_empty() {
            "S3".into()
        } else {
            name
        },
        enabled: true,
        s3: S3Destination {
            endpoint,
            region,
            bucket,
            prefix,
            access_key,
            secret_key,
        },
    });
    save_config(&cfg)?;
    get_bootstrap()
}

async fn run_blocking<T, F>(f: F) -> Result<T, String>
where
    F: FnOnce() -> Result<T, String> + Send + 'static,
    T: Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| format!("后台任务异常: {e}"))?
}

#[tauri::command]
pub async fn test_s3_destination(
    endpoint: String,
    region: String,
    bucket: String,
    prefix: String,
    access_key: String,
    secret_key: String,
) -> Result<String, String> {
    let _ = prefix;
    run_blocking(move || {
        crate::s3::test_connection(&S3Destination {
            endpoint,
            region,
            bucket,
            prefix: String::new(),
            access_key,
            secret_key,
        })
    })
    .await
}

#[tauri::command]
pub fn remove_destination(id: String) -> Result<BootstrapState, String> {
    let mut cfg = load_config()?;
    cfg.destinations.retain(|d| d.id() != id);
    save_config(&cfg)?;
    get_bootstrap()
}

#[tauri::command]
pub fn set_destination_enabled(id: String, enabled: bool) -> Result<BootstrapState, String> {
    let mut cfg = load_config()?;
    for dest in &mut cfg.destinations {
        if dest.id() == id {
            match dest {
                Destination::Local { enabled: e, .. } => *e = enabled,
                Destination::S3 { enabled: e, .. } => *e = enabled,
            }
        }
    }
    save_config(&cfg)?;
    get_bootstrap()
}

#[tauri::command]
pub fn add_custom_source(label: String, path: String) -> Result<BootstrapState, String> {
    let mut cfg = load_config()?;
    if crate::packager::is_within(std::path::Path::new(&path), &backup_output_dirs(&cfg)) {
        return Err("该文件夹位于备份目标的 backups 目录内，不能作为备份源".into());
    }
    cfg.sources.custom.push(CustomSource {
        id: format!("custom_{}", uuid::Uuid::new_v4()),
        label: if label.trim().is_empty() {
            "custom".into()
        } else {
            label
        },
        path,
        enabled: true,
    });
    save_config(&cfg)?;
    get_bootstrap()
}

#[tauri::command]
pub fn remove_custom_source(id: String) -> Result<BootstrapState, String> {
    let mut cfg = load_config()?;
    cfg.sources.custom.retain(|c| c.id != id);
    save_config(&cfg)?;
    get_bootstrap()
}

#[tauri::command]
pub fn set_agent_enabled(key: String, enabled: bool) -> Result<BootstrapState, String> {
    let mut cfg = load_config()?;
    cfg.sources
        .agents
        .entry(key)
        .or_insert_with(|| crate::config::AgentSourceState {
            enabled: true,
            paths: Default::default(),
        })
        .enabled = enabled;
    save_config(&cfg)?;
    get_bootstrap()
}

#[tauri::command]
pub fn set_agent_path_enabled(
    key: String,
    group_id: String,
    enabled: bool,
) -> Result<BootstrapState, String> {
    let mut cfg = load_config()?;
    let entry = cfg
        .sources
        .agents
        .entry(key)
        .or_insert_with(|| crate::config::AgentSourceState {
            enabled: true,
            paths: Default::default(),
        });
    entry.paths.insert(group_id, enabled);
    save_config(&cfg)?;
    get_bootstrap()
}

#[tauri::command]
pub async fn estimate_backup_size() -> Result<BackupEstimate, String> {
    run_blocking(|| estimate_backup(&load_config()?)).await
}

pub fn run_and_notify(app: &tauri::AppHandle, trigger: &str) -> Result<BackupRunResult, String> {
    let result = load_config().and_then(|cfg| run_backup_with_app(Some(app), &cfg, trigger));
    match &result {
        Ok(r) => crate::notify::notify_and_emit(app, r),
        Err(e) => crate::notify::notify_error(app, e),
    }
    result
}

#[tauri::command]
pub async fn run_backup_now(app: tauri::AppHandle) -> Result<BackupRunResult, String> {
    run_blocking(move || run_and_notify(&app, "manual")).await
}

#[tauri::command]
pub async fn retry_backup(app: tauri::AppHandle) -> Result<BackupRunResult, String> {
    run_blocking(move || run_and_notify(&app, "retry")).await
}

#[tauri::command]
pub fn list_history(only_failed: bool) -> Result<Vec<HistoryEntry>, String> {
    crate::history::load_history(100, only_failed)
}

#[tauri::command]
pub fn get_latest_failure() -> Result<Option<HistoryEntry>, String> {
    crate::history::latest_non_ok()
}

#[derive(Debug, Serialize)]
pub struct StatusView {
    pub last_success: Option<HistoryEntry>,
    pub next_run: Option<NextRun>,
}

#[tauri::command]
pub fn get_status() -> Result<StatusView, String> {
    Ok(StatusView {
        last_success: crate::history::latest_success()?,
        next_run: crate::scheduler::next_run(&load_config()?),
    })
}

#[tauri::command]
pub fn open_config_dir() -> Result<(), String> {
    open_path_in_explorer(&config_dir()?)
}

#[tauri::command]
pub fn open_log_dir() -> Result<(), String> {
    let dir = crate::logging::log_dir()?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    open_path_in_explorer(&dir)
}

#[tauri::command]
pub fn open_presets_file() -> Result<(), String> {
    let path = write_user_presets_template_if_missing()?;
    open_path_in_explorer(path.parent().unwrap_or(std::path::Path::new(".")))
}

#[tauri::command]
pub fn reset_presets() -> Result<BootstrapState, String> {
    reset_user_presets()?;
    get_bootstrap()
}

#[tauri::command]
pub fn open_path(path: String) -> Result<(), String> {
    open_path_in_explorer(&PathBuf::from(path))
}

#[tauri::command]
pub fn get_docs_hint() -> Result<String, String> {
    Ok(include_str!("../resources/CONFIGURATION.md").to_string())
}

#[tauri::command]
pub fn update_encryption(
    enabled: bool,
    password: Option<String>,
) -> Result<BootstrapState, String> {
    let mut cfg = load_config()?;
    cfg.encryption.enabled = enabled;
    if let Some(pw) = password {
        let trimmed = pw.trim().to_string();
        cfg.encryption.password = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        };
    }
    if cfg.encryption.enabled
        && cfg
            .encryption
            .password
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .is_none()
    {
        return Err("开启加密时必须设置密码".into());
    }
    save_config(&cfg)?;
    get_bootstrap()
}

#[tauri::command]
pub fn update_schedule(enabled: bool, time_local: String) -> Result<BootstrapState, String> {
    let trimmed = time_local.trim().to_string();
    let parts: Vec<_> = trimmed.split(':').collect();
    if parts.len() != 2
        || parts[0].parse::<u32>().ok().filter(|h| *h < 24).is_none()
        || parts[1].parse::<u32>().ok().filter(|m| *m < 60).is_none()
    {
        return Err("时间格式应为 HH:mm".into());
    }
    let mut cfg = load_config()?;
    cfg.schedule.enabled = enabled;
    cfg.schedule.time_local = trimmed;
    save_config(&cfg)?;
    get_bootstrap()
}

#[tauri::command]
pub fn update_hostname_override(hostname: Option<String>) -> Result<BootstrapState, String> {
    let mut cfg = load_config()?;
    cfg.app.hostname_override = hostname
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    save_config(&cfg)?;
    get_bootstrap()
}

#[tauri::command]
pub fn set_start_on_login(
    app: tauri::AppHandle,
    enabled: bool,
) -> Result<BootstrapState, String> {
    use tauri_plugin_autostart::ManagerExt;
    let launcher = app.autolaunch();
    if enabled {
        launcher
            .enable()
            .map_err(|e| format!("启用登录启动失败: {e}"))?;
    } else {
        launcher
            .disable()
            .map_err(|e| format!("关闭登录启动失败: {e}"))?;
    }
    let mut cfg = load_config()?;
    cfg.app.start_on_login = enabled;
    save_config(&cfg)?;
    get_bootstrap()
}

#[tauri::command]
pub fn get_app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn s3_call_inside_blocking_task_returns_error_instead_of_panicking() {
        let result = tauri::async_runtime::block_on(test_s3_destination(
            "http://127.0.0.1:9".into(),
            "auto".into(),
            "bucket".into(),
            String::new(),
            "ak".into(),
            "sk".into(),
        ));
        let err = result.expect_err("unreachable endpoint must fail");
        assert!(err.contains("连接失败"), "{err}");
    }
}
