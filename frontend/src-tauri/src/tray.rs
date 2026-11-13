use chrono::{DateTime, Duration, Local, TimeZone, Utc};
use once_cell::sync::Lazy;
use std::sync::Mutex;
use tauri::{
    Emitter,
    menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem},
    tray::TrayIconBuilder,
    AppHandle, Manager, Runtime,
};

use crate::calendar::CalendarEvent;

const CAL_EVENT_PREFIX: &str = "cal_event:";
const TITLE_MAX_CHARS: usize = 45;

/// One calendar line in the tray menu.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum CalendarMenuEntry {
    Header(String),
    Event { id: String, label: String },
    Note(String),
}

/// Calendar section last computed by the poller (rendered by build_menu).
static CALENDAR_ENTRIES: Lazy<Mutex<Vec<CalendarMenuEntry>>> =
    Lazy::new(|| Mutex::new(vec![CalendarMenuEntry::Note("Calendar not connected".to_string())]));

#[derive(Debug, Clone)]
pub enum RecordingState {
    Stopped,
    Starting,
    Recording,
    Pausing,
    Paused,
    Resuming,
    Stopping,
}

pub fn create_tray<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    // Start with default menu, will update with actual state after initialization
    // Pass can_record=true initially, will be updated by update_tray_menu immediately
    let menu = build_menu(app, RecordingState::Stopped, true)?;

    TrayIconBuilder::with_id("main-tray")
        .menu(&menu)
        .tooltip("Graphite")
        .icon(app.default_window_icon().unwrap().clone())
        .on_menu_event(|app, event| handle_menu_event(app, event.id.as_ref()))
        .build(app)?;

    // Update tray menu with actual recording state after creation
    update_tray_menu(app);

    Ok(())
}

fn handle_menu_event<R: Runtime>(app: &AppHandle<R>, item_id: &str) {
    match item_id {
        "toggle_recording" => toggle_recording_handler(app),
        "pause_recording" => pause_recording_handler(app),
        "resume_recording" => resume_recording_handler(app),
        "stop_recording" => stop_recording_handler(app),
        id if id.starts_with(CAL_EVENT_PREFIX) => {
            record_calendar_event_handler(app, id[CAL_EVENT_PREFIX.len()..].to_string())
        }
        "open_window" => focus_main_window(app),
        "settings" => {
            focus_main_window(app);
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.eval("window.location.assign('/settings')");
            }
        }
        "check_updates" => check_updates_handler(app),
        "quit" => app.exit(0),
        _ => {}
    }
}
fn toggle_recording_handler<R: Runtime>(app: &AppHandle<R>) {
    focus_main_window(app);
    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        if crate::is_recording().await {
            // Immediately show stopping state
            set_tray_state(&app_clone, RecordingState::Stopping);
            log::info!("Tray toggle: Stopping recording...");
            if let Err(e) = stop_recording_and_save(&app_clone).await {
                log::error!("Tray toggle: {}", e);
            }
        } else {
            // Immediately show starting state
            set_tray_state(&app_clone, RecordingState::Starting);

            log::info!("Emitting start recording event from tray");
            if let Some(window) = app_clone.get_webview_window("main") {
                let _ = window.eval("sessionStorage.setItem('autoStartRecording', 'true')"); // Set the flag to start recording automatically
                let _ = window.eval("window.location.assign('/')");
            }
        }
    });
}

fn pause_recording_handler<R: Runtime>(app: &AppHandle<R>) {
    // Immediately show pausing state
    set_tray_state(app, RecordingState::Pausing);

    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = crate::audio::recording_commands::pause_recording(app_clone.clone()).await {
            log::error!("Failed to pause recording from tray: {}", e);
            // Revert to current state on error
            update_tray_menu_async(&app_clone).await;
        } else {
            log::info!("Recording paused from tray");
            // The pause_recording function will call update_tray_menu, so no need to call it here
        }
    });
}

fn resume_recording_handler<R: Runtime>(app: &AppHandle<R>) {
    // Immediately show resuming state
    set_tray_state(app, RecordingState::Resuming);

    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = crate::audio::recording_commands::resume_recording(app_clone.clone()).await
        {
            log::error!("Failed to resume recording from tray: {}", e);
            // Revert to current state on error
            update_tray_menu_async(&app_clone).await;
        } else {
            log::info!("Recording resumed from tray");
            // The resume_recording function will call update_tray_menu, so no need to call it here
        }
    });
}

