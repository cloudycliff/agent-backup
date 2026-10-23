use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub version: u32,
    pub app: AppSettings,
    pub encryption: EncryptionSettings,
    pub schedule: ScheduleSettings,
    pub sources: SourcesSettings,
    pub destinations: Vec<Destination>,
    pub exclusions: ExclusionSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub hostname_override: Option<String>,
    pub start_on_login: bool,
    pub minimize_to_tray: bool,
    pub language: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EncryptionSettings {
    pub enabled: bool,
    pub password: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ScheduleSettings {
    pub enabled: bool,
    pub time_local: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SourcesSettings {
    pub agents: HashMap<String, AgentSourceState>,
    pub custom: Vec<CustomSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSourceState {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub paths: HashMap<String, bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomSource {
    pub id: String,
    pub label: String,
    pub path: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Destination {
    Local {
        id: String,
        name: String,
        enabled: bool,
        local: LocalDestination,
    },
    S3 {
        id: String,
        name: String,
        enabled: bool,
        s3: S3Destination,
    },
}

impl Destination {
    pub fn id(&self) -> &str {
        match self {
            Destination::Local { id, .. } => id,
            Destination::S3 { id, .. } => id,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Destination::Local { name, .. } => name,
            Destination::S3 { name, .. } => name,
        }
    }

    pub fn enabled(&self) -> bool {
        match self {
            Destination::Local { enabled, .. } => *enabled,
            Destination::S3 { enabled, .. } => *enabled,
        }
    }

    pub fn dest_type(&self) -> &'static str {
        match self {
            Destination::Local { .. } => "local",
            Destination::S3 { .. } => "s3",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalDestination {
    pub root_path: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct S3Destination {
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub prefix: String,
    pub access_key: String,
    pub secret_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ExclusionSettings {
    pub file_name_globs: Vec<String>,
    pub dir_name_globs: Vec<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version: 1,
            app: AppSettings::default(),
            encryption: EncryptionSettings::default(),
            schedule: ScheduleSettings::default(),
            sources: SourcesSettings::default(),
            destinations: Vec::new(),
            exclusions: ExclusionSettings::default(),
        }
    }
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            hostname_override: None,
            start_on_login: false,
            minimize_to_tray: true,
            language: "zh-CN".into(),
        }
    }
}

impl Default for EncryptionSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            password: None,
        }
    }
}

impl Default for ScheduleSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            time_local: "21:00".into(),
        }
    }
}

impl Default for ExclusionSettings {
    fn default() -> Self {
        Self {
            file_name_globs: vec![
                "*.pem".into(),
                "*.key".into(),
                "*credentials*".into(),
                "*secret*".into(),
                ".env".into(),
                ".env.*".into(),
            ],
            dir_name_globs: vec![
                "Cache".into(),
                "CachedData".into(),
                "GPUCache".into(),
                "Code Cache".into(),
                "logs".into(),
                "tmp".into(),
                "temp".into(),
            ],
        }
    }
}

/// Writes via a sibling temp file and rename, so a crash never leaves a half-written file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut tmp_name = path.file_name().unwrap_or_default().to_os_string();
    tmp_name.push(".tmp");
    let tmp = path.with_file_name(tmp_name);
    fs::write(&tmp, bytes).map_err(|e| format!("写入 {} 失败: {e}", tmp.display()))?;
    fs::rename(&tmp, path).map_err(|e| format!("替换 {} 失败: {e}", path.display()))
}

pub fn config_dir() -> Result<PathBuf, String> {
    let dir = match std::env::var_os("AGENT_BACKUP_CONFIG_DIR").filter(|v| !v.is_empty()) {
        Some(custom) => PathBuf::from(custom),
        None => dirs_home()
            .ok_or_else(|| "无法解析用户主目录".to_string())?
            .join(".agent-backup"),
    };
    fs::create_dir_all(&dir).map_err(|e| format!("创建配置目录失败: {e}"))?;
    Ok(dir)
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

pub fn config_path() -> Result<PathBuf, String> {
    Ok(config_dir()?.join("config.json"))
}

pub fn user_presets_path() -> Result<PathBuf, String> {
    Ok(config_dir()?.join("agent-presets.json"))
}

pub fn history_path() -> Result<PathBuf, String> {
    Ok(config_dir()?.join("history.jsonl"))
}

pub fn load_config() -> Result<AppConfig, String> {
    let path = config_path()?;
    if !path.exists() {
        let cfg = AppConfig::default();
        save_config(&cfg)?;
        return Ok(cfg);
    }
    let raw = fs::read_to_string(&path).map_err(|e| format!("读取配置失败: {e}"))?;
    serde_json::from_str(&raw).map_err(|e| format!("解析配置失败: {e}"))
}

pub fn save_config(cfg: &AppConfig) -> Result<(), String> {
    let raw = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    write_atomic(&config_path()?, raw.as_bytes()).map_err(|e| format!("写入配置失败: {e}"))
}

/// Fills in toggles for agents/groups the config has not seen yet. Returns whether anything changed.
pub fn ensure_agent_defaults(cfg: &mut AppConfig, presets: &crate::presets::PresetFile) -> bool {
    let mut changed = false;
    for agent in &presets.agents {
        if agent.disabled {
            continue;
        }
        let entry = cfg
            .sources
            .agents
            .entry(agent.key.clone())
            .or_insert_with(|| {
                changed = true;
                AgentSourceState {
                    enabled: true,
                    paths: HashMap::new(),
                }
            });
        for group in agent.groups.iter().flatten() {
            if !entry.paths.contains_key(&group.id) {
                entry.paths.insert(group.id.clone(), group.default_enabled);
                changed = true;
            }
        }
    }
    changed
}

pub fn resolve_hostname(cfg: &AppConfig) -> String {
    if let Some(name) = cfg
        .app
        .hostname_override
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        return sanitize_path_component(name);
    }
    let host = whoami::fallible::hostname().unwrap_or_else(|_| "unknown-host".into());
    sanitize_path_component(&host)
}

pub fn sanitize_path_component(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => out.push('_'),
            c if c.is_control() => out.push('_'),
            c => out.push(c),
        }
    }
    if out.is_empty() {
        "unknown-host".into()
    } else {
        out
    }
}

pub fn slugify_label(label: &str, fallback: &str) -> String {
    let mut out = String::new();
    for ch in label.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if ch.is_whitespace() || ch == '_' || ch == '-' {
            if !out.ends_with('-') {
                out.push('-');
            }
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed
    }
}

pub fn open_path_in_explorer(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(path)
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(path)
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = path;
        Err("当前平台暂不支持打开目录".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_config_fills_missing_fields_with_defaults() {
        let cfg: AppConfig = serde_json::from_str(
            r#"{"version":1,"schedule":{"enabled":true},
                "sources":{"agents":{"cursor":{"paths":{"rules":false}}}},
                "some_future_field":123}"#,
        )
        .unwrap();
        assert!(cfg.schedule.enabled);
        assert_eq!(cfg.schedule.time_local, "21:00");
        assert!(cfg.encryption.enabled);
        assert!(cfg.sources.agents["cursor"].enabled);
        assert!(!cfg.exclusions.file_name_globs.is_empty());
    }

    #[test]
    fn write_atomic_replaces_existing_file() {
        let dir = std::env::temp_dir().join(format!("agent-backup-atomic-{}", uuid::Uuid::new_v4()));
        let path = dir.join("config.json");
        write_atomic(&path, b"one").unwrap();
        write_atomic(&path, b"two").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "two");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        let _ = fs::remove_dir_all(&dir);
    }
}
