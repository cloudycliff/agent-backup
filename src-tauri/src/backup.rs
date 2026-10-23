use chrono::{DateTime, NaiveDateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, TryLockError};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

use crate::config::{history_path, resolve_hostname, slugify_label, AppConfig, Destination};
use crate::history::{load_history, HistoryEntry};
use crate::packager::{plan_agent, plan_custom, PackedSource, SourceEstimate, SourcePlan};
use crate::presets::{agent_enabled, effective_group_enabled, load_presets};

static BACKUP_LOCK: Mutex<()> = Mutex::new(());

pub const BACKUP_BUSY: &str = "已有备份正在进行，已跳过";
/// Subdirectory of a local destination that holds all backups.
pub const BACKUPS_DIR: &str = "backups";

const TIMESTAMP_FORMAT: &str = "%Y-%m-%dT%H%M%SZ";
const PACK_PERCENT_SHARE: f64 = 90.0;
const UPLOAD_PERCENT_SHARE: f64 = 9.0;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(150);
/// A scheduled run with unchanged content still writes a fresh copy once the last
/// full backup is this old, so deleted or damaged copies don't go unnoticed forever.
const UNCHANGED_MAX_AGE: TimeDelta = TimeDelta::days(7);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Ok,
    Partial,
    #[default]
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SourceRecord {
    pub key: String,
    #[serde(rename = "type")]
    pub source_type: String,
    pub label: String,
    pub enabled_paths: Option<Vec<String>>,
    pub root_path: String,
    pub archive: String,
    pub bytes: u64,
    pub sha256: String,
    pub archive_sha256: String,
    pub file_count: u64,
    pub status: Status,
    pub error: Option<String>,
    pub warnings: Vec<String>,
}

