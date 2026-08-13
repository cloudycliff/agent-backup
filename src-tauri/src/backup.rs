use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::config::{history_path, resolve_hostname, slugify_label, AppConfig, Destination};
use crate::packager::{pack_agent, pack_custom, PackedSource};
use crate::presets::{
    agent_enabled, effective_group_enabled, load_merged_presets, resolve_agents,
};

static BACKUP_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupRunResult {
    pub id: String,
    pub created_at: String,
    pub hostname: String,
    pub trigger: String,
    pub overall_status: String,
    pub work_dir: String,
    pub sources: Vec<serde_json::Value>,
    pub destinations: Vec<serde_json::Value>,
    pub message: String,
}

fn encryption_password(cfg: &AppConfig) -> Result<Option<String>, String> {
    if !cfg.encryption.enabled {
        return Ok(None);
    }
    let pw = cfg
        .encryption
        .password
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    if pw.is_none() {
        return Err("已开启加密，请先在设置中设置密码".into());
    }
    Ok(pw)
}

pub fn run_backup(cfg: &AppConfig, trigger: &str) -> Result<BackupRunResult, String> {
    let _guard = BACKUP_LOCK
        .try_lock()
        .map_err(|_| "已有备份正在进行，已跳过".to_string())?;
    run_backup_inner(cfg, trigger)
}

fn run_backup_inner(cfg: &AppConfig, trigger: &str) -> Result<BackupRunResult, String> {
    let password = encryption_password(cfg)?;
    let password_ref = password.as_deref();

    let enabled_dests: Vec<&Destination> = cfg.destinations.iter().filter(|d| d.enabled()).collect();
    if enabled_dests.is_empty() {
        return Err("请先添加并启用至少一个备份目标".into());
    }

    let timestamp = Utc::now().format("%Y-%m-%dT%H%M%SZ").to_string();
    let hostname = resolve_hostname(cfg);
    let presets = load_merged_presets()?;
    let agents = resolve_agents(&presets)?;

    let work_root = std::env::temp_dir().join("agent-backup").join(&timestamp);
    fs::create_dir_all(&work_root).map_err(|e| e.to_string())?;

    let mut packed: Vec<PackedSource> = Vec::new();

    for agent in agents.iter().filter(|a| !a.disabled && a.installed) {
        if !agent_enabled(cfg, &agent.key) {
            continue;
        }
        let enabled_groups: Vec<String> = agent
            .groups
            .iter()
            .filter(|g| effective_group_enabled(cfg, &agent.key, g))
            .map(|g| g.id.clone())
            .collect();
        if enabled_groups.is_empty() {
            continue;
        }
        packed.push(pack_agent(
            agent,
            &enabled_groups,
            &cfg.exclusions,
            &work_root,
            &timestamp,
            password_ref,
        ));
    }

    for custom in cfg.sources.custom.iter().filter(|c| c.enabled) {
        let fallback = format!("custom-{}", &custom.id[..8.min(custom.id.len())]);
        let slug = slugify_label(&custom.label, &fallback);
        packed.push(pack_custom(
            &custom.id,
            &custom.label,
            Path::new(&custom.path),
            &cfg.exclusions,
            &work_root,
            &timestamp,
            &slug,
            password_ref,
        ));
    }

    if !packed.iter().any(|p| p.status == "ok") {
        let _ = fs::remove_dir_all(&work_root);
        return Err("没有可备份的内容（请检查勾选与路径）".into());
    }

    let mut dest_results = Vec::new();
    for dest in &enabled_dests {
        match dest {
            Destination::Local { id, name, local, .. } => {
                let target = PathBuf::from(&local.root_path)
                    .join("backups")
                    .join(&hostname)
                    .join(&timestamp);
                match write_to_local(&target, &packed) {
                    Ok(uri) => dest_results.push(json!({
                        "id": id,
                        "type": "local",
                        "name": name,
                        "status": "ok",
                        "error": null,
                        "uri": uri,
                    })),
                    Err(e) => dest_results.push(json!({
                        "id": id,
                        "type": "local",
                        "name": name,
                        "status": "failed",
                        "error": e,
                        "uri": target.to_string_lossy(),
                    })),
                }
            }
            Destination::S3 { id, name, s3, .. } => {
                match crate::s3::upload_archives(s3, &hostname, &timestamp, &packed) {
                    Ok(uri) => dest_results.push(json!({
                        "id": id,
                        "type": "s3",
                        "name": name,
                        "status": "ok",
                        "error": null,
                        "uri": uri,
                    })),
                    Err(e) => dest_results.push(json!({
                        "id": id,
                        "type": "s3",
                        "name": name,
                        "status": "failed",
                        "error": e,
                        "uri": null,
                    })),
                }
            }
        }
    }

    let manifest_json = build_manifest_value(&packed, cfg, &hostname, &timestamp, &dest_results);
    let manifest_bytes =
        serde_json::to_vec_pretty(&manifest_json).map_err(|e| e.to_string())?;

    for dest in &enabled_dests {
        let ok = dest_results.iter().any(|d| {
            d.get("id").and_then(|v| v.as_str()) == Some(dest.id())
                && d.get("status").and_then(|v| v.as_str()) == Some("ok")
        });
        if !ok {
            continue;
        }
        match dest {
            Destination::Local { local, .. } => {
                let target = PathBuf::from(&local.root_path)
                    .join("backups")
                    .join(&hostname)
                    .join(&timestamp);
                if let Err(e) = fs::write(target.join("manifest.json"), &manifest_bytes) {
                    eprintln!("write local manifest failed: {e}");
                }
            }
            Destination::S3 { s3, .. } => {
                if let Err(e) =
                    crate::s3::upload_manifest(s3, &hostname, &timestamp, &manifest_bytes)
                {
                    eprintln!("upload s3 manifest failed: {e}");
                }
            }
        }
    }

    let sources_ok = packed.iter().filter(|p| p.status == "ok").count();
    let sources_failed = packed.iter().filter(|p| p.status == "failed").count();
    let destinations_ok = dest_results
        .iter()
        .filter(|d| d.get("status").and_then(|v| v.as_str()) == Some("ok"))
        .count();
    let destinations_failed = dest_results
        .iter()
        .filter(|d| d.get("status").and_then(|v| v.as_str()) == Some("failed"))
        .count();

    let overall = if destinations_ok > 0 && destinations_failed == 0 && sources_failed == 0 {
        "ok"
    } else if destinations_ok > 0 {
        "partial"
    } else {
        "failed"
    };

    let source_values: Vec<serde_json::Value> = packed
        .iter()
        .map(|p| {
            json!({
                "key": p.key,
                "type": p.source_type,
                "label": p.label,
                "enabled_paths": p.enabled_paths,
                "root_path": p.root_path.to_string_lossy(),
                "archive": p.archive_name,
                "bytes": p.bytes,
                "sha256": p.sha256,
                "file_count": p.file_count,
                "status": p.status,
                "error": p.error,
            })
        })
        .collect();

    let run_id = format!("run_{}", uuid::Uuid::new_v4());
    let result = BackupRunResult {
        id: run_id,
        created_at: timestamp,
        hostname,
        trigger: trigger.to_string(),
        overall_status: overall.to_string(),
        work_dir: work_root.to_string_lossy().to_string(),
        sources: source_values,
        destinations: dest_results,
        message: format!(
            "源成功 {sources_ok}，源失败 {sources_failed}；目标成功 {destinations_ok}，目标失败 {destinations_failed}"
        ),
    };

    append_history(&result)?;
    let _ = fs::remove_dir_all(&work_root);
    Ok(result)
}

