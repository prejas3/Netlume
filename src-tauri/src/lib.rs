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
    menu::{CheckMenuItem, Menu, MenuItem},
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
    tray: OnceLock<Tray>,
    /// "pl" or "en": the window's language, mirrored in the tray menu.
    lang: Mutex<String>,
}

struct Tray {
    open: MenuItem<tauri::Wry>,
    pause: MenuItem<tauri::Wry>,
    /// Kept in step with the Settings toggle.
    start: CheckMenuItem<tauri::Wry>,
    quit: MenuItem<tauri::Wry>,
}

/// Tray labels: open, pause (or resume when paused), start with Windows, quit.
fn tray_texts(lang: &str, paused: bool) -> [&'static str; 4] {
    match (lang, paused) {
        ("pl", false) => ["Otwórz Netlume", "Wstrzymaj wszystkie limity", "Uruchamiaj z Windows", "Zamknij"],
        ("pl", true) => ["Otwórz Netlume", "Wznów limity", "Uruchamiaj z Windows", "Zamknij"],
        (_, false) => ["Open Netlume", "Pause all limits", "Start with Windows", "Quit"],
        (_, true) => ["Open Netlume", "Resume limits", "Start with Windows", "Quit"],
    }
}

fn label_tray(state: &AppState, paused: bool) {
    let (Some(tray), Ok(lang)) = (state.tray.get(), state.lang.lock()) else {
        return;
    };
    let [open, pause, start, quit] = tray_texts(&lang, paused);
    let _ = tray.open.set_text(open);
    let _ = tray.pause.set_text(pause);
    let _ = tray.start.set_text(start);
    let _ = tray.quit.set_text(quit);
}

/// Polish when the Windows display language is Polish, else English.
fn system_lang() -> &'static str {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetUserDefaultUILanguage() -> u16;
    }
    const LANG_POLISH: u16 = 0x15;
    if unsafe { GetUserDefaultUILanguage() } & 0x3ff == LANG_POLISH { "pl" } else { "en" }
}

