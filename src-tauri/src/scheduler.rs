use chrono::{Local, NaiveDate, NaiveDateTime, TimeDelta};
use serde::{Deserialize, Serialize};
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;
use tauri::AppHandle;

use crate::backup::BACKUP_BUSY;
use crate::config::{config_dir, load_config, write_atomic};
use crate::notify::{notify_and_emit, notify_error};

static SCHEDULER_STARTED: AtomicBool = AtomicBool::new(false);

const STATE_TIME_FORMAT: &str = "%Y-%m-%dT%H:%M:%S";

#[derive(Debug, Default, Serialize, Deserialize)]
struct ScheduleState {
    /// Written by older versions; read only as a fallback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_run_date: Option<String>,
    #[serde(default)]
    last_run_at: Option<String>,
}

fn schedule_state_path() -> Result<std::path::PathBuf, String> {
    Ok(config_dir()?.join("schedule_state.json"))
}

fn load_schedule_state() -> ScheduleState {
    let Ok(path) = schedule_state_path() else {
        return ScheduleState::default();
    };
    let Ok(raw) = fs::read_to_string(path) else {
        return ScheduleState::default();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

fn save_schedule_state(state: &ScheduleState) -> Result<(), String> {
    let raw = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
    write_atomic(&schedule_state_path()?, raw.as_bytes())
}

fn parse_hhmm(s: &str) -> Option<(u32, u32)> {
    let parts: Vec<_> = s.trim().split(':').collect();
    if parts.len() != 2 {
        return None;
    }
    let h = parts[0].parse().ok()?;
    let m = parts[1].parse().ok()?;
    if h < 24 && m < 60 {
        Some((h, m))
    } else {
        None
    }
}

fn last_run(state: &ScheduleState, (h, m): (u32, u32)) -> Option<NaiveDateTime> {
    if let Some(at) = state
        .last_run_at
        .as_deref()
        .and_then(|s| NaiveDateTime::parse_from_str(s, STATE_TIME_FORMAT).ok())
    {
        return Some(at);
    }
    state
        .last_run_date
        .as_deref()
        .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
        .and_then(|d| d.and_hms_opt(h, m, 0))
}

/// The most recent scheduled moment at or before `now`.
fn latest_slot(now: NaiveDateTime, (h, m): (u32, u32)) -> NaiveDateTime {
    let today = now.date().and_hms_opt(h, m, 0).expect("validated HH:mm");
    if now >= today {
        today
    } else {
        today - TimeDelta::days(1)
    }
}

fn is_due(now: NaiveDateTime, hm: (u32, u32), last: NaiveDateTime) -> bool {
    last < latest_slot(now, hm)
}

fn tick(app: &AppHandle) {
    let cfg = match load_config() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[scheduler] load config failed: {e}");
            return;
        }
    };
    if !cfg.schedule.enabled {
        return;
    }
    let Some(hm) = parse_hhmm(&cfg.schedule.time_local) else {
        eprintln!(
            "[scheduler] invalid time_local: {}",
            cfg.schedule.time_local
        );
        return;
    };
    let now = Local::now().naive_local();
    let mut state = load_schedule_state();
    let stamp = now.format(STATE_TIME_FORMAT).to_string();

    let Some(last) = last_run(&state, hm) else {
        // First time the schedule is seen: start counting from now instead of firing immediately.
        state.last_run_at = Some(stamp);
        let _ = save_schedule_state(&state);
        return;
    };
    if !is_due(now, hm, last) {
        return;
    }

    let result = crate::backup::run_backup_with_app(Some(app), &cfg, "schedule");
    if matches!(&result, Err(e) if e == BACKUP_BUSY) {
        return;
    }
    state.last_run_at = Some(stamp);
    state.last_run_date = None;
    let _ = save_schedule_state(&state);
    match result {
        Ok(r) => {
            eprintln!("[scheduler] backup {}: {}", r.overall_status, r.message);
            notify_and_emit(app, &r);
        }
        Err(e) => {
            eprintln!("[scheduler] backup failed: {e}");
            notify_error(app, &e);
        }
    }
}

pub fn spawn_scheduler(app: AppHandle) {
    if SCHEDULER_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    thread::spawn(move || loop {
        tick(&app);
        thread::sleep(Duration::from_secs(20));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, STATE_TIME_FORMAT).unwrap()
    }

    #[test]
    fn due_after_slot_passes_and_catches_up_missed_days() {
        let hm = (21, 0);
        assert!(!is_due(at("2026-10-23T20:59:00"), hm, at("2026-10-22T21:00:05")));
        assert!(is_due(at("2026-10-23T21:00:00"), hm, at("2026-10-22T21:00:05")));
        assert!(!is_due(at("2026-10-23T21:00:30"), hm, at("2026-10-23T21:00:10")));
        // Laptop asleep at 21:00, opened next morning: yesterday's slot still owed.
        assert!(is_due(at("2026-10-24T08:00:00"), hm, at("2026-10-22T21:00:05")));
        // Already caught up this morning: wait for tonight.
        assert!(!is_due(at("2026-10-24T09:00:00"), hm, at("2026-10-24T08:00:00")));
    }

    #[test]
    fn legacy_date_state_counts_as_that_days_slot() {
        let state = ScheduleState {
            last_run_date: Some("2026-10-22".into()),
            last_run_at: None,
        };
        assert_eq!(last_run(&state, (21, 0)), Some(at("2026-10-22T21:00:00")));
    }
}
