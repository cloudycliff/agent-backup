use chrono::{Local, Timelike};
use serde::{Deserialize, Serialize};
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;
use tauri::AppHandle;

use crate::config::{config_dir, load_config};
use crate::notify::{notify_and_emit, notify_error};

static SCHEDULER_STARTED: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Default, Serialize, Deserialize)]
struct ScheduleState {
    last_run_date: Option<String>,
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
    let path = schedule_state_path()?;
    let raw = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
    fs::write(path, raw).map_err(|e| e.to_string())
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
    let Some((sh, sm)) = parse_hhmm(&cfg.schedule.time_local) else {
        eprintln!(
            "[scheduler] invalid time_local: {}",
            cfg.schedule.time_local
        );
        return;
    };
    let now = Local::now();
    if now.hour() != sh || now.minute() != sm {
        return;
    }
    let today = now.format("%Y-%m-%d").to_string();
    let mut state = load_schedule_state();
    if state.last_run_date.as_deref() == Some(today.as_str()) {
        return;
    }

    match crate::backup::run_backup_with_app(Some(app), &cfg, "schedule") {
        Ok(r) => {
            eprintln!("[scheduler] backup {}: {}", r.overall_status, r.message);
            state.last_run_date = Some(today);
            let _ = save_schedule_state(&state);
            notify_and_emit(app, &r);
        }
        Err(e) => {
            if e.contains("正在进行") {
                state.last_run_date = Some(today);
                let _ = save_schedule_state(&state);
            }
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
