use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::{user_presets_path, AgentSourceState, AppConfig};

const DEFAULT_PRESETS: &str = include_str!("../resources/agent-presets.default.json");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresetFile {
    pub version: u32,
    pub agents: Vec<AgentPreset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentPreset {
    pub key: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub root: Option<RootSpec>,
    #[serde(default)]
    pub groups: Option<Vec<PathGroup>>,
    #[serde(default)]
    pub hard_exclude: Option<Vec<String>>,
    #[serde(default)]
    pub detect: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RootSpec {
    pub kind: String,
    pub win: String,
    pub mac: String,
    #[serde(default)]
    pub env_override: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathGroup {
    pub id: String,
    pub label: String,
    pub default_enabled: bool,
    pub include: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResolvedAgent {
    pub key: String,
    pub label: String,
    pub disabled: bool,
    pub root: RootSpec,
    pub groups: Vec<PathGroup>,
    pub hard_exclude: Vec<String>,
    pub root_path: Option<PathBuf>,
    pub installed: bool,
}

pub fn load_merged_presets() -> Result<PresetFile, String> {
    let defaults: PresetFile =
        serde_json::from_str(DEFAULT_PRESETS).map_err(|e| format!("内置预设损坏: {e}"))?;
    let user_path = user_presets_path()?;
    if !user_path.exists() {
        return Ok(defaults);
    }
    let raw = fs::read_to_string(&user_path).map_err(|e| format!("读取用户预设失败: {e}"))?;
    let user: PresetFile =
        serde_json::from_str(&raw).map_err(|e| format!("解析用户预设失败: {e}"))?;
    Ok(merge_presets(defaults, user))
}

fn merge_presets(mut defaults: PresetFile, user: PresetFile) -> PresetFile {
    let mut map: HashMap<String, AgentPreset> = defaults
        .agents
        .drain(..)
        .map(|a| (a.key.clone(), a))
        .collect();

    for u in user.agents {
        match map.get_mut(&u.key) {
            Some(existing) => {
                if u.label.is_some() {
                    existing.label = u.label;
                }
                existing.disabled = u.disabled;
                if u.root.is_some() {
                    existing.root = u.root;
                }
                if u.groups.is_some() {
                    existing.groups = u.groups;
                }
                if u.hard_exclude.is_some() {
                    existing.hard_exclude = u.hard_exclude;
                }
                if u.detect.is_some() {
                    existing.detect = u.detect;
                }
            }
            None => {
                map.insert(u.key.clone(), u);
            }
        }
    }

    defaults.agents = map.into_values().collect();
    defaults.agents.sort_by(|a, b| a.key.cmp(&b.key));
    defaults.version = defaults.version.max(user.version);
    defaults
}

pub fn resolve_agents(presets: &PresetFile) -> Result<Vec<ResolvedAgent>, String> {
    let mut out = Vec::new();
    for agent in &presets.agents {
        let root = agent
            .root
            .clone()
            .ok_or_else(|| format!("Agent `{}` 缺少 root 定义", agent.key))?;
        let groups = agent.groups.clone().unwrap_or_default();
        let hard_exclude = agent.hard_exclude.clone().unwrap_or_default();
        let label = agent
            .label
            .clone()
            .unwrap_or_else(|| agent.key.clone());
        let root_path = resolve_root_path(&root)?;
        let detect = agent.detect.clone().unwrap_or_default();
        let installed = root_path
            .as_deref()
            .is_some_and(|p| detect_installed(p, &detect));
        out.push(ResolvedAgent {
            key: agent.key.clone(),
            label,
            disabled: agent.disabled,
            root,
            groups,
            hard_exclude,
            root_path,
            installed,
        });
    }
    Ok(out)
}

fn detect_installed(root: &Path, detect: &[String]) -> bool {
    if !root.exists() {
        return false;
    }
    detect.is_empty()
        || detect
            .iter()
            .any(|rel| root.join(rel.trim_end_matches(['/', '\\'])).exists())
}

fn home_dir() -> Result<PathBuf, String> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .ok_or_else(|| "无法解析用户主目录".to_string())
}

fn app_data_dir() -> Result<PathBuf, String> {
    if cfg!(target_os = "windows") {
        if let Some(dir) = env::var_os("APPDATA").filter(|v| !v.is_empty()) {
            return Ok(PathBuf::from(dir));
        }
        return Ok(home_dir()?.join("AppData").join("Roaming"));
    }
    if cfg!(target_os = "macos") {
        return Ok(home_dir()?.join("Library").join("Application Support"));
    }
    if let Some(dir) = env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    Ok(home_dir()?.join(".config"))
}

pub fn resolve_root_path(root: &RootSpec) -> Result<Option<PathBuf>, String> {
    let base = match root.kind.as_str() {
        "home_subdir" => home_dir()?,
        "app_data" => app_data_dir()?,
        other => return Err(format!("暂不支持 root.kind = {other}")),
    };
    if let Some(env_name) = root
        .env_override
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        if let Ok(val) = env::var(env_name) {
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                return Ok(Some(PathBuf::from(trimmed)));
            }
        }
    }
    let sub = if cfg!(target_os = "windows") {
        &root.win
    } else {
        &root.mac
    };
    let mut path = base;
    for part in sub.split(['/', '\\']).filter(|p| !p.is_empty()) {
        path = path.join(part);
    }
    Ok(Some(path))
}

pub fn effective_group_enabled(
    cfg: &AppConfig,
    agent_key: &str,
    group: &PathGroup,
) -> bool {
    cfg.sources
        .agents
        .get(agent_key)
        .and_then(|s| s.paths.get(&group.id).copied())
        .unwrap_or(group.default_enabled)
}

pub fn agent_enabled(cfg: &AppConfig, agent_key: &str) -> bool {
    cfg.sources
        .agents
        .get(agent_key)
        .map(|s| s.enabled)
        .unwrap_or(true)
}

pub fn default_agent_state(agent: &ResolvedAgent) -> AgentSourceState {
    let mut paths = HashMap::new();
    for g in &agent.groups {
        paths.insert(g.id.clone(), g.default_enabled);
    }
    AgentSourceState {
        enabled: true,
        paths,
    }
}

pub fn reset_user_presets() -> Result<(), String> {
    let path = user_presets_path()?;
    if path.exists() {
        fs::remove_file(&path).map_err(|e| format!("删除用户预设失败: {e}"))?;
    }
    Ok(())
}

pub fn write_user_presets_template_if_missing() -> Result<PathBuf, String> {
    let path = user_presets_path()?;
    if !path.exists() {
        // seed empty overlay so users know where to edit
        let empty = PresetFile {
            version: 1,
            agents: vec![],
        };
        let raw = serde_json::to_string_pretty(&empty).map_err(|e| e.to_string())?;
        fs::write(&path, raw).map_err(|e| e.to_string())?;
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(kind: &str, sub: &str) -> RootSpec {
        RootSpec {
            kind: kind.into(),
            win: sub.into(),
            mac: sub.into(),
            env_override: None,
        }
    }

    #[test]
    fn empty_subdir_resolves_to_home() {
        let path = resolve_root_path(&root("home_subdir", "")).unwrap().unwrap();
        assert_eq!(path, home_dir().unwrap());
    }

    #[test]
    fn app_data_root_resolves_under_platform_dir() {
        let path = resolve_root_path(&root("app_data", "Cursor/User")).unwrap().unwrap();
        assert_eq!(path, app_data_dir().unwrap().join("Cursor").join("User"));
        assert!(resolve_root_path(&root("bogus", "x")).is_err());
    }

    #[test]
    fn detect_requires_a_marker_when_given() {
        let base = std::env::temp_dir().join(format!("agent-backup-detect-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(base.join("marker-dir")).unwrap();
        assert!(detect_installed(&base, &[]));
        assert!(!detect_installed(&base, &["cli-config.json".into()]));
        assert!(detect_installed(&base, &["cli-config.json".into(), "marker-dir/".into()]));
        assert!(!detect_installed(&base.join("missing"), &[]));
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn builtin_presets_resolve() {
        let presets: PresetFile = serde_json::from_str(DEFAULT_PRESETS).unwrap();
        let agents = resolve_agents(&presets).unwrap();
        for key in ["cursor-cli", "cursor-editor", "kiro-editor", "windsurf-editor", "aider"] {
            assert!(agents.iter().any(|a| a.key == key), "missing preset {key}");
        }
    }
}
