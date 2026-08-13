use tauri::{AppHandle, Emitter};
use tauri_plugin_notification::NotificationExt;

use crate::backup::BackupRunResult;

pub fn notify_and_emit(app: &AppHandle, result: &BackupRunResult) {
    let _ = app.emit("backup-finished", result);
    if result.overall_status == "ok" {
        return;
    }
    let title = if result.overall_status == "partial" {
        "备份部分失败"
    } else {
        "备份失败"
    };
    let _ = app
        .notification()
        .builder()
        .title(title)
        .body(&result.message)
        .show();
}

pub fn notify_error(app: &AppHandle, err: &str) {
    let _ = app.emit(
        "backup-finished",
        serde_json::json!({
            "id": null,
            "created_at": "",
            "hostname": "",
            "trigger": "manual",
            "overall_status": "failed",
            "work_dir": "",
            "sources": [],
            "destinations": [],
            "message": err,
        }),
    );
    let _ = app
        .notification()
        .builder()
        .title("备份失败")
        .body(err)
        .show();
}
