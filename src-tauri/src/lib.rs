#[macro_use]
mod i18n;
mod backup;
mod commands;
mod config;
mod history;
mod logging;
mod notify;
mod packager;
mod presets;
mod s3;
mod scheduler;
mod targets;
mod tray;
#[cfg(test)]
mod smoke;

use tauri::Manager;
use tauri_plugin_autostart::MacosLauncher;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    logging::init();
    log::info!("Agent Backup {} starting", env!("CARGO_PKG_VERSION"));
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .setup(|app| {
            let cfg = config::load_config();
            let lang = i18n::apply(&cfg.clone().unwrap_or_default());
            log::info!("UI language: {}", lang.code());
            if let Ok(cfg) = cfg {
                use tauri_plugin_autostart::ManagerExt;
                let launcher = app.autolaunch();
                let _ = if cfg.app.start_on_login {
                    launcher.enable()
                } else {
                    launcher.disable()
                };
            }

            scheduler::spawn_scheduler(app.handle().clone());
            tray::build(app)?;

            if let Some(window) = app.get_webview_window("main") {
                let window_clone = window.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = window_clone.hide();
                    }
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_bootstrap,
            commands::save_app_config,
            commands::add_local_destination,
            commands::add_s3_destination,
            commands::test_s3_destination,
            commands::remove_destination,
            commands::set_destination_enabled,
            commands::update_local_destination,
            commands::update_s3_destination,
            commands::add_custom_source,
            commands::set_custom_source_enabled,
            commands::remove_custom_source,
            commands::update_exclusions,
            commands::reset_exclusions,
            commands::set_agent_enabled,
            commands::set_agent_path_enabled,
            commands::run_backup_now,
            commands::retry_backup,
            commands::estimate_backup_size,
            commands::list_history,
            commands::get_latest_failure,
            commands::get_status,
            commands::open_config_dir,
            commands::open_log_dir,
            commands::open_presets_file,
            commands::reset_presets,
            commands::open_path,
            commands::get_docs_hint,
            commands::update_encryption,
            commands::update_schedule,
            commands::update_hostname_override,
            commands::set_start_on_login,
            commands::get_app_version,
            commands::set_language,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
