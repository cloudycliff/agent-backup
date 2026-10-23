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
    let _ = app
        .notification()
        .builder()
        .title(title)
        .body(&result.message)
        .show();
}

pub fn notify_error(app: &AppHandle, err: &str) {
    let _ = app.emit("backup-finished", BackupRunResult::failure(err));
    let _ = app
        .notification()
        .builder()
        .title("备份失败")
        .body(err)
        .show();
}
