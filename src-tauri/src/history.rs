use serde::{Deserialize, Serialize};
use std::fs;

use crate::config::history_path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: String,
    pub created_at: String,
    pub hostname: String,
    pub trigger: String,
    pub overall_status: String,
    pub message: String,
    #[serde(default)]
    pub sources: Vec<serde_json::Value>,
    #[serde(default)]
    pub destinations: Vec<serde_json::Value>,
}

pub fn load_history(limit: usize, only_failed: bool) -> Result<Vec<HistoryEntry>, String> {
    let path = history_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = fs::read_to_string(&path).map_err(|e| format!("读取历史失败: {e}"))?;
    let mut entries: Vec<HistoryEntry> = raw
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    if only_failed {
        entries.retain(|e| e.overall_status != "ok");
    }
    entries.reverse();
    if entries.len() > limit {
        entries.truncate(limit);
    }
    Ok(entries)
}

pub fn latest_non_ok() -> Result<Option<HistoryEntry>, String> {
    let entries = load_history(20, true)?;
    Ok(entries.into_iter().next())
}
