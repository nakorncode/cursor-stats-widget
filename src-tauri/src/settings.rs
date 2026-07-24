use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, State};

pub const REFRESH_CHOICES_SECS: &[i64] = &[10, 20, 30, 60, 120, 300, 600, -1];
pub const RECENT_CHATS_CHOICES: &[i64] = &[1, 2, 3, 5, 10, 0]; // 0 = hide
pub const CLOCK_CHOICES: &[&str] = &["system", "12h", "24h"];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// Seconds between polls. `-1` = never (manual only).
    pub refresh_secs: i64,
    /// How many recent chats to show. `0` = hide section.
    #[serde(default = "default_recent_chats")]
    pub recent_chats: i64,
    /// `system` | `12h` | `24h` — system falls back to 12h if detection fails.
    #[serde(default = "default_clock_format")]
    pub clock_format: String,
    /// Default on: register Windows logon autostart unless the user turns it off.
    #[serde(default = "default_launch_on_startup")]
    pub launch_on_startup: bool,
}

fn default_recent_chats() -> i64 {
    3
}

fn default_clock_format() -> String {
    "system".into()
}

fn default_launch_on_startup() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            refresh_secs: 20,
            recent_chats: 3,
            clock_format: "system".into(),
            launch_on_startup: true,
        }
    }
}

pub struct SettingsState(pub Mutex<Settings>);

fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("app config dir: {e}"))?;
    fs::create_dir_all(&dir).map_err(|e| format!("create config dir: {e}"))?;
    Ok(dir.join("settings.json"))
}

pub fn load(app: &AppHandle) -> Settings {
    let Ok(path) = settings_path(app) else {
        return Settings::default();
    };
    let Ok(raw) = fs::read_to_string(path) else {
        return Settings::default();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

pub fn save(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let path = settings_path(app)?;
    let raw = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(path, raw).map_err(|e| format!("write settings: {e}"))
}

fn emit_saved(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    save(app, settings)?;
    let _ = app.emit("settings-changed", settings);
    Ok(())
}

#[tauri::command]
pub fn get_settings(state: State<'_, SettingsState>) -> Settings {
    state.0.lock().map(|g| g.clone()).unwrap_or_default()
}

#[tauri::command]
pub fn set_refresh_secs(
    app: AppHandle,
    state: State<'_, SettingsState>,
    secs: i64,
) -> Result<Settings, String> {
    if !REFRESH_CHOICES_SECS.contains(&secs) {
        return Err(format!("unsupported refresh interval: {secs}"));
    }
    let mut guard = state.0.lock().map_err(|e| e.to_string())?;
    guard.refresh_secs = secs;
    let cloned = guard.clone();
    drop(guard);
    emit_saved(&app, &cloned)?;
    Ok(cloned)
}

#[tauri::command]
pub fn set_recent_chats(
    app: AppHandle,
    state: State<'_, SettingsState>,
    count: i64,
) -> Result<Settings, String> {
    if !RECENT_CHATS_CHOICES.contains(&count) {
        return Err(format!("unsupported recent chats count: {count}"));
    }
    let mut guard = state.0.lock().map_err(|e| e.to_string())?;
    guard.recent_chats = count;
    let cloned = guard.clone();
    drop(guard);
    emit_saved(&app, &cloned)?;
    Ok(cloned)
}

#[tauri::command]
pub fn set_clock_format(
    app: AppHandle,
    state: State<'_, SettingsState>,
    format: String,
) -> Result<Settings, String> {
    if !CLOCK_CHOICES.contains(&format.as_str()) {
        return Err(format!("unsupported clock format: {format}"));
    }
    let mut guard = state.0.lock().map_err(|e| e.to_string())?;
    guard.clock_format = format;
    let cloned = guard.clone();
    drop(guard);
    emit_saved(&app, &cloned)?;
    Ok(cloned)
}
