use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{App, AppHandle, Manager, Wry};

use crate::commands;

struct TrayItems {
    show: MenuItem<Wry>,
    backup: MenuItem<Wry>,
    quit: MenuItem<Wry>,
}

fn labels() -> [String; 3] {
    [
        t!("打开主界面", "Open Agent Backup"),
        t!("立即备份", "Back up now"),
        t!("退出", "Quit"),
    ]
}

pub fn build(app: &App) -> tauri::Result<()> {
    let [show, backup, quit] = labels();
    let items = TrayItems {
        show: MenuItem::with_id(app, "show", show, true, None::<&str>)?,
        backup: MenuItem::with_id(app, "backup", backup, true, None::<&str>)?,
        quit: MenuItem::with_id(app, "quit", quit, true, None::<&str>)?,
    };
    let menu = Menu::with_items(app, &[&items.show, &items.backup, &items.quit])?;
    TrayIconBuilder::new()
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
                    let _ = commands::run_and_notify(&handle, "manual");
                });
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .build(app)?;
    app.manage(items);
    Ok(())
}

/// Re-applies menu labels after the language changes.
pub fn relabel(app: &AppHandle) {
    let Some(items) = app.try_state::<TrayItems>() else {
        return;
    };
    let [show, backup, quit] = labels();
    for (item, text) in [(&items.show, show), (&items.backup, backup), (&items.quit, quit)] {
        if let Err(e) = item.set_text(text) {
            log::warn!("update tray label failed: {e}");
        }
    }
}