impl From<&PackedSource> for SourceRecord {
    fn from(p: &PackedSource) -> Self {
        Self {
            key: p.key.clone(),
            source_type: p.source_type.to_string(),
            label: p.label.clone(),
            enabled_paths: p.enabled_paths.clone(),
            root_path: p.root_path.to_string_lossy().to_string(),
            archive: p.archive_name.clone(),
            bytes: p.bytes,
            sha256: p.sha256.clone(),
            archive_sha256: p.archive_sha256.clone(),
            file_count: p.file_count,
            status: p.status,
            error: p.error.clone(),
            warnings: p.warnings.clone(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DestResult {
    pub id: String,
    #[serde(rename = "type")]
    pub dest_type: String,
    pub name: String,
    pub status: Status,
    pub error: Option<String>,
    pub uri: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupRunResult {
    pub id: String,
    pub created_at: String,
    pub hostname: String,
    pub trigger: String,
    pub overall_status: Status,
    pub encryption_enabled: bool,
    pub work_dir: String,
    pub sources: Vec<SourceRecord>,
    pub destinations: Vec<DestResult>,
    pub message: String,
}

impl BackupRunResult {
    /// Result reported to the UI when a run could not start or aborted early.
    pub fn failure(message: &str) -> Self {
        Self {
            id: String::new(),
            created_at: String::new(),
            hostname: String::new(),
            trigger: "manual".into(),
            overall_status: Status::Failed,
            encryption_enabled: false,
            work_dir: String::new(),
            sources: Vec::new(),
            destinations: Vec::new(),
            message: message.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct BackupProgress {
    pub phase: String,
    pub current: u32,
    pub total: u32,
    pub label: String,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub percent: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct BackupEstimate {
    pub file_count: u64,
    pub bytes: u64,
    pub sources: Vec<SourceEstimate>,
}

/// Temp staging directory, removed on every exit path.
struct WorkDir(PathBuf);

impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn encryption_password(cfg: &AppConfig) -> Result<Option<String>, String> {
    if !cfg.encryption.enabled {
        return Ok(None);
    }
    let pw = cfg
        .encryption
        .password
        .clone()
        .filter(|s| !s.trim().is_empty());
    if pw.is_none() {
        return Err("已开启加密，请先在设置中设置密码".into());
    }
    Ok(pw)
}

fn emit_progress(
    app: Option<&AppHandle>,
    phase: &str,
    current: u32,
    total: u32,
    label: &str,
    bytes: (u64, u64),
    percent: u32,
) {
    if let Some(app) = app {
        let _ = app.emit(
            "backup-progress",
            &BackupProgress {
                phase: phase.into(),
                current,
                total: total.max(1),
                label: label.into(),
                bytes_done: bytes.0,
                bytes_total: bytes.1,
                percent,
            },
        );
    }
}

fn pack_percent(done: u64, total: u64) -> u32 {
    if total == 0 {
        0
    } else {
        ((done.min(total) as f64 / total as f64) * PACK_PERCENT_SHARE).round() as u32
    }
}

struct Throttle(Option<Instant>);

impl Throttle {
    fn ready(&mut self) -> bool {
        let now = Instant::now();
        if self.0.is_some_and(|t| now.duration_since(t) < PROGRESS_INTERVAL) {
            return false;
        }
        self.0 = Some(now);
        true
    }
}

/// Every enabled source with its file list collected; the single place that decides what gets backed up.
fn plan_sources(cfg: &AppConfig) -> Result<Vec<SourcePlan>, String> {
    let agents = load_presets()?.agents;
    let skip_dirs = backup_output_dirs(cfg);
    let mut plans = Vec::new();
    for agent in agents
        .iter()
        .filter(|a| !a.disabled && a.installed && agent_enabled(cfg, &a.key))
    {
        let groups: Vec<String> = agent
            .groups
            .iter()
            .filter(|g| effective_group_enabled(cfg, &agent.key, g))
            .map(|g| g.id.clone())
            .collect();
        if !groups.is_empty() {
            plans.push(plan_agent(agent, &groups, &cfg.exclusions, &skip_dirs));
        }
    }
    for custom in cfg.sources.custom.iter().filter(|c| c.enabled) {
        let fallback = format!("custom-{}", &custom.id[..8.min(custom.id.len())]);
        let slug = slugify_label(&custom.label, &fallback);
        plans.push(plan_custom(
            &custom.id,
            &custom.label,
            Path::new(&custom.path),
            &slug,
            &cfg.exclusions,
            &skip_dirs,
        ));
    }
    Ok(plans)
}

/// Canonical `backups/` directories of every local destination that has written one.
pub fn backup_output_dirs(cfg: &AppConfig) -> Vec<PathBuf> {
    cfg.destinations
        .iter()
        .filter_map(|d| match d {
            Destination::Local { local, .. } => {
                fs::canonicalize(Path::new(&local.root_path).join(BACKUPS_DIR)).ok()
            }
            Destination::S3 { .. } => None,
        })
        .collect()
}

pub fn estimate_backup(cfg: &AppConfig) -> Result<BackupEstimate, String> {
    let sources: Vec<SourceEstimate> = plan_sources(cfg)?.iter().map(SourcePlan::estimate).collect();
    Ok(BackupEstimate {
        file_count: sources.iter().map(|s| s.file_count).sum(),
        bytes: sources.iter().map(|s| s.bytes).sum(),
        sources,
    })
}

pub fn run_backup_with_app(
    app: Option<&AppHandle>,
    cfg: &AppConfig,
    trigger: &str,
) -> Result<BackupRunResult, String> {
    let _guard = match BACKUP_LOCK.try_lock() {
        Ok(g) => g,
        Err(TryLockError::Poisoned(p)) => p.into_inner(),
        Err(TryLockError::WouldBlock) => {
            log::info!("{trigger} backup skipped: another backup is running");
            return Err(BACKUP_BUSY.to_string());
        }
    };
    log::info!("{trigger} backup started");
    let started = Instant::now();
    let result = run_backup_inner(app, cfg, trigger);
    let secs = started.elapsed().as_secs_f64();
    match &result {
        Ok(r) => log::info!(
            "{trigger} backup finished in {secs:.1}s: {:?}, {}",
            r.overall_status,
            r.message
        ),
        Err(e) => log::error!("{trigger} backup failed after {secs:.1}s: {e}"),
    }
    result
}

fn local_target(root: &str, hostname: &str, timestamp: &str) -> PathBuf {
    PathBuf::from(root).join(BACKUPS_DIR).join(hostname).join(timestamp)
}

fn run_backup_inner(
    app: Option<&AppHandle>,
    cfg: &AppConfig,
    trigger: &str,
) -> Result<BackupRunResult, String> {
    let password = encryption_password(cfg)?;
    let enabled_dests: Vec<&Destination> = cfg.destinations.iter().filter(|d| d.enabled()).collect();
    if enabled_dests.is_empty() {
        return Err("请先添加并启用至少一个备份目标".into());
    }

    emit_progress(app, "estimating", 0, 1, "正在扫描文件…", (0, 0), 0);
    let plans = plan_sources(cfg)?;
    let raw_total: u64 = plans.iter().map(|p| p.bytes).sum();

    let timestamp = Utc::now().format(TIMESTAMP_FORMAT).to_string();
    let hostname = resolve_hostname(cfg);
    let work = WorkDir(std::env::temp_dir().join("agent-backup").join(&timestamp));
    fs::create_dir_all(&work.0).map_err(|e| e.to_string())?;

    let pack_total = plans.len() as u32;
    let mut packed: Vec<PackedSource> = Vec::with_capacity(plans.len());
    let mut raw_done = 0u64;
    let mut throttle = Throttle(None);
    for (i, plan) in plans.into_iter().enumerate() {
        let current = i as u32 + 1;
        let label = format!("打包 {}", plan.label);
        let report = |done: u64| {
            emit_progress(app, "packing", current, pack_total, &label, (done, raw_total), pack_percent(done, raw_total));
        };
        report(raw_done);
        packed.push(plan.pack(&work.0, &timestamp, password.as_deref(), &mut |n| {
            raw_done += n;
            if throttle.ready() {
                report(raw_done);
            }
        }));
    }

    for p in &packed {
        match (&p.error, p.warnings.first()) {
            (Some(e), _) => log::warn!("source {} failed: {e}", p.key),
            (None, Some(first)) => log::warn!(
                "source {}: {} warning(s), first: {first}",
                p.key,
                p.warnings.len()
            ),
            (None, None) => {}
        }
    }
    if !packed.iter().any(|p| p.status == Status::Ok) {
        return Err("没有可备份的内容（请检查勾选与路径）".into());
    }

    let dest_ids: Vec<&str> = enabled_dests.iter().map(|d| d.id()).collect();
    if trigger == "schedule" {
        let history = load_history(50, false).unwrap_or_default();
        if let Some(base) =
            unchanged_baseline(cfg, &hostname, &packed, &dest_ids, &history, Utc::now())
        {
            let result = BackupRunResult {
                id: format!("run_{}", uuid::Uuid::new_v4()),
                created_at: timestamp,
                hostname,
                trigger: trigger.to_string(),
                overall_status: Status::Ok,
                encryption_enabled: cfg.encryption.enabled,
                work_dir: work.0.to_string_lossy().to_string(),
                sources: packed.iter().map(SourceRecord::from).collect(),
                destinations: Vec::new(),
                message: format!("内容与 {} 的备份相同，本次未写入新副本", base.created_at),
            };
            append_history(&result)?;
            emit_progress(app, "done", 1, 1, "完成（内容无变化）", (raw_done, raw_total), 100);
            return Ok(result);
        }
    }

    let dest_total = enabled_dests.len();
    let mut dest_results = Vec::with_capacity(dest_total);
    for (i, dest) in enabled_dests.iter().enumerate() {
        let percent = PACK_PERCENT_SHARE + UPLOAD_PERCENT_SHARE * i as f64 / dest_total as f64;
        emit_progress(
            app,
            "uploading",
            i as u32 + 1,
            dest_total as u32,
            &format!("写入目标 {}", dest.name()),
            (0, 0),
            percent.round() as u32,
        );
        let outcome = match dest {
            Destination::Local { local, .. } => {
                let target = local_target(&local.root_path, &hostname, &timestamp);
                write_to_local(&target, &packed)
            }
            Destination::S3 { s3, .. } => {
                crate::s3::upload_archives(s3, &hostname, &timestamp, &packed)
            }
        };
        let (status, error, uri) = match outcome {
            Ok(uri) => (Status::Ok, None, Some(uri)),
            Err(e) => {
                log::error!("destination {} failed: {e}", dest.name());
                (Status::Failed, Some(e), None)
            }
        };
        dest_results.push(DestResult {
            id: dest.id().to_string(),
            dest_type: dest.dest_type().to_string(),
            name: dest.name().to_string(),
            status,
            error,
            uri,
        });
    }

    emit_progress(app, "finishing", 1, 1, "写入清单…", (0, 0), 99);
    let summary = Summary::new(&packed, &dest_results);
    let manifest_json =
        build_manifest_value(&packed, cfg, &hostname, &timestamp, &dest_results, &summary);
    let manifest_bytes = serde_json::to_vec_pretty(&manifest_json).map_err(|e| e.to_string())?;
    for (dest, res) in enabled_dests.iter().zip(&dest_results) {
        if res.status != Status::Ok {
            continue;
        }
        let written = match dest {
            Destination::Local { local, .. } => fs::write(
                local_target(&local.root_path, &hostname, &timestamp).join("manifest.json"),
                &manifest_bytes,
            )
            .map_err(|e| e.to_string()),
            Destination::S3 { s3, .. } => {
                crate::s3::upload_manifest(s3, &hostname, &timestamp, &manifest_bytes)
            }
        };
        if let Err(e) = written {
            log::warn!("write manifest to {} failed: {e}", dest.name());
        }
    }

    let result = BackupRunResult {
        id: format!("run_{}", uuid::Uuid::new_v4()),
        created_at: timestamp,
        hostname,
        trigger: trigger.to_string(),
        overall_status: summary.overall,
        encryption_enabled: cfg.encryption.enabled,
        work_dir: work.0.to_string_lossy().to_string(),
        sources: packed.iter().map(SourceRecord::from).collect(),
        destinations: dest_results,
        message: summary.message(),
    };
    append_history(&result)?;
    emit_progress(app, "done", 1, 1, "完成", (raw_done, raw_total), 100);
    Ok(result)
}

/// The most recent fully successful backup whose content, destinations and settings
/// match this run, if writing another copy would be redundant.
fn unchanged_baseline<'h>(
    cfg: &AppConfig,
    hostname: &str,
    packed: &[PackedSource],
    dest_ids: &[&str],
    history: &'h [HistoryEntry],
    now: DateTime<Utc>,
) -> Option<&'h HistoryEntry> {
    if packed
        .iter()
        .any(|p| p.status == Status::Failed || !p.warnings.is_empty())
    {
        return None;
    }
    let current: BTreeMap<&str, &str> = packed
        .iter()
        .filter(|p| p.status == Status::Ok)
        .map(|p| (p.key.as_str(), p.sha256.as_str()))
        .collect();
    let base = history
        .iter()
        .find(|e| e.overall_status == Status::Ok && !e.destinations.is_empty())?;

    let written_at = NaiveDateTime::parse_from_str(&base.created_at, TIMESTAMP_FORMAT)
        .ok()?
        .and_utc();
    if now - written_at > UNCHANGED_MAX_AGE
        || base.hostname != hostname
        || base.encryption_enabled != Some(cfg.encryption.enabled)
    {
        return None;
    }
    let base_dests: BTreeSet<&str> = base
        .destinations
        .iter()
        .filter(|d| d.status == Status::Ok)
        .map(|d| d.id.as_str())
        .collect();
    let base_sources: BTreeMap<&str, &str> = base
        .sources
        .iter()
        .filter(|s| s.status == Status::Ok)
        .map(|s| (s.key.as_str(), s.sha256.as_str()))
        .collect();
    if base_dests != dest_ids.iter().copied().collect() || base_sources != current {
        return None;
    }
    let local_copies_present = base
        .destinations
        .iter()
        .filter(|d| d.dest_type == "local")
        .all(|d| {
            d.uri
                .as_deref()
                .is_some_and(|u| Path::new(u).join("manifest.json").exists())
        });
    local_copies_present.then_some(base)
}

/// Copies into `<target>.partial` and renames when complete, so an interrupted
/// copy never looks like a finished backup.
fn write_to_local(target: &Path, packed: &[PackedSource]) -> Result<String, String> {
    let mut partial_name = target.file_name().unwrap_or_default().to_os_string();
    partial_name.push(".partial");
    let partial = target.with_file_name(partial_name);
    let copy_all = || -> Result<(), String> {
        fs::create_dir_all(&partial).map_err(|e| format!("创建目标目录失败: {e}"))?;
        for p in packed.iter().filter(|p| p.status == Status::Ok) {
            fs::copy(&p.archive_path, partial.join(&p.archive_name))
                .map_err(|e| format!("复制 {} 失败: {e}", p.archive_name))?;
        }
        fs::rename(&partial, target).map_err(|e| format!("完成目标目录失败: {e}"))
    };
    if let Err(e) = copy_all() {
        let _ = fs::remove_dir_all(&partial);
        return Err(e);
    }
    Ok(target.to_string_lossy().to_string())
}

struct Summary {
    sources_ok: usize,
    sources_failed: usize,
    sources_warned: usize,
    destinations_ok: usize,
    destinations_failed: usize,
    overall: Status,
}

impl Summary {
    fn new(packed: &[PackedSource], dest_results: &[DestResult]) -> Self {
        let count_dest = |s: Status| dest_results.iter().filter(|d| d.status == s).count();
        let sources_ok = packed.iter().filter(|p| p.status == Status::Ok).count();
        let sources_failed = packed.iter().filter(|p| p.status == Status::Failed).count();
        let sources_warned = packed.iter().filter(|p| !p.warnings.is_empty()).count();
        let destinations_ok = count_dest(Status::Ok);
        let destinations_failed = count_dest(Status::Failed);
        let overall = if destinations_ok == 0 {
            Status::Failed
        } else if destinations_failed == 0 && sources_failed == 0 && sources_warned == 0 {
            Status::Ok
        } else {
            Status::Partial
        };
        Self {
            sources_ok,
            sources_failed,
            sources_warned,
            destinations_ok,
            destinations_failed,
            overall,
        }
    }

    fn message(&self) -> String {
        let mut msg = format!(
            "源成功 {}，源失败 {}；目标成功 {}，目标失败 {}",
            self.sources_ok, self.sources_failed, self.destinations_ok, self.destinations_failed
        );
        if self.sources_warned > 0 {
            msg.push_str(&format!("；{} 个源有文件被跳过", self.sources_warned));
        }
        msg
    }
}

fn build_manifest_value(
    packed: &[PackedSource],
    cfg: &AppConfig,
    hostname: &str,
    timestamp: &str,
    dest_results: &[DestResult],
    summary: &Summary,
) -> serde_json::Value {
    json!({
        "manifest_version": 1,
        "app_version": env!("CARGO_PKG_VERSION"),
        "created_at": timestamp,
        "hostname": hostname,
        "encryption": {
            "enabled": cfg.encryption.enabled,
            "algorithm": if cfg.encryption.enabled { "zip-aes256" } else { "none" }
        },
        "sources": packed.iter().map(SourceRecord::from).collect::<Vec<_>>(),
        "destinations": dest_results,
        "summary": {
            "sources_ok": summary.sources_ok,
            "sources_failed": summary.sources_failed,
            "sources_warned": summary.sources_warned,
            "destinations_ok": summary.destinations_ok,
            "destinations_failed": summary.destinations_failed,
            "overall_status": summary.overall,
        }
    })
}

fn append_history(result: &BackupRunResult) -> Result<(), String> {
    let entry = HistoryEntry {
        id: result.id.clone(),
        created_at: result.created_at.clone(),
        hostname: result.hostname.clone(),
        trigger: result.trigger.clone(),
        overall_status: result.overall_status,
        encryption_enabled: Some(result.encryption_enabled),
        message: result.message.clone(),
        sources: result.sources.clone(),
        destinations: result.destinations.clone(),
    };
    let mut line = serde_json::to_string(&entry).map_err(|e| e.to_string())?;
    line.push('\n');
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(history_path()?)
        .map_err(|e| e.to_string())?;
    file.write_all(line.as_bytes())
        .map_err(|e| format!("写入历史失败: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("agent-backup-{name}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn packed(key: &str, sha: &str) -> PackedSource {
        PackedSource {
            key: key.into(),
            source_type: "agent",
            label: key.into(),
            enabled_paths: None,
            root_path: PathBuf::new(),
            archive_name: format!("{key}.zip"),
            archive_path: PathBuf::new(),
            bytes: 1,
            sha256: sha.into(),
            archive_sha256: String::new(),
            file_count: 1,
            status: Status::Ok,
            error: None,
            warnings: Vec::new(),
        }
    }

    fn entry(created_at: &str, sha: &str, local_uri: &Path) -> HistoryEntry {
        HistoryEntry {
            id: "run".into(),
            created_at: created_at.into(),
            hostname: "host".into(),
            trigger: "manual".into(),
            overall_status: Status::Ok,
            encryption_enabled: Some(true),
            message: String::new(),
            sources: vec![SourceRecord {
                key: "cursor".into(),
                sha256: sha.into(),
                status: Status::Ok,
                ..Default::default()
            }],
            destinations: vec![DestResult {
                id: "d1".into(),
                dest_type: "local".into(),
                status: Status::Ok,
                uri: Some(local_uri.to_string_lossy().into()),
                ..Default::default()
            }],
        }
    }

    fn at(s: &str) -> DateTime<Utc> {
        NaiveDateTime::parse_from_str(s, TIMESTAMP_FORMAT).unwrap().and_utc()
    }

    #[test]
    fn unchanged_baseline_only_when_everything_matches() {
        let copy = temp_dir("baseline");
        fs::write(copy.join("manifest.json"), "{}").unwrap();
        let cfg = AppConfig::default();
        let now = at("2026-10-23T120000Z");
        let same = [packed("cursor", "aa")];
        let history = [entry("2026-10-22T210000Z", "aa", &copy)];
        let check = |cfg: &AppConfig, packed: &[PackedSource], dests: &[&str], h: &[HistoryEntry]| {
            unchanged_baseline(cfg, "host", packed, dests, h, now).is_some()
        };

        assert!(check(&cfg, &same, &["d1"], &history));
        assert!(!check(&cfg, &[packed("cursor", "bb")], &["d1"], &history), "content changed");
        assert!(!check(&cfg, &same, &["d1", "d2"], &history), "new destination");
        assert!(!check(&cfg, &same, &["d1"], &[entry("2026-10-10T210000Z", "aa", &copy)]), "too old");

        let mut plain = cfg.clone();
        plain.encryption.enabled = false;
        assert!(!check(&plain, &same, &["d1"], &history), "encryption toggled");

        let mut warned = packed("cursor", "aa");
        warned.warnings.push("skipped".into());
        assert!(!check(&cfg, &[warned], &["d1"], &history), "has warnings");

        let mut skip_entry = entry("2026-10-23T090000Z", "aa", &copy);
        skip_entry.destinations.clear();
        let with_skip = [skip_entry, history[0].clone()];
        let base = unchanged_baseline(&cfg, "host", &same, &["d1"], &with_skip, now).unwrap();
        assert_eq!(base.created_at, "2026-10-22T210000Z", "skip entries are never baselines");

        fs::remove_file(copy.join("manifest.json")).unwrap();
        assert!(!check(&cfg, &same, &["d1"], &history), "local copy deleted");
        let _ = fs::remove_dir_all(&copy);
    }

    #[test]
    fn local_write_is_all_or_nothing() {
        let work = temp_dir("work");
        fs::write(work.join("a.zip"), "a").unwrap();
        let mut ok = packed("a", "x");
        ok.archive_path = work.join("a.zip");
        let mut missing = packed("b", "y");
        missing.archive_path = work.join("missing.zip");

        let dest = temp_dir("dest");
        let target = dest.join("2026-10-23T120000Z");
        assert!(write_to_local(&target, &[ok.clone(), missing]).is_err());
        assert_eq!(fs::read_dir(&dest).unwrap().count(), 0, "no partial or final dir left");

        write_to_local(&target, &[ok]).unwrap();
        assert!(target.join("a.zip").exists());
        assert_eq!(fs::read_dir(&dest).unwrap().count(), 1);
        let _ = fs::remove_dir_all(&work);
        let _ = fs::remove_dir_all(&dest);
    }
}
