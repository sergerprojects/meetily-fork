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

pub mod ax;

/// Save the accessibility tree of a running call app to a text file so we can
/// see how it marks participants, the active speaker and shared screens.
/// `app` is "zoom", "teams" or "chrome" (Google Meet).
#[cfg(target_os = "macos")]
#[tauri::command]
pub async fn capture_ax_probe(app: String) -> Result<String, String> {
    if !perms::accessibility_granted() {
        return Err("Accessibility is not allowed yet (Settings, Capture).".into());
    }
    let which = ax::CallApp::parse(&app).ok_or("app must be zoom, teams or chrome")?;
    let path = tauri::async_runtime::spawn_blocking(move || -> Result<String, String> {
        let pid = ax::find_pid(which).ok_or_else(|| format!("{app} is not running"))?;
        let tree = ax::snapshot_app(pid, which == ax::CallApp::Chrome, &ax::Limits::probe())
            .ok_or("could not read the app's windows")?;
        let dir = dirs::data_dir().ok_or("no data dir")?.join("com.meetily.ai").join("ax-probes");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let file = dir.join(format!("{}-{}.txt", app, chrono::Local::now().format("%Y%m%d-%H%M%S")));
        let body = format!("{} nodes\n\n{}", tree.count(), tree.dump());
        std::fs::write(&file, body).map_err(|e| e.to_string())?;
        Ok(file.to_string_lossy().into_owned())
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(path)
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
pub async fn capture_ax_probe(_app: String) -> Result<String, String> {
    Err("only on macOS".into())
}
