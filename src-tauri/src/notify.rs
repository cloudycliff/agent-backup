use tauri::{AppHandle, Emitter};
use tauri_plugin_notification::NotificationExt;

use crate::backup::{BackupRunResult, Status};

pub fn notify_and_emit(app: &AppHandle, result: &BackupRunResult) {
    let _ = app.emit("backup-finished", result);
    if result.overall_status == Status::Ok {
        return;
    }
    let title = if result.overall_status == Status::Partial {
        "备份部分失败"
    } else {
        "备份失败"
    };
    show(app, title, &result.message);
}

pub fn notify_error(app: &AppHandle, err: &str) {
    let _ = app.emit("backup-finished", BackupRunResult::failure(err));
    show(app, "备份失败", err);
}

fn show(app: &AppHandle, title: &str, body: &str) {
    if let Err(e) = app.notification().builder().title(title).body(body).show() {
        log::warn!("system notification failed: {e}");
    }
}
