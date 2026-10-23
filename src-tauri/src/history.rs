use serde::{Deserialize, Serialize};
use std::fs;

use crate::backup::{DestResult, SourceRecord, Status};
use crate::config::history_path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: String,
    pub created_at: String,
    pub hostname: String,
    pub trigger: String,
    pub overall_status: Status,
    /// Missing in entries written before this field existed.
    #[serde(default)]
    pub encryption_enabled: Option<bool>,
    pub message: String,
    #[serde(default)]
    pub sources: Vec<SourceRecord>,
    #[serde(default)]
    pub destinations: Vec<DestResult>,
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
        entries.retain(|e| e.overall_status != Status::Ok);
    }
    entries.reverse();
    entries.truncate(limit);
    Ok(entries)
}

pub fn latest_non_ok() -> Result<Option<HistoryEntry>, String> {
    let entries = load_history(20, true)?;
    Ok(entries.into_iter().next())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_entries_written_by_older_versions() {
        let line = r#"{"id":"run_1","created_at":"2026-10-23T044204Z","hostname":"h","trigger":"manual","overall_status":"partial","message":"m","sources":[{"key":"cursor","type":"agent","label":"Cursor","enabled_paths":["config"],"root_path":"C:/x","archive":"cursor.zip","bytes":1,"sha256":"ab","file_count":1,"status":"skipped","error":null}],"destinations":[{"id":"d","type":"local","name":"n","status":"failed","error":"e","uri":"C:/y"}]}"#;
        let entry: HistoryEntry = serde_json::from_str(line).unwrap();
        assert_eq!(entry.overall_status, Status::Partial);
        assert_eq!(entry.encryption_enabled, None);
        assert_eq!(entry.sources[0].status, Status::Skipped);
        assert!(entry.sources[0].warnings.is_empty());
        assert_eq!(entry.destinations[0].dest_type, "local");
    }
}
