//! Background poller: keeps today's + tomorrow's events cached (tray menu,
//! auto-record) and prompts (Tauri event + native notification) when a video
//! meeting is starting. Never starts a recording itself.

use chrono::{Duration, Local, Utc};
use once_cell::sync::Lazy;
use std::collections::HashSet;
use tauri::{AppHandle, Emitter, Manager, Wry};
use tokio_util::sync::CancellationToken;

use super::{client, end_of_tomorrow, fetch_events, init_client, load_ical_url, should_prompt, CalendarEvent, STATE};
use crate::notifications::commands::NotificationManagerState;
use crate::notifications::types::{Notification, NotificationType};

const POLL_INTERVAL_SECS: u64 = 60;
const STARTUP_DELAY_SECS: u64 = 15;
pub const MEETING_STARTING_EVENT: &str = "calendar-meeting-starting";

static CANCEL: Lazy<CancellationToken> = Lazy::new(CancellationToken::new);

/// Stop the poller (idempotent).
pub fn stop() {
    CANCEL.cancel();
}

/// Spawn the poller. Idles until a calendar source is configured.
pub fn start(app: AppHandle<Wry>) {
    tauri::async_runtime::spawn(async move {
        // No early return when nothing is configured: the user can paste an
        // iCal link later and the loop picks the new source up.
        init_client(load_ical_url(&app)).await;
        let cancel = CANCEL.clone();
        let mut prompted: HashSet<String> = HashSet::new();
        let mut last_error: Option<String> = None;

        // Give the UI and notification manager a moment to come up so the
        // first prompt isn't emitted before anyone is listening.
        tokio::select! {
            _ = cancel.cancelled() => return,
            _ = tokio::time::sleep(std::time::Duration::from_secs(STARTUP_DELAY_SECS)) => {}
        }

        loop {
            if client().await.is_some() {
                poll_once(&app, &mut prompted, &mut last_error).await;
            }
            tokio::select! {
                _ = cancel.cancelled() => {
                    log::info!("Calendar: poller stopped");
                    break;
                }
                _ = tokio::time::sleep(std::time::Duration::from_secs(POLL_INTERVAL_SECS)) => {}
            }
        }
    });
}

async fn poll_once(
    app: &AppHandle<Wry>,
    prompted: &mut HashSet<String>,
    last_error: &mut Option<String>,
) {
    let now = Utc::now();
    let horizon = end_of_tomorrow(&now.with_timezone(&Local));
    let events = match fetch_events(now - Duration::minutes(15), horizon).await {
        Ok(events) => {
            if last_error.take().is_some() {
                log::info!("Calendar: fetch recovered");
                STATE.status.write().await.reason = None;
            }
            events
        }
        Err(e) => {
            let msg = e.to_string();
            // Log each distinct error once to avoid spamming every minute.
            if last_error.as_deref() != Some(msg.as_str()) {
                log::warn!("Calendar: fetch failed: {}", msg);
                STATE.status.write().await.reason = Some(msg.clone());
            }
            *last_error = Some(msg);
            return;
        }
    };

    *STATE.upcoming.write().await = events.clone();
    crate::tray::set_calendar_events(app, true, &events);

    // Forget prompted ids for events no longer in the window.
    prompted.retain(|id| events.iter().any(|e| &e.id == id));

    let due: Vec<CalendarEvent> = events
        .into_iter()
        .filter(|e| !prompted.contains(&e.id) && should_prompt(e, now))
        .collect();
    if due.is_empty() {
        return;
    }

    // Don't prompt while already recording; leave unmarked so the prompt can
    // still fire if recording stops within the prompt window.
    if crate::audio::recording_commands::is_recording().await {
        return;
    }

    for event in due {
        prompted.insert(event.id.clone());
        log::info!("Calendar: meeting starting, prompting (event {})", event.id);

        if let Err(e) = app.emit(MEETING_STARTING_EVENT, serde_json::json!({ "event": &event })) {
            log::warn!("Calendar: failed to emit {}: {}", MEETING_STARTING_EVENT, e);
        }

        notify(app, format!("{} is starting — open Meetily to record", event.summary)).await;
    }
}

/// Native notification through the app's NotificationManager
/// (applies consent / DND / show_meeting_reminders settings).
pub(crate) async fn notify(app: &AppHandle<Wry>, body: String) {
    let state = app.state::<NotificationManagerState<Wry>>();
    let guard = state.read().await;
    if let Some(manager) = guard.as_ref() {
        let notification = Notification::new("Meetily", body, NotificationType::MeetingReminder(0));
        if let Err(e) = manager.show_notification(notification).await {
            log::warn!("Calendar: failed to show notification: {}", e);
        }
    }
}