fn stop_recording_handler<R: Runtime>(app: &AppHandle<R>) {
    // Immediately show stopping state
    set_tray_state(app, RecordingState::Stopping);

    focus_main_window(app);
    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        log::info!("Tray: Stopping recording...");
        if let Err(e) = stop_recording_and_save(&app_clone).await {
            log::error!("Tray: {}", e);
        }
    });
}

/// Stop the recording from Rust and hand off to the frontend save flow
/// (`recording-stop-complete` -> RecordingPostProcessingProvider: SQLite save,
/// navigation, analytics). Shared by the tray and calendar auto-stop.
pub(crate) async fn stop_recording_and_save<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    // Generate save path (same as RecordingControls.tsx)
    let data_dir = match app.path().app_data_dir() {
        Ok(dir) => dir,
        Err(e) => {
            update_tray_menu_async(app).await;
            return Err(format!("Failed to get app data dir: {}", e));
        }
    };

    let timestamp = chrono::Local::now().format("%Y-%m-%dT%H-%M-%S").to_string();
    let save_path = data_dir.join(format!("recording-{}.wav", timestamp));

    // Call Rust stop_recording command (like pause/resume pattern)
    let stop_result = crate::audio::recording_commands::stop_recording(
        app.clone(),
        crate::audio::recording_commands::RecordingArgs {
            save_path: save_path.to_string_lossy().to_string(),
        },
    )
    .await;

    match stop_result {
        Ok(_) => {
            log::info!("Tray: Recording stopped successfully");
            // Trigger frontend post-processing via event (works from any page)
            if let Err(e) = app.emit("recording-stop-complete", true) {
                log::error!("Tray: Failed to emit recording-stop-complete event: {}", e);
            }
            Ok(())
        }
        Err(e) => {
            // Revert tray state on error
            update_tray_menu_async(app).await;
            Err(format!("Failed to stop recording: {}", e))
        }
    }
}

/// Tray click on a calendar event: open Graphite and start a recording named
/// after the event via the frontend's normal start path.
fn record_calendar_event_handler<R: Runtime>(app: &AppHandle<R>, event_id: String) {
    focus_main_window(app);
    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        if crate::audio::recording_commands::is_recording().await {
            log::info!("Tray: already recording, ignoring calendar event click");
            return;
        }
        let event = crate::calendar::STATE
            .upcoming
            .read()
            .await
            .iter()
            .find(|e| e.id == event_id)
            .cloned();
        let Some(event) = event else {
            log::warn!("Tray: calendar event {} no longer cached", event_id);
            return;
        };
        let payload = serde_json::json!({ "event": &event, "title": &event.summary });
        if let Err(e) = app_clone.emit(crate::calendar::autorecord::RECORD_REQUEST_EVENT, payload) {
            log::error!("Tray: failed to emit calendar record request: {}", e);
        }
    });
}

fn check_updates_handler<R: Runtime>(app: &AppHandle<R>) {
    focus_main_window(app);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.eval(
            "window.dispatchEvent(new CustomEvent('check-updates-from-tray'))"
        );
    }
}

/// Poller hook: recompute the calendar section; rebuild the tray only when it
/// changed (recording-state changes rebuild via update_tray_menu already).
pub(crate) fn set_calendar_events<R: Runtime>(app: &AppHandle<R>, enabled: bool, events: &[CalendarEvent]) {
    if let Some(tray) = app.tray_by_id("main-tray") {
        let title = if enabled { tray_title(events, Utc::now()) } else { None };
        let _ = tray.set_title(title.as_deref());
    }
    let entries = calendar_menu_entries(enabled, events, &Local::now());
    let changed = match CALENDAR_ENTRIES.lock() {
        Ok(mut cur) if *cur != entries => {
            *cur = entries;
            true
        }
        _ => false,
    };
    if changed {
        update_tray_menu(app);
    }
}

/// Menu-bar text: "Weekly sync in 12m" (within 90 min) or "Weekly sync · now".
/// Only real meetings (video link or 2+ attendees), never focus blocks.
pub(crate) fn tray_title(events: &[CalendarEvent], now: DateTime<Utc>) -> Option<String> {
    let (event, start) = events
        .iter()
        .filter(|e| e.video_url.is_some() || e.attendees.len() >= 2)
        .filter_map(|e| e.bounds().map(|(s, end)| (e, s, end)))
        .filter(|(_, _, end)| *end > now)
        .min_by_key(|(_, s, _)| *s)
        .map(|(e, s, _)| (e, s))?;
    let name: String = {
        let t = event.summary.trim();
        if t.chars().count() > 18 {
            format!("{}…", t.chars().take(17).collect::<String>().trim_end())
        } else {
            t.to_string()
        }
    };
    if start <= now {
        return Some(format!("{} · now", name));
    }
    let mins = (start - now).num_minutes();
    match mins {
        0 => Some(format!("{} in <1m", name)),
        1..=59 => Some(format!("{} in {}m", name, mins)),
        60..=90 => Some(format!("{} in {}h{:02}m", name, mins / 60, mins % 60)),
        _ => None,
    }
}

