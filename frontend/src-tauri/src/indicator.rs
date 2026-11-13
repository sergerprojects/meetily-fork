//! A small always-on-top chip ("Recording 12:34  Open  Stop") that appears while a
//! recording runs and Graphite itself is in the background, so you can see it is
//! on and stop it without hunting for the window. Turn it off in Settings.

use serde::Serialize;
use std::time::Duration;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, Wry};
use tauri_plugin_store::StoreExt;

const LABEL: &str = "indicator";
const STORE_KEY: &str = "live_indicator";
const WIDTH: f64 = 270.0;
const HEIGHT: f64 = 44.0;

/// Show the chip only while recording, when enabled, and when Graphite's own window
/// is not the one you are looking at.
pub fn should_show(recording: bool, enabled: bool, main_focused: bool) -> bool {
    recording && enabled && !main_focused
}

pub fn enabled(app: &AppHandle<Wry>) -> bool {
    app.store(crate::calendar::ical::STORE_FILE)
        .ok()
        .and_then(|st| st.get(STORE_KEY))
        .and_then(|v| v.as_bool())
        .unwrap_or(true)
}

#[derive(Serialize)]
pub struct IndicatorState {
    pub recording: bool,
    pub paused: bool,
    pub secs: f64,
}

#[tauri::command]
pub async fn indicator_state() -> IndicatorState {
    IndicatorState {
        recording: crate::audio::recording_commands::is_recording().await,
        paused: crate::audio::recording_commands::is_recording_paused().await,
        secs: crate::capture::clock::stream_now().unwrap_or(0.0),
    }
}

#[tauri::command]
pub async fn indicator_open(app: AppHandle<Wry>) {
    crate::tray::focus_main_window(&app);
}

#[tauri::command]
pub async fn indicator_stop(app: AppHandle<Wry>) -> Result<(), String> {
    crate::tray::stop_recording_and_save(&app).await
}

#[tauri::command]
pub async fn get_live_indicator(app: AppHandle<Wry>) -> bool {
    enabled(&app)
}

#[tauri::command]
pub async fn set_live_indicator(app: AppHandle<Wry>, enabled: bool) -> Result<(), String> {
    let store = app
        .store(crate::calendar::ical::STORE_FILE)
        .map_err(|e| format!("Failed to access store: {e}"))?;
    store.set(STORE_KEY, serde_json::json!(enabled));
    store.save().map_err(|e| format!("Failed to save store: {e}"))
}

fn open_window(app: &AppHandle<Wry>) {
    if app.get_webview_window(LABEL).is_some() {
        return;
    }
    let mut builder = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("indicator.html".into()))
        .title("Graphite")
        .inner_size(WIDTH, HEIGHT)
        .decorations(false)
        .always_on_top(true)
        .resizable(false)
        .skip_taskbar(true)
        .visible_on_all_workspaces(true)
        .focused(false)
        .shadow(true);
    // Top right of the screen the main window is on (else the primary one).
    let monitor = app
        .get_webview_window("main")
        .and_then(|w| w.current_monitor().ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());
    if let Some(m) = monitor {
        let scale = m.scale_factor();
        let pos = m.position().to_logical::<f64>(scale);
        let size = m.size().to_logical::<f64>(scale);
        builder = builder.position(pos.x + size.width - WIDTH - 24.0, pos.y + 56.0);
    }
    if let Err(e) = builder.build() {
        log::warn!("indicator: could not open window: {e}");
    }
}

fn close_window(app: &AppHandle<Wry>) {
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.close();
    }
}

pub fn start(app: AppHandle<Wry>) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let recording = crate::audio::recording_commands::is_recording().await;
            let main_focused = app
                .get_webview_window("main")
                .map(|w| w.is_focused().unwrap_or(false) && w.is_visible().unwrap_or(false))
                .unwrap_or(false);
            if should_show(recording, enabled(&app), main_focused) {
                open_window(&app);
            } else {
                close_window(&app);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shows_only_while_recording_enabled_and_app_in_background() {
        assert!(should_show(true, true, false));
        assert!(!should_show(false, true, false), "not recording");
        assert!(!should_show(true, false, false), "switched off");
        assert!(!should_show(true, true, true), "Graphite is in front");
    }
}
