mod auth;
mod cursor_api;
mod settings;

use cursor_api::{fetch_cost_series, fetch_usage_snapshot, CostSeries, UsageSnapshot};
use settings::{
    SettingsState, CLOCK_CHOICES, RECENT_CHATS_CHOICES, REFRESH_CHOICES_SECS,
};
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

#[tauri::command]
async fn get_usage(
    app: AppHandle,
    day_start_ms: u64,
    bypass_cache: Option<bool>,
) -> UsageSnapshot {
    let recent = app
        .state::<SettingsState>()
        .0
        .lock()
        .map(|s| s.recent_chats.max(0) as usize)
        .unwrap_or(3);
    fetch_usage_snapshot(day_start_ms, recent, bypass_cache.unwrap_or(false)).await
}

#[tauri::command]
async fn get_cost_series(range_secs: u64, bypass_cache: Option<bool>) -> CostSeries {
    fetch_cost_series(range_secs, bypass_cache.unwrap_or(false)).await
}

#[tauri::command]
fn show_overlay(app: AppHandle) -> Result<(), String> {
    set_window_visible(&app, "main", true)
}

#[tauri::command]
fn hide_overlay(app: AppHandle) -> Result<(), String> {
    set_window_visible(&app, "main", false)
}

#[tauri::command]
fn toggle_chart_window(app: AppHandle) -> Result<(), String> {
    // Prefer asking the main UI to create/show the chart webview (JS WebviewWindow API).
    // Fallback: if main is gone, try native create on this thread.
    if app.get_webview_window("main").is_some() {
        let _ = app.emit("toggle-chart", ());
        return Ok(());
    }
    create_chart_window_native(&app)
}

fn create_chart_window_native(app: &AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("chart") {
        if win.is_visible().unwrap_or(false) {
            win.hide().map_err(|e| e.to_string())?;
        } else {
            win.show().map_err(|e| e.to_string())?;
            win.set_focus().map_err(|e| e.to_string())?;
            let _ = app.emit("refresh-chart", ());
        }
        return Ok(());
    }

    let win = WebviewWindowBuilder::new(app, "chart", WebviewUrl::App("chart.html".into()))
        .title("Cursor cost chart")
        .inner_size(520.0, 340.0)
        .resizable(true)
        .decorations(false)
        .transparent(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(true)
        .build()
        .map_err(|e| e.to_string())?;

    let app_handle = app.clone();
    win.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            let _ = set_window_visible(&app_handle, "chart", false);
        }
    });
    let app_emit = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(500));
        let _ = app_emit.emit("refresh-chart", ());
    });
    Ok(())
}

fn set_window_visible(app: &AppHandle, label: &str, visible: bool) -> Result<(), String> {
    if let Some(win) = app.get_webview_window(label) {
        if visible {
            win.show().map_err(|e| e.to_string())?;
            win.set_focus().map_err(|e| e.to_string())?;
        } else {
            win.hide().map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn set_overlay_visible(app: &AppHandle, visible: bool) {
    let _ = set_window_visible(app, "main", visible);
}

fn refresh_label(secs: i64) -> &'static str {
    match secs {
        -1 => "Never (manual)",
        10 => "10 seconds",
        20 => "20 seconds",
        30 => "30 seconds",
        60 => "1 minute",
        120 => "2 minutes",
        300 => "5 minutes",
        600 => "10 minutes",
        _ => "Custom",
    }
}

fn recent_label(n: i64) -> &'static str {
    match n {
        0 => "Hide",
        1 => "1 chat",
        2 => "2 chats",
        3 => "3 chats",
        5 => "5 chats",
        10 => "10 chats",
        _ => "Custom",
    }
}

fn clock_label(fmt: &str) -> &'static str {
    match fmt {
        "12h" => "12-hour",
        "24h" => "24-hour",
        _ => "System (fallback 12h)",
    }
}