/// "9:30–10:00 AM" (same meridiem) or "11:30 AM–12:30 PM".
pub(crate) fn format_time_range<Tz: TimeZone>(start: &DateTime<Tz>, end: &DateTime<Tz>) -> String
where
    Tz::Offset: std::fmt::Display,
{
    let (sh, sp) = (start.format("%-I:%M").to_string(), start.format("%p").to_string());
    let (eh, ep) = (end.format("%-I:%M").to_string(), end.format("%p").to_string());
    if sp == ep {
        format!("{}–{} {}", sh, eh, ep)
    } else {
        format!("{} {}–{} {}", sh, sp, eh, ep)
    }
}

fn truncate_title(title: &str) -> String {
    if title.chars().count() <= TITLE_MAX_CHARS {
        title.to_string()
    } else {
        let cut: String = title.chars().take(TITLE_MAX_CHARS - 1).collect();
        format!("{}…", cut.trim_end())
    }
}

/// "▶ Title  ·  9:30–10:00 AM" (in progress), "● …" (has video link), else plain.
pub(crate) fn event_label<Tz: TimeZone>(event: &CalendarEvent, now: &DateTime<Tz>) -> Option<String>
where
    Tz::Offset: std::fmt::Display,
{
    let (start, end) = event.bounds()?;
    let tz = now.timezone();
    let now_utc = now.with_timezone(&Utc);
    let prefix = if start <= now_utc && now_utc < end {
        "▶ "
    } else if event.video_url.is_some() {
        "● "
    } else {
        ""
    };
    Some(format!(
        "{}{}  ·  {}",
        prefix,
        truncate_title(&event.summary),
        format_time_range(&start.with_timezone(&tz), &end.with_timezone(&tz))
    ))
}

/// Today / Tomorrow sections (local days of `now`). Ended events are dropped;
/// an in-progress event that began yesterday counts as today.
pub(crate) fn calendar_menu_entries<Tz: TimeZone>(
    enabled: bool,
    events: &[CalendarEvent],
    now: &DateTime<Tz>,
) -> Vec<CalendarMenuEntry>
where
    Tz::Offset: std::fmt::Display,
{
    if !enabled {
        return vec![CalendarMenuEntry::Note("Calendar not connected".to_string())];
    }
    let tz = now.timezone();
    let today = now.date_naive();
    let tomorrow = today + Duration::days(1);
    let now_utc = now.with_timezone(&Utc);

    let mut live: Vec<(&CalendarEvent, DateTime<Utc>)> = events
        .iter()
        .filter_map(|e| e.bounds().map(|(s, end)| (e, s, end)))
        .filter(|(_, _, end)| *end > now_utc)
        .map(|(e, s, _)| (e, s))
        .collect();
    live.sort_by_key(|(_, s)| *s);

    let mut out = Vec::new();
    for (header, day, empty) in [("Today", today, "No more meetings today"), ("Tomorrow", tomorrow, "No meetings")] {
        out.push(CalendarMenuEntry::Header(header.to_string()));
        let mut any = false;
        for (e, s) in &live {
            let d = s.with_timezone(&tz).date_naive();
            let in_day = if day == today { d <= today } else { d == day };
            if !in_day {
                continue;
            }
            if let Some(label) = event_label(e, now) {
                out.push(CalendarMenuEntry::Event { id: e.id.clone(), label });
                any = true;
            }
        }
        if !any {
            out.push(CalendarMenuEntry::Note(empty.to_string()));
        }
    }
    out
}

pub fn update_tray_menu<R: Runtime>(app: &AppHandle<R>) {
    // For sync update, spawn async task to get current state
    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        // Small delay to ensure recording state has been updated
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        update_tray_menu_async(&app_clone).await;
    });
}

