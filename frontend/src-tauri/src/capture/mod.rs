//! Capture features driven by what the call app shows: participant names and
//! shared-screen snapshots. Everything stays on this Mac and each feature has
//! its own switch in Settings.

pub mod clock;
pub mod files;
pub mod perms;

use serde::Serialize;
use tauri::{AppHandle, Wry};
use tauri_plugin_store::StoreExt;

const NAMES_KEY: &str = "capture_names";
const SNAPSHOTS_KEY: &str = "capture_snapshots";

fn store_bool(app: &AppHandle<Wry>, key: &str, default: bool) -> bool {
    app.store(crate::calendar::ical::STORE_FILE)
        .ok()
        .and_then(|st| st.get(key))
        .and_then(|v| v.as_bool())
        .unwrap_or(default)
}

fn set_store_bool(app: &AppHandle<Wry>, key: &str, value: bool) -> Result<(), String> {
    let store = app
        .store(crate::calendar::ical::STORE_FILE)
        .map_err(|e| format!("Failed to access store: {e}"))?;
    store.set(key, serde_json::json!(value));
    store.save().map_err(|e| format!("Failed to save store: {e}"))
}

/// Names from the call app: on by default (needs Accessibility to do anything).
pub fn names_enabled(app: &AppHandle<Wry>) -> bool {
    store_bool(app, NAMES_KEY, true)
}

/// Snapshots store other people's shared content: off until switched on.
pub fn snapshots_enabled(app: &AppHandle<Wry>) -> bool {
    store_bool(app, SNAPSHOTS_KEY, false)
}

#[derive(Serialize)]
pub struct CaptureStatus {
    pub accessibility: bool,
    pub screen_recording: bool,
    pub names: bool,
    pub snapshots: bool,
}

#[tauri::command]
pub async fn capture_status(app: AppHandle<Wry>) -> CaptureStatus {
    CaptureStatus {
        accessibility: perms::accessibility_granted(),
        screen_recording: perms::screen_granted(),
        names: names_enabled(&app),
        snapshots: snapshots_enabled(&app),
    }
}

/// Shows the macOS prompt (first time) and returns whether access is granted.
#[tauri::command]
pub async fn capture_request_accessibility() -> bool {
    perms::request_accessibility()
}

#[tauri::command]
pub async fn capture_request_screen() -> bool {
    perms::request_screen()
}

#[tauri::command]
pub async fn set_capture_names(app: AppHandle<Wry>, enabled: bool) -> Result<(), String> {
    set_store_bool(&app, NAMES_KEY, enabled)
}

#[tauri::command]
pub async fn set_capture_snapshots(app: AppHandle<Wry>, enabled: bool) -> Result<(), String> {
    set_store_bool(&app, SNAPSHOTS_KEY, enabled)
}
