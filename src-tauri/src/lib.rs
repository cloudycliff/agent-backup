mod backup;
mod commands;
mod config;
mod history;
mod notify;
mod packager;
mod presets;
mod s3;
mod scheduler;
#[cfg(test)]
mod smoke;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::Manager;
use tauri_plugin_autostart::MacosLauncher;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .setup(|app| {
            if let Ok(cfg) = config::load_config() {
                use tauri_plugin_autostart::ManagerExt;
                let launcher = app.autolaunch();
                let _ = if cfg.app.start_on_login {
                    launcher.enable()
                } else {
                    launcher.disable()
                };
            }

            scheduler::spawn_scheduler(app.handle().clone());

            let show_i = MenuItem::with_id(app, "show", "打开主界面", true, None::<&str>)?;
            let backup_i = MenuItem::with_id(app, "backup", "立即备份", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_i, &backup_i, &quit_i])?;

            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .tooltip("Agent Backup")
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    "backup" => {
                        let handle = app.clone();
                        std::thread::spawn(move || {
                            let cfg = match config::load_config() {
                                Ok(c) => c,
                                Err(e) => {
                                    crate::notify::notify_error(&handle, &e);
                                    return;
                                }
                            };
                            match backup::run_backup(&cfg, "manual") {
                                Ok(r) => {
                                    eprintln!("backup {}: {}", r.overall_status, r.message);
                                    crate::notify::notify_and_emit(&handle, &r);
                                }
                                Err(e) => {
                                    eprintln!("backup failed: {e}");
                                    crate::notify::notify_error(&handle, &e);
                                }
                            }
                        });
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .build(app)?;

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
            commands::add_custom_source,
            commands::remove_custom_source,
            commands::set_agent_enabled,
            commands::set_agent_path_enabled,
            commands::run_backup_now,
            commands::retry_backup,
            commands::list_history,
            commands::get_latest_failure,
            commands::open_config_dir,
            commands::open_presets_file,
            commands::reset_presets,
            commands::open_path,
            commands::get_docs_hint,
            commands::update_encryption,
            commands::update_schedule,
            commands::update_hostname_override,
            commands::set_start_on_login,
            commands::get_app_version,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
