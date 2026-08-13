use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
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
pub struct AppSettings {
    pub hostname_override: Option<String>,
    pub start_on_login: bool,
    pub minimize_to_tray: bool,
    pub language: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptionSettings {
    pub enabled: bool,
    pub password: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleSettings {
    pub enabled: bool,
    pub time_local: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourcesSettings {
    pub agents: HashMap<String, AgentSourceState>,
    pub custom: Vec<CustomSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSourceState {
    pub enabled: bool,
    #[serde(default)]
    pub paths: HashMap<String, bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomSource {
    pub id: String,
    pub label: String,
    pub path: String,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3Destination {
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub prefix: String,
    pub access_key: String,
    pub secret_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExclusionSettings {
    pub file_name_globs: Vec<String>,
    pub dir_name_globs: Vec<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version: 1,
            app: AppSettings {
                hostname_override: None,
                start_on_login: false,
                minimize_to_tray: true,
                language: "zh-CN".into(),
            },
            encryption: EncryptionSettings {
                enabled: true,
                password: None,
            },
            schedule: ScheduleSettings {
                enabled: false,
                time_local: "21:00".into(),
            },
            sources: SourcesSettings {
                agents: HashMap::new(),
                custom: Vec::new(),
            },
            destinations: Vec::new(),
            exclusions: ExclusionSettings {
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
            },
        }
    }
}

pub fn config_dir() -> Result<PathBuf, String> {
    let home = dirs_home().ok_or_else(|| "无法解析用户主目录".to_string())?;
    let dir = home.join(".agent-backup");
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
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let raw = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    fs::write(&path, raw).map_err(|e| format!("写入配置失败: {e}"))
}

pub fn ensure_agent_defaults(cfg: &mut AppConfig, presets: &crate::presets::PresetFile) {
    for agent in &presets.agents {
        if agent.disabled {
            continue;
        }
        let entry = cfg
            .sources
            .agents
            .entry(agent.key.clone())
            .or_insert_with(|| AgentSourceState {
                enabled: true,
                paths: HashMap::new(),
            });
        if let Some(groups) = &agent.groups {
            for group in groups {
                entry
                    .paths
                    .entry(group.id.clone())
                    .or_insert(group.default_enabled);
            }
        }
    }
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