pub fn set_tray_state<R: Runtime>(app: &AppHandle<R>, state: RecordingState) {
    log::info!("Tray: Setting intermediate state: {:?}", state);
    // During recording state transitions, we assume recording is allowed (we're already recording)
    if let Ok(menu) = build_menu(app, state, true) {
        if let Some(tray) = app.tray_by_id("main-tray") {
            let result = tray.set_menu(Some(menu));
            log::info!("Tray: Intermediate state menu update result: {:?}", result);
        } else {
            log::warn!("Tray: Could not find tray with id 'main-tray'");
        }
    } else {
        log::error!("Tray: Failed to build menu for intermediate state");
    }
}

async fn get_current_recording_state() -> RecordingState {
    // Check if currently recording
    let is_recording = crate::audio::recording_commands::is_recording().await;
    log::info!(
        "Tray: get_current_recording_state - is_recording: {}",
        is_recording
    );

    if !is_recording {
        log::info!("Tray: Recording state is Stopped");
        return RecordingState::Stopped;
    }

    // Check if paused
    let is_paused = crate::audio::recording_commands::is_recording_paused().await;
    log::info!("Tray: is_paused: {}", is_paused);

    if is_paused {
        log::info!("Tray: Recording state is Paused");
        RecordingState::Paused
    } else {
        log::info!("Tray: Recording state is Recording");
        RecordingState::Recording
    }
}

/// Check if recording is allowed based on onboarding status and transcription model availability
/// Returns true if:
/// - Onboarding is complete (user may prefer Whisper later), OR
/// - Parakeet transcription model is ready (downloaded)
async fn check_can_record<R: Runtime>(app: &AppHandle<R>) -> bool {
    // First check if onboarding is complete
    let onboarding_complete = match crate::onboarding::load_onboarding_status(app).await {
        Ok(status) => status.completed,
        Err(e) => {
            log::warn!("Tray: Failed to load onboarding status: {}, assuming complete", e);
            true // Assume complete if we can't check (safe default)
        }
    };

    // If onboarding is complete, always allow recording
    // (user may prefer Whisper or have their own transcription setup)
    if onboarding_complete {
        return true;
    }

    // During onboarding, check if Parakeet transcription model is ready
    match crate::parakeet_engine::commands::parakeet_has_available_models().await {
        Ok(has_models) => has_models,
        Err(e) => {
            log::warn!("Tray: Failed to check Parakeet models: {}, assuming not ready", e);
            false
        }
    }
}

pub async fn update_tray_menu_async<R: Runtime>(app: &AppHandle<R>) {
    log::info!("Tray: update_tray_menu_async called");
    // Get the current recording state
    let recording_state = get_current_recording_state().await;
    log::info!("Tray: Current recording state: {:?}", recording_state);

    // Determine if recording should be allowed
    // Only block recording during incomplete onboarding when no transcription model is ready
    let can_record = check_can_record(app).await;
    log::info!("Tray: can_record: {}", can_record);

    if let Ok(menu) = build_menu(app, recording_state, can_record) {
        if let Some(tray) = app.tray_by_id("main-tray") {
            let result = tray.set_menu(Some(menu));
            log::info!("Tray: Menu update result: {:?}", result);
        } else {
            log::warn!("Tray: Could not find tray with id 'main-tray'");
        }
    } else {
        log::error!("Tray: Failed to build menu");
    }
}

