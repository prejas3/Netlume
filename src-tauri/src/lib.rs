mod engine;
pub mod model;

use std::{sync::Mutex, thread, time::Duration};

use model::{AppStat, Global, Limit, GLOBAL_EVENT, STATS_EVENT};
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager, State, WindowEvent,
};

#[derive(Default)]
struct AppState {
    global: Mutex<Global>,
}

#[tauri::command]
fn set_limit(_path: String, _limit: Limit) -> Result<(), String> {
    Ok(())
}

#[tauri::command]
fn set_block(_path: String, _blocked: bool) -> Result<(), String> {
    Ok(())
}

#[tauri::command]
fn set_global(global: Global, state: State<'_, AppState>, app: AppHandle) -> Result<(), String> {
    *state.global.lock().map_err(|error| error.to_string())? = global.clone();
    app.emit(GLOBAL_EVENT, global)
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
fn history(_day: String) -> Vec<AppStat> {
    mock_stats(0)
}

fn mock_stats(tick: u64) -> Vec<AppStat> {
    vec![
        AppStat {
            id: "browser".into(),
            name: "Web Browser".into(),
            path: r"C:\Program Files\Browser\browser.exe".into(),
            down_bps: 840_000 + tick % 80_000,
            up_bps: 52_000,
            down_total: 128_000_000 + tick * 840_000,
            up_total: 8_000_000 + tick * 52_000,
            limit: None,
            blocked: false,
        },
        AppStat {
            id: "chat".into(),
            name: "Chat".into(),
            path: r"C:\Program Files\Chat\chat.exe".into(),
            down_bps: 18_000,
            up_bps: 11_000,
            down_total: 5_000_000 + tick * 18_000,
            up_total: 2_000_000 + tick * 11_000,
            limit: None,
            blocked: false,
        },
    ]
}

fn emit_stats(app: AppHandle, engine: std::sync::Arc<engine::Engine>) {
    thread::spawn(move || {
        let mut apps = std::collections::BTreeMap::new();
        loop {
            thread::sleep(Duration::from_secs(1));
            if app.emit(STATS_EVENT, engine.tick(&mut apps)).is_err() {
                break;
            }
        }
    });
}

fn emit_mock_stats(app: AppHandle) {
    thread::spawn(move || {
        let mut tick = 0;
        loop {
            thread::sleep(Duration::from_secs(1));
            tick += 1;
            if app.emit(STATS_EVENT, mock_stats(tick)).is_err() {
                break;
            }
        }
    });
}

pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .setup(|app| {
            let open = MenuItem::with_id(app, "open", "Open Netlume", true, None::<&str>)?;
            let pause = MenuItem::with_id(app, "pause", "Pause all limits", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &pause, &quit])?;
            let icon = app
                .default_window_icon()
                .cloned()
                .ok_or("missing default window icon")?;

            TrayIconBuilder::new()
                .icon(icon)
                .menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "open" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "pause" => {
                        if let Some(state) = app.try_state::<AppState>() {
                            if let Ok(mut global) = state.global.lock() {
                                global.paused = !global.paused;
                                let _ = app.emit(GLOBAL_EVENT, global.clone());
                            }
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;

            // ponytail: no admin or no WinDivert (UI dev) → mock stats, so the UI still runs.
            match engine::start([None, None]) {
                Ok(engine) => emit_stats(app.handle().clone(), engine),
                Err(error) => {
                    eprintln!("engine off, mock stats: {error}");
                    emit_mock_stats(app.handle().clone());
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            set_limit, set_block, set_global, history
        ])
        .run(tauri::generate_context!())
        .expect("error while running Netlume");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_stats_match_contract() {
        let stats = mock_stats(3);
        assert_eq!(stats.len(), 2);
        assert!(stats
            .iter()
            .all(|stat| !stat.id.is_empty() && !stat.path.is_empty()));
    }
}
