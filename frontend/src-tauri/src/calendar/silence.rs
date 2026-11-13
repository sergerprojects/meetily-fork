//! Stop a recording after a stretch with no speech.
//!
//! "No speech" = no new transcript line for N minutes (the call ended, everyone
//! left, or the mic and the call audio both went quiet). Applies to every
//! recording, manual or automatic, and never while paused. N defaults to 10
//! minutes; 0 turns it off. Stopping saves the recording like a normal stop.

use chrono::Utc;
use std::sync::atomic::{AtomicI64, Ordering};
use tauri::{AppHandle, Wry};
use tauri_plugin_store::StoreExt;
use tokio_util::sync::CancellationToken;

const STORE_KEY: &str = "silence_stop_minutes";
const DEFAULT_MINUTES: i64 = 10;
const CHECK_EVERY_SECS: u64 = 20;

static LAST_SPEECH_MS: AtomicI64 = AtomicI64::new(0);
static LIMIT_MINUTES: AtomicI64 = AtomicI64::new(DEFAULT_MINUTES);
static CANCEL: once_cell::sync::Lazy<CancellationToken> = once_cell::sync::Lazy::new(CancellationToken::new);

/// Called for every transcript line the app produces.
pub fn note_speech() {
    LAST_SPEECH_MS.store(Utc::now().timestamp_millis(), Ordering::Relaxed);
}

/// Pure decision, unit tested.
pub fn should_stop(recording: bool, paused: bool, silent_secs: i64, limit_minutes: i64) -> bool {
    recording && !paused && limit_minutes > 0 && silent_secs >= limit_minutes * 60
}

/// The Mac slept between two checks (the clock jumped far past the check interval).
pub fn slept(prev_ms: i64, now_ms: i64, interval_secs: u64) -> bool {
    now_ms - prev_ms > (interval_secs as i64) * 3 * 1000
}

/// The calendar slot is over and the call has gone quiet: stop without waiting
/// out the full silence limit. Never inside the slot, never while paused.
pub fn should_stop_after_event(recording: bool, paused: bool, now_ms: i64, event_end_ms: i64, silent_secs: i64) -> bool {
    const GRACE_AFTER_END_SECS: i64 = 5 * 60;
    const QUIET_SECS: i64 = 2 * 60;
    recording && !paused && now_ms >= event_end_ms + GRACE_AFTER_END_SECS * 1000 && silent_secs >= QUIET_SECS
}

pub fn stop() {
    CANCEL.cancel();
}

fn load_limit(app: &AppHandle<Wry>) -> i64 {
    app.store(super::ical::STORE_FILE)
        .ok()
        .and_then(|st| st.get(STORE_KEY))
        .and_then(|v| v.as_i64())
        .unwrap_or(DEFAULT_MINUTES)
}

#[tauri::command]
pub async fn get_silence_stop_minutes(app: AppHandle<Wry>) -> i64 {
    let m = load_limit(&app);
    LIMIT_MINUTES.store(m, Ordering::Relaxed);
    m
}

#[tauri::command]
pub async fn set_silence_stop_minutes(app: AppHandle<Wry>, minutes: i64) -> Result<(), String> {
    let minutes = minutes.clamp(0, 240);
    let store = app.store(super::ical::STORE_FILE).map_err(|e| format!("Failed to access store: {}", e))?;
    store.set(STORE_KEY, serde_json::json!(minutes));
    store.save().map_err(|e| format!("Failed to save store: {}", e))?;
    LIMIT_MINUTES.store(minutes, Ordering::Relaxed);
    Ok(())
}

pub fn start(app: AppHandle<Wry>) {
    LIMIT_MINUTES.store(load_limit(&app), Ordering::Relaxed);
    tauri::async_runtime::spawn(async move {
        let cancel = CANCEL.clone();
        let mut was_recording = false;
        let mut stop_sent = false;
        let mut last_tick_ms = Utc::now().timestamp_millis();
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = tokio::time::sleep(std::time::Duration::from_secs(CHECK_EVERY_SECS)) => {}
            }
            let now_ms = Utc::now().timestamp_millis();
            let woke = slept(last_tick_ms, now_ms, CHECK_EVERY_SECS);
            last_tick_ms = now_ms;
            let recording = crate::audio::recording_commands::is_recording().await;
            if recording && !was_recording {
                note_speech(); // grace period starts with the recording
                stop_sent = false;
            }
            was_recording = recording;
            if !recording || stop_sent {
                continue;
            }
            let paused = crate::audio::recording_commands::is_recording_paused().await;
            let silent_secs = (now_ms - LAST_SPEECH_MS.load(Ordering::Relaxed)) / 1000;
            let limit = LIMIT_MINUTES.load(Ordering::Relaxed);
            // Why to stop, if at all. A sleep or a finished calendar slot always
            // counts; the silence limit only when the user left it on.
            let reason: Option<String> = if woke && !paused {
                Some("the Mac went to sleep".to_string())
            } else if should_stop(recording, paused, silent_secs, limit) {
                Some(format!("no speech for {} min", limit))
            } else if limit > 0 {
                let end = super::calendar_current_event().await.and_then(|e| e.bounds()).map(|(_, end)| end.timestamp_millis());
                end.filter(|end| should_stop_after_event(recording, paused, now_ms, *end, silent_secs))
                    .map(|_| "the meeting is over".to_string())
            } else {
                None
            };
            if let Some(reason) = reason {
                stop_sent = true;
                log::info!("Auto-stop: {}, stopping recording", reason);
                match crate::tray::stop_recording_and_save(&app).await {
                    Ok(()) => super::poller::notify(&app, format!("Stopped recording: {}", reason)).await,
                    Err(e) => log::error!("Auto-stop failed: {}", e),
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sleep_is_a_clock_jump_far_past_the_interval() {
        assert!(!slept(0, 21_000, 20));
        assert!(!slept(0, 59_000, 20));
        assert!(slept(0, 61_000, 20));
        assert!(slept(0, 3_600_000, 20));
    }

    #[test]
    fn stops_after_the_slot_only_when_quiet_and_past_grace() {
        let end = 1_000_000_000;
        let grace = 5 * 60 * 1000;
        assert!(!should_stop_after_event(true, false, end + grace - 1, end, 600), "still inside the grace period");
        assert!(!should_stop_after_event(true, false, end + grace, end, 119), "someone is still talking");
        assert!(should_stop_after_event(true, false, end + grace, end, 120));
        assert!(!should_stop_after_event(true, true, end + grace, end, 600), "paused");
        assert!(!should_stop_after_event(false, false, end + grace, end, 600), "not recording");
    }

    #[test]
    fn stops_only_after_the_limit_while_recording_and_not_paused() {
        assert!(!should_stop(true, false, 599, 10));
        assert!(should_stop(true, false, 600, 10));
        assert!(!should_stop(true, true, 9999, 10), "paused");
        assert!(!should_stop(false, false, 9999, 10), "not recording");
        assert!(!should_stop(true, false, 9999, 0), "off");
    }
}