fn build_menu<R: Runtime>(
    app: &AppHandle<R>,
    state: RecordingState,
    can_record: bool, // True if recording is allowed (onboarding complete OR transcription model ready)
) -> tauri::Result<tauri::menu::Menu<R>> {
    let mut builder = MenuBuilder::new(app);

    // Calendar section (Granola-style): Today / Tomorrow headers + events.
    let entries = CALENDAR_ENTRIES.lock().map(|e| e.clone()).unwrap_or_default();
    for entry in &entries {
        builder = match entry {
            CalendarMenuEntry::Header(text) | CalendarMenuEntry::Note(text) => {
                builder.item(&MenuItemBuilder::new(text.as_str()).enabled(false).build(app)?)
            }
            CalendarMenuEntry::Event { id, label } => builder.item(
                &MenuItemBuilder::with_id(format!("{}{}", CAL_EVENT_PREFIX, id), label.as_str())
                    .enabled(can_record)
                    .build(app)?,
            ),
        };
    }
    if !entries.is_empty() {
        builder = builder.item(&PredefinedMenuItem::separator(app)?);
    }

    // If recording is not allowed (during onboarding, no transcription model), show disabled message
    if !can_record {
        builder = builder.item(
            &MenuItemBuilder::new("⏳ Downloading transcription model...")
                .enabled(false)
                .build(app)?,
        );
    } else {
        match state {
            RecordingState::Stopped => {
                builder = builder
                    .item(&MenuItemBuilder::with_id("toggle_recording", "Start Recording").build(app)?);
            }
            RecordingState::Starting => {
                builder = builder.item(
                    &MenuItemBuilder::new("🔄 Starting Recording...")
                        .enabled(false)
                        .build(app)?,
                );
            }
            RecordingState::Recording => {
                builder = builder
                    .item(&MenuItemBuilder::with_id("pause_recording", "⏸ Pause Recording").build(app)?)
                    .item(&MenuItemBuilder::with_id("stop_recording", "⏹ Stop Recording").build(app)?);
            }
            RecordingState::Pausing => {
                builder = builder
                    .item(
                        &MenuItemBuilder::new("⏸ Pausing...")
                            .enabled(false)
                            .build(app)?,
                    )
                    .item(&MenuItemBuilder::with_id("stop_recording", "⏹ Stop Recording").build(app)?);
            }
            RecordingState::Paused => {
                builder = builder
                    .item(
                        &MenuItemBuilder::with_id("resume_recording", "▶ Resume Recording")
                            .build(app)?,
                    )
                    .item(&MenuItemBuilder::with_id("stop_recording", "⏹ Stop Recording").build(app)?);
            }
            RecordingState::Resuming => {
                builder = builder
                    .item(
                        &MenuItemBuilder::new("▶ Resuming...")
                            .enabled(false)
                            .build(app)?,
                    )
                    .item(&MenuItemBuilder::with_id("stop_recording", "⏹ Stop Recording").build(app)?);
            }
            RecordingState::Stopping => {
                builder = builder.item(
                    &MenuItemBuilder::new("⏹ Stopping...")
                        .enabled(false)
                        .build(app)?,
                );
            }
        }
    }

    builder
        .item(&MenuItemBuilder::with_id("open_window", "Open Graphite").build(app)?)
        .item(&MenuItemBuilder::with_id("settings", "Settings").build(app)?)
        .item(&PredefinedMenuItem::separator(app)?)
        .item(&MenuItemBuilder::with_id("quit", "Quit").build(app)?)
        .build()
}