#[tauri::command]
fn set_language(lang: String, state: State<'_, AppState>) -> Result<(), String> {
    *state.lang.lock().map_err(|error| error.to_string())? = if lang == "pl" { "pl" } else { "en" }.into();
    let paused = state.global.lock().map_err(|error| error.to_string())?.paused;
    label_tray(&state, paused);
    Ok(())
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
    drop(rules);
    label_tray(&state, global.paused);
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

#[tauri::command]
fn get_autostart() -> bool {
    autostart_enabled()
}

#[tauri::command]
fn set_autostart(on: bool, state: State<'_, AppState>) -> Result<bool, String> {
    let result = write_autostart(on);
    let enabled = autostart_enabled();
    if let Some(tray) = state.tray.get() {
        let _ = tray.start.set_checked(enabled);
    }
    result.map(|()| enabled)
}

/// Opens an https link in the default browser, or "data" for the data folder.
#[tauri::command]
fn open_external(target: String) -> Result<(), String> {
    let target = if target == "data" {
        data_root()?.to_string_lossy().into_owned()
    } else if target.starts_with("https://") && !target.contains('"') {
        target
    } else {
        return Err("unsupported target".into());
    };
    std::process::Command::new("explorer.exe")
        .arg(target)
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn data_root() -> Result<PathBuf, String> {
    std::env::var_os("APPDATA")
        .map(|root| PathBuf::from(root).join("Netlume"))
        .ok_or_else(|| "APPDATA is unavailable".into())
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

const TASK: &str = "Netlume";
const AUTOSTART_ARG: &str = "--autostart";

fn schtasks(args: &[&str]) -> std::io::Result<bool> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    Ok(std::process::Command::new("schtasks")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()?
        .status
        .success())
}

fn autostart_enabled() -> bool {
    schtasks(&["/Query", "/TN", TASK]).unwrap_or(false)
}

/// Logon task at highest privileges: elevated start with no UAC prompt. XML, not schtasks flags,
/// because flag-made tasks die after 72 h and skip on battery.
fn write_autostart(on: bool) -> Result<(), String> {
    let ok = if on {
        let exe = std::env::current_exe().map_err(|error| error.to_string())?;
        let user = format!(
            "{}\\{}",
            std::env::var("USERDOMAIN").unwrap_or_default(),
            std::env::var("USERNAME").map_err(|error| error.to_string())?
        );
        let file = std::env::temp_dir().join("netlume-task.xml");
        let xml = task_xml(&exe.to_string_lossy(), &user);
        let bytes: Vec<u8> = std::iter::once(0xFEFF)
            .chain(xml.encode_utf16())
            .flat_map(u16::to_le_bytes)
            .collect();
        std::fs::write(&file, bytes).map_err(|error| error.to_string())?;
        let path = file.to_string_lossy().into_owned();
        let ok = schtasks(&["/Create", "/TN", TASK, "/XML", &path, "/F"]);
        let _ = std::fs::remove_file(file);
        ok
    } else {
        schtasks(&["/Delete", "/TN", TASK, "/F"])
    };
    match ok {
        Ok(true) => Ok(()),
        Ok(false) => Err("schtasks failed".into()),
        Err(error) => Err(error.to_string()),
    }
}

fn task_xml(exe: &str, user: &str) -> String {
    let escape = |text: &str| {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <Triggers><LogonTrigger><UserId>{user}</UserId></LogonTrigger></Triggers>
  <Principals><Principal id="Author"><UserId>{user}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>HighestAvailable</RunLevel></Principal></Principals>
  <Settings>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
  </Settings>
  <Actions Context="Author"><Exec><Command>{exe}</Command><Arguments>{AUTOSTART_ARG}</Arguments></Exec></Actions>
</Task>"#,
        user = escape(user),
        exe = escape(exe),
    )
}

pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .setup(|app| {
            let root = data_root()?;
            let storage = Arc::new(Storage::new(root)?);
            let rules = storage.load_rules().unwrap_or_default();
            let state = app.state::<AppState>();
            *state.global.lock().map_err(|error| error.to_string())? = rules.global.clone();
            *state.rules.lock().map_err(|error| error.to_string())? = rules.clone();
            let _ = state.storage.set(storage.clone());
            if !std::env::args().any(|arg| arg == AUTOSTART_ARG) {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                }
            }
            *state.lang.lock().map_err(|error| error.to_string())? = system_lang().into();
            let [open, pause, start, quit] = tray_texts(system_lang(), rules.global.paused);
            let open = MenuItem::with_id(app, "open", open, true, None::<&str>)?;
            let pause = MenuItem::with_id(app, "pause", pause, true, None::<&str>)?;
            let start = CheckMenuItem::with_id(
                app,
                "autostart",
                start,
                true,
                autostart_enabled(),
                None::<&str>,
            )?;
            let quit = MenuItem::with_id(app, "quit", quit, true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &pause, &start, &quit])?;
            let _ = state.tray.set(Tray { open, pause, start, quit });
            let icon = app
                .default_window_icon()
                .cloned()
                .ok_or("missing default window icon")?;

            TrayIconBuilder::new()
                .icon(icon)
                .menu(&menu)
                .on_menu_event(move |app, event| match event.id.as_ref() {
                    "autostart" => {
                        let on = autostart_enabled();
                        if let Err(error) = write_autostart(!on) {
                            eprintln!("autostart: {error}");
                        }
                        if let Some(tray) = app.state::<AppState>().tray.get() {
                            let _ = tray.start.set_checked(autostart_enabled());
                        }
                    }
                    "open" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "pause" => {
                        if let Some(state) = app.try_state::<AppState>() {
                            let paused = if let Ok(mut global) = state.global.lock() {
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
                                global.paused
                            } else {
                                return;
                            };
                            label_tray(&state, paused);
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
            set_limit, set_block, set_global, get_global, history, get_autostart, set_autostart,
            open_external, set_language
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

    #[test]
    fn tray_is_localised_and_ends_with_quit() {
        assert_eq!(tray_texts("pl", false), ["Otwórz Netlume", "Wstrzymaj wszystkie limity", "Uruchamiaj z Windows", "Zamknij"]);
        assert_eq!(tray_texts("en", true)[1], "Resume limits");
        assert_eq!(tray_texts("de", false)[3], "Quit");
    }

    #[test]
    fn autostart_task_is_elevated_unbounded_and_escaped() {
        let xml = task_xml(r"C:\A&B\netlume.exe", r"PC\me");
        assert!(xml.contains(r"<Command>C:\A&amp;B\netlume.exe</Command>"));
        assert!(xml.contains("<RunLevel>HighestAvailable</RunLevel>"));
        assert!(xml.contains("<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>"));
        assert!(xml.contains(&format!("<Arguments>{AUTOSTART_ARG}</Arguments>")));
    }
}