fn write_to_local(target: &Path, packed: &[PackedSource]) -> Result<String, String> {
    fs::create_dir_all(target).map_err(|e| format!("创建目标目录失败: {e}"))?;
    for p in packed.iter().filter(|p| p.status == "ok") {
        let dest_file = target.join(&p.archive_name);
        fs::copy(&p.archive_path, &dest_file)
            .map_err(|e| format!("复制 {} 失败: {e}", p.archive_name))?;
    }
    Ok(target.to_string_lossy().to_string())
}

fn build_manifest_value(
    packed: &[PackedSource],
    cfg: &AppConfig,
    hostname: &str,
    timestamp: &str,
    dest_results: &[serde_json::Value],
) -> serde_json::Value {
    let sources_ok = packed.iter().filter(|p| p.status == "ok").count();
    let sources_failed = packed.iter().filter(|p| p.status == "failed").count();
    let destinations_ok = dest_results
        .iter()
        .filter(|d| d.get("status").and_then(|v| v.as_str()) == Some("ok"))
        .count();
    let destinations_failed = dest_results
        .iter()
        .filter(|d| d.get("status").and_then(|v| v.as_str()) == Some("failed"))
        .count();
    let overall = if destinations_ok > 0 && destinations_failed == 0 && sources_failed == 0 {
        "ok"
    } else if destinations_ok > 0 {
        "partial"
    } else {
        "failed"
    };

    json!({
        "manifest_version": 1,
        "app_version": env!("CARGO_PKG_VERSION"),
        "created_at": timestamp,
        "hostname": hostname,
        "encryption": {
            "enabled": cfg.encryption.enabled,
            "algorithm": if cfg.encryption.enabled { "zip-aes256" } else { "none" }
        },
        "sources": packed.iter().map(|p| json!({
            "key": p.key,
            "type": p.source_type,
            "label": p.label,
            "enabled_paths": p.enabled_paths,
            "root_path": p.root_path.to_string_lossy(),
            "archive": p.archive_name,
            "bytes": p.bytes,
            "sha256": p.sha256,
            "file_count": p.file_count,
            "status": p.status,
            "error": p.error,
        })).collect::<Vec<_>>(),
        "destinations": dest_results,
        "summary": {
            "sources_ok": sources_ok,
            "sources_failed": sources_failed,
            "destinations_ok": destinations_ok,
            "destinations_failed": destinations_failed,
            "overall_status": overall,
        }
    })
}

fn append_history(result: &BackupRunResult) -> Result<(), String> {
    let path = history_path()?;
    let mut line = serde_json::to_string(&json!({
        "id": result.id,
        "created_at": result.created_at,
        "hostname": result.hostname,
        "trigger": result.trigger,
        "overall_status": result.overall_status,
        "message": result.message,
        "sources": result.sources,
        "destinations": result.destinations,
    }))
    .map_err(|e| e.to_string())?;
    line.push('\n');
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| e.to_string())?;
    file.write_all(line.as_bytes())
        .map_err(|e| format!("写入历史失败: {e}"))?;
    Ok(())
}
