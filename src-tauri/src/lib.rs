mod engine;
pub mod model;
mod storage;

use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
    thread,
    time::Duration,
};

use model::{AppStat, Global, Limit, GLOBAL_EVENT, STATS_EVENT};
use storage::{Rules, Storage};
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager, State, WindowEvent,
};

#[derive(Default)]
struct AppState {
    global: Mutex<Global>,
    rules: Mutex<Rules>,
    storage: OnceLock<Arc<Storage>>,
    /// Unset when the engine can't start (unelevated UI dev): commands still persist.
    engine: OnceLock<Arc<engine::Engine>>,
}

#[tauri::command]
fn set_limit(path: String, limit: Limit, state: State<'_, AppState>) -> Result<(), String> {
    let mut rules = state.rules.lock().map_err(|error| error.to_string())?;
    rules.apps.entry(path.clone()).or_default().limit = limit.clone();
    save_rules(&state, &rules)?;
    if let Some(engine) = state.engine.get() {
        engine.set_limit(path, limit);
    }
    Ok(())
}

#[tauri::command]
fn set_block(path: String, blocked: bool, state: State<'_, AppState>) -> Result<(), String> {
    let mut rules = state.rules.lock().map_err(|error| error.to_string())?;
    rules.apps.entry(path.clone()).or_default().blocked = blocked;
    save_rules(&state, &rules)?;
    if let Some(engine) = state.engine.get() {
        engine.set_block(path, blocked);
    }
    Ok(())
}

#[tauri::command]
fn set_global(global: Global, state: State<'_, AppState>, app: AppHandle) -> Result<(), String> {
    *state.global.lock().map_err(|error| error.to_string())? = global.clone();
    let mut rules = state.rules.lock().map_err(|error| error.to_string())?;
    rules.global = global.clone();
    save_rules(&state, &rules)?;
    if let Some(engine) = state.engine.get() {
        engine.set_global(global.clone());
    }
    app.emit(GLOBAL_EVENT, global)
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
fn get_global(state: State<'_, AppState>) -> Result<Global, String> {
    state
        .global
        .lock()
        .map(|global| global.clone())
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn history(day: String, state: State<'_, AppState>) -> Result<Vec<AppStat>, String> {
    state
        .storage
        .get()
        .ok_or("storage unavailable")?
        .history(&day)
        .map_err(|error| error.to_string())
}

fn save_rules(state: &AppState, rules: &Rules) -> Result<(), String> {
    state
        .storage
        .get()
        .ok_or("storage unavailable")?
        .save_rules(rules)
        .map_err(|error| error.to_string())
}

fn today() -> String {
    #[repr(C)]
    #[derive(Default)]
    struct SystemTime {
        year: u16,
        month: u16,
        weekday: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        millis: u16,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetLocalTime(time: *mut SystemTime);
    }
    let mut time = SystemTime::default();
    unsafe { GetLocalTime(&mut time) };
    format!("{:04}-{:02}-{:02}", time.year, time.month, time.day)
}

fn emit_stats(app: AppHandle, engine: Arc<engine::Engine>, storage: Arc<Storage>) {
    thread::spawn(move || {
        let mut apps = BTreeMap::new();
        let mut ticks = 0;
        loop {
            thread::sleep(Duration::from_secs(1));
            let mut stats = engine.tick(&mut apps);
            if let Ok(rules) = app.state::<AppState>().rules.lock() {
                apply_rules(&mut stats, &rules);
            }
            let _ = storage.record(&today(), &stats);
            ticks += 1;
            if ticks % 60 == 0 {
                let _ = storage.flush();
            }
            if app.emit(STATS_EVENT, stats).is_err() {
                break;
            }
        }
    });
}

fn apply_rules(stats: &mut [AppStat], rules: &Rules) {
    for stat in stats {
        let rule = rules.apps.get(&stat.path);
        stat.limit = rule
            .map(|rule| rule.limit.clone())
            .filter(|limit| limit.down_bps.is_some() || limit.up_bps.is_some());
        stat.blocked = rule.is_some_and(|rule| rule.blocked);
    }
}

pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .setup(|app| {
            let root = std::env::var_os("APPDATA")
                .map(PathBuf::from)
                .ok_or("APPDATA is unavailable")?
                .join("Netlume");
            let storage = Arc::new(Storage::new(root)?);
            let rules = storage.load_rules().unwrap_or_default();
            let state = app.state::<AppState>();
            *state.global.lock().map_err(|error| error.to_string())? = rules.global.clone();
            *state.rules.lock().map_err(|error| error.to_string())? = rules.clone();
            let _ = state.storage.set(storage.clone());
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
                                if let Some(engine) = state.engine.get() {
                                    engine.set_global(global.clone());
                                }
                                if let Ok(mut rules) = state.rules.lock() {
                                    rules.global = global.clone();
                                    if let Some(storage) = state.storage.get() {
                                        let _ = storage.save_rules(&rules);
                                    }
                                }
                                let _ = app.emit(GLOBAL_EVENT, global.clone());
                            }
                        }
                    }
                    "quit" => {
                        if let Some(state) = app.try_state::<AppState>() {
                            if let Some(storage) = state.storage.get() {
                                let _ = storage.flush();
                            }
                        }
                        app.exit(0)
                    }
                    _ => {}
                })
                .build(app)?;

            match engine::start() {
                Ok(engine) => {
                    for (path, rule) in &rules.apps {
                        engine.set_limit(path.clone(), rule.limit.clone());
                        engine.set_block(path.clone(), rule.blocked);
                    }
                    engine.set_global(rules.global);
                    let _ = app.state::<AppState>().engine.set(engine.clone());
                    emit_stats(app.handle().clone(), engine, storage);
                }
                Err(error) => {
                    eprintln!("engine off: {error}");
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
            set_limit, set_block, set_global, get_global, history
        ])
        .run(tauri::generate_context!())
        .expect("error while running Netlume");
}

#[cfg(test)]
mod tests {
    use super::*;
    use storage::AppRule;

    #[test]
    fn persisted_app_rules_are_visible_in_stats() {
        let mut rules = Rules::default();
        rules.apps.insert(
            "browser.exe".into(),
            AppRule {
                limit: Limit { down_bps: Some(1_000_000), up_bps: None },
                blocked: true,
            },
        );
        let mut stats = vec![AppStat {
            id: "browser.exe".into(), name: "browser".into(), path: "browser.exe".into(),
            down_bps: 0, up_bps: 0, down_total: 0, up_total: 0, limit: None, blocked: false,
        }];

        apply_rules(&mut stats, &rules);

        assert_eq!(stats[0].limit.as_ref().unwrap().down_bps, Some(1_000_000));
        assert!(stats[0].blocked);
    }
}