fn patch_settings(app: &AppHandle, f: impl FnOnce(&mut settings::Settings)) {
    if let Ok(mut guard) = app.state::<SettingsState>().0.lock() {
        f(&mut guard);
        let cloned = guard.clone();
        drop(guard);
        let _ = settings::save(app, &cloned);
        let _ = app.emit("settings-changed", &cloned);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![]),
        ))
        .invoke_handler(tauri::generate_handler![
            get_usage,
            get_cost_series,
            show_overlay,
            hide_overlay,
            toggle_chart_window,
            settings::get_settings,
            settings::set_refresh_secs,
            settings::set_recent_chats,
            settings::set_clock_format,
        ])
        .setup(|app| {
            let loaded = settings::load(app.handle());
            app.manage(SettingsState(std::sync::Mutex::new(loaded.clone())));

            let show_i = MenuItem::with_id(app, "show", "Show overlay", true, None::<&str>)?;
            let hide_i = MenuItem::with_id(app, "hide", "Hide overlay", true, None::<&str>)?;
            let chart_i = MenuItem::with_id(app, "chart", "Cost chart…", true, None::<&str>)?;
            let refresh_i = MenuItem::with_id(app, "refresh", "Refresh now", true, None::<&str>)?;

            let mut refresh_checks = Vec::new();
            for &secs in REFRESH_CHOICES_SECS {
                refresh_checks.push(CheckMenuItem::with_id(
                    app,
                    format!("refresh_every_{secs}"),
                    refresh_label(secs),
                    true,
                    loaded.refresh_secs == secs,
                    None::<&str>,
                )?);
            }
            let refresh_refs: Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> = refresh_checks
                .iter()
                .map(|i| i as &dyn tauri::menu::IsMenuItem<tauri::Wry>)
                .collect();
            let refresh_sub = Submenu::with_items(app, "Refresh every", true, &refresh_refs)?;

            let mut recent_checks = Vec::new();
            for &n in RECENT_CHATS_CHOICES {
                recent_checks.push(CheckMenuItem::with_id(
                    app,
                    format!("recent_chats_{n}"),
                    recent_label(n),
                    true,
                    loaded.recent_chats == n,
                    None::<&str>,
                )?);
            }
            let recent_refs: Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> = recent_checks
                .iter()
                .map(|i| i as &dyn tauri::menu::IsMenuItem<tauri::Wry>)
                .collect();
            let recent_sub = Submenu::with_items(app, "Recent chats", true, &recent_refs)?;

            let mut clock_checks = Vec::new();
            for &fmt in CLOCK_CHOICES {
                clock_checks.push(CheckMenuItem::with_id(
                    app,
                    format!("clock_{fmt}"),
                    clock_label(fmt),
                    true,
                    loaded.clock_format == fmt,
                    None::<&str>,
                )?);
            }
            let clock_refs: Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> = clock_checks
                .iter()
                .map(|i| i as &dyn tauri::menu::IsMenuItem<tauri::Wry>)
                .collect();
            let clock_sub = Submenu::with_items(app, "Clock format", true, &clock_refs)?;

            let sep = PredefinedMenuItem::separator(app)?;
            let autostart_enabled = app.autolaunch().is_enabled().unwrap_or(false);
            let autostart_i = CheckMenuItem::with_id(
                app,
                "autostart",
                "Launch on startup",
                true,
                autostart_enabled,
                None::<&str>,
            )?;
            let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(
                app,
                &[
                    &show_i,
                    &hide_i,
                    &chart_i,
                    &refresh_i,
                    &refresh_sub,
                    &recent_sub,
                    &clock_sub,
                    &sep,
                    &autostart_i,
                    &quit_i,
                ],
            )?;

            let refresh_arc = std::sync::Arc::new(
                REFRESH_CHOICES_SECS
                    .iter()
                    .copied()
                    .zip(refresh_checks.into_iter())
                    .collect::<Vec<_>>(),
            );
            let recent_arc = std::sync::Arc::new(
                RECENT_CHATS_CHOICES
                    .iter()
                    .copied()
                    .zip(recent_checks.into_iter())
                    .collect::<Vec<_>>(),
            );
            let clock_arc = std::sync::Arc::new(
                CLOCK_CHOICES
                    .iter()
                    .copied()
                    .zip(clock_checks.into_iter())
                    .collect::<Vec<_>>(),
            );
            let autostart_item = std::sync::Arc::new(autostart_i);

            let refresh_m = refresh_arc.clone();
            let recent_m = recent_arc.clone();
            let clock_m = clock_arc.clone();
            let autostart_m = autostart_item.clone();

            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("Cursor Stats")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(move |app, event| {
                    let id = event.id.as_ref();
                    match id {
                        "show" => set_overlay_visible(app, true),
                        "hide" => set_overlay_visible(app, false),
                        "chart" => {
                            let _ = toggle_chart_window(app.clone());
                        }
                        "refresh" => {
                            let _ = app.emit("refresh-usage", true);
                        }
                        "autostart" => {
                            let mgr = app.autolaunch();
                            let currently = mgr.is_enabled().unwrap_or(false);
                            let ok = if currently {
                                mgr.disable()
                            } else {
                                mgr.enable()
                            };
                            let enabled = if ok.is_ok() {
                                !currently
                            } else {
                                mgr.is_enabled().unwrap_or(currently)
                            };
                            let _ = autostart_m.set_checked(enabled);
                            if let Err(e) = ok {
                                eprintln!("launch on startup failed: {e}");
                            }
                        }
                        "quit" => app.exit(0),
                        other if other.starts_with("refresh_every_") => {
                            if let Ok(secs) = other
                                .trim_start_matches("refresh_every_")
                                .parse::<i64>()
                            {
                                if REFRESH_CHOICES_SECS.contains(&secs) {
                                    patch_settings(app, |s| s.refresh_secs = secs);
                                    for (choice, item) in refresh_m.iter() {
                                        let _ = item.set_checked(*choice == secs);
                                    }
                                }
                            }
                        }
                        other if other.starts_with("recent_chats_") => {
                            if let Ok(n) = other
                                .trim_start_matches("recent_chats_")
                                .parse::<i64>()
                            {
                                if RECENT_CHATS_CHOICES.contains(&n) {
                                    patch_settings(app, |s| s.recent_chats = n);
                                    for (choice, item) in recent_m.iter() {
                                        let _ = item.set_checked(*choice == n);
                                    }
                                    let _ = app.emit("refresh-usage", false);
                                }
                            }
                        }
                        other if other.starts_with("clock_") => {
                            let fmt = other.trim_start_matches("clock_").to_string();
                            if CLOCK_CHOICES.contains(&fmt.as_str()) {
                                patch_settings(app, |s| s.clock_format = fmt.clone());
                                for (choice, item) in clock_m.iter() {
                                    let _ = item.set_checked(*choice == fmt);
                                }
                            }
                        }
                        _ => {}
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(win) = app.get_webview_window("main") {
                            let visible = win.is_visible().unwrap_or(false);
                            set_overlay_visible(app, !visible);
                        }
                    }
                })
                .build(app)?;

            let _ = (refresh_arc, recent_arc, clock_arc, autostart_item);

            if let Some(win) = app.get_webview_window("main") {
                let app_handle = app.handle().clone();
                win.on_window_event(move |event| {
                    if let WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        set_overlay_visible(&app_handle, false);
                    }
                });
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