pub(crate) fn focus_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        if let Err(e) = window.unminimize() {
            log::error!("Failed to unminimize main window: {}", e);
        }

        if let Err(e) = window.show() {
            log::error!("Failed to show main window: {}", e);
        }

        if let Err(e) = window.set_focus() {
            log::error!("Failed to focus main window: {}", e);
        }

        if let Err(e) = window.eval("window.focus()") {
            log::error!("Failed to focus main webview: {}", e);
        }
    } else {
        log::warn!("Could not find main window");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;

    fn ev(id: &str, title: &str, start: &str, end: &str, video: bool) -> CalendarEvent {
        CalendarEvent {
            id: id.to_string(),
            summary: title.to_string(),
            start: start.to_string(),
            end: end.to_string(),
            attendees: vec![],
            organizer: None,
            html_link: None,
            location: None,
            description: None,
            video_url: video.then(|| "https://zoom.us/j/1".to_string()),
        }
    }

    fn est() -> FixedOffset {
        FixedOffset::west_opt(5 * 3600).unwrap()
    }

    #[test]
    fn menu_bar_title_countdown() {
        let now = DateTime::parse_from_rfc3339("2026-11-12T15:00:00Z").unwrap().with_timezone(&Utc);
        let soon = ev("a", "Pricing review", "2026-11-12T15:12:00Z", "2026-11-12T15:42:00Z", true);
        assert_eq!(tray_title(&[soon.clone()], now).as_deref(), Some("Pricing review in 12m"));
        let live = ev("b", "Standup", "2026-11-12T14:50:00Z", "2026-11-12T15:20:00Z", true);
        assert_eq!(tray_title(&[live, soon.clone()], now).as_deref(), Some("Standup · now"));
        let far = ev("c", "Later", "2026-11-12T18:00:00Z", "2026-11-12T19:00:00Z", true);
        assert_eq!(tray_title(&[far], now), None, "more than 90 min away: no title");
        let focus = ev("d", "Focus time", "2026-11-12T15:05:00Z", "2026-11-12T16:00:00Z", false);
        assert_eq!(tray_title(&[focus], now), None, "no video and no attendees: not a meeting");
        let long = ev("e", "A very long meeting title here", "2026-11-12T15:30:00Z", "2026-11-12T16:00:00Z", true);
        assert_eq!(tray_title(&[long], now).as_deref(), Some("A very long meeti… in 30m"));
        assert_eq!(tray_title(&[], now), None);
    }

    #[test]
    fn time_range_formatting() {
        let tz = est();
        let t = |h, m| tz.with_ymd_and_hms(2026, 11, 12, h, m, 0).unwrap();
        assert_eq!(format_time_range(&t(9, 30), &t(10, 0)), "9:30–10:00 AM");
        assert_eq!(format_time_range(&t(11, 30), &t(12, 30)), "11:30 AM–12:30 PM");
        assert_eq!(format_time_range(&t(13, 5), &t(14, 0)), "1:05–2:00 PM");
    }

    #[test]
    fn labels_prefixes_and_truncation() {
        let now = est().with_ymd_and_hms(2026, 11, 12, 9, 45, 0).unwrap();
        let live = ev("a", "Standup", "2026-11-12T09:30:00-05:00", "2026-11-12T10:00:00-05:00", true);
        let video = ev("b", "1:1", "2026-11-12T15:30:00Z", "2026-11-12T16:00:00Z", true);
        let plain = ev("c", "Lunch", "2026-11-12T12:00:00-05:00", "2026-11-12T13:00:00-05:00", false);
        assert_eq!(event_label(&live, &now).unwrap(), "▶ Standup  ·  9:30–10:00 AM");
        assert_eq!(event_label(&video, &now).unwrap(), "● 1:1  ·  10:30–11:00 AM");
        assert_eq!(event_label(&plain, &now).unwrap(), "Lunch  ·  12:00–1:00 PM");
        let long = ev("d", &"x".repeat(80), "2026-11-12T12:00:00-05:00", "2026-11-12T13:00:00-05:00", false);
        let label = event_label(&long, &now).unwrap();
        assert!(label.starts_with(&format!("{}…  ·", "x".repeat(44))), "{}", label);
    }

    #[test]
    fn today_tomorrow_grouping_in_local_time() {
        // 10:00 PM EST on the 12th = 03:00Z on the 13th.
        let now = est().with_ymd_and_hms(2026, 11, 12, 22, 0, 0).unwrap();
        let events = vec![
            ev("ended", "Ended", "2026-11-12T20:00:00-05:00", "2026-11-12T21:00:00-05:00", true),
            // Started yesterday-local, still running.
            ev("overnight", "Overnight", "2026-11-11T23:00:00-05:00", "2026-11-12T23:00:00-05:00", false),
            // 11:30 PM local today (04:30Z next UTC day) -> Today.
            ev("late", "Late", "2026-11-13T04:30:00Z", "2026-11-13T05:00:00Z", true),
            // 00:30 local tomorrow -> Tomorrow.
            ev("early", "Early", "2026-11-13T05:30:00Z", "2026-11-13T06:00:00Z", false),
            // Day after tomorrow -> dropped.
            ev("far", "Far", "2026-11-14T15:00:00Z", "2026-11-14T16:00:00Z", false),
        ];
        let entries = calendar_menu_entries(true, &events, &now);
        let ids: Vec<String> = entries
            .iter()
            .map(|e| match e {
                CalendarMenuEntry::Header(h) => format!("#{}", h),
                CalendarMenuEntry::Event { id, .. } => id.clone(),
                CalendarMenuEntry::Note(n) => format!("({})", n),
            })
            .collect();
        assert_eq!(ids, vec!["#Today", "overnight", "late", "#Tomorrow", "early"]);
        match &entries[1] {
            CalendarMenuEntry::Event { label, .. } => assert!(label.starts_with("▶ Overnight")),
            other => panic!("unexpected {:?}", other),
        }
    }

    #[test]
    fn empty_and_disabled_sections() {
        let now = est().with_ymd_and_hms(2026, 11, 12, 9, 0, 0).unwrap();
        assert_eq!(
            calendar_menu_entries(false, &[], &now),
            vec![CalendarMenuEntry::Note("Calendar not connected".into())]
        );
        assert_eq!(
            calendar_menu_entries(true, &[], &now),
            vec![
                CalendarMenuEntry::Header("Today".into()),
                CalendarMenuEntry::Note("No more meetings today".into()),
                CalendarMenuEntry::Header("Tomorrow".into()),
                CalendarMenuEntry::Note("No meetings".into()),
            ]
        );
    }
}
