//! Auto-record: when the mic says "joined a call", match it to a calendar
//! video meeting and (per `calendar_record_mode`) start recording, prompt, or
//! do nothing. Recordings started here are stopped when the mic says "left"
//! or at event end + 10 min. Manually started recordings are never touched.
//!
//! Start goes through the frontend's normal start path (event
//! `calendar-record-request`); stop goes through the tray's stop path so the
//! frontend save flow (`recording-stop-complete`) runs exactly as for a
//! manual stop.

use chrono::{DateTime, Duration, Local, Utc};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use tauri::{AppHandle, Emitter, Runtime, Wry};
use tauri_plugin_store::StoreExt;
use tokio_util::sync::CancellationToken;

use super::mic_activity::{self, MicDebounce, MicSample, MicTransition};
use super::poller::{notify, MEETING_STARTING_EVENT};
use super::{CalendarEvent, STATE};

/// Frontend: start a recording (payload `{ event: CalendarEvent | null, title }`).
pub const RECORD_REQUEST_EVENT: &str = "calendar-record-request";
/// Frontend: ask "Record this call?" (payload `{ title }`).
pub const ADHOC_CALL_EVENT: &str = "calendar-adhoc-call";

const SAMPLE_INTERVAL_SECS: u64 = 2;
/// Event matches a join when now is in [start - 5min, end + 5min].
const MATCH_SLACK: Duration = Duration::minutes(5);
/// Unmatched call: prompt after the mic has been active this long.
const ADHOC_PROMPT_AFTER: Duration = Duration::seconds(20);
/// Auto-started recordings stop at event end + this.
const AUTO_STOP_AFTER_END: Duration = Duration::minutes(10);
/// Forget an auto-start request if recording hasn't begun within this.
const START_CONFIRM_TIMEOUT: Duration = Duration::seconds(90);

const STORE_FILE: &str = "recording_preferences.json";
const STORE_KEY: &str = "calendar_record_mode";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum RecordMode {
    Off,
    #[default]
    Prompt,
    Auto,
}

impl RecordMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "off" => Some(Self::Off),
            "prompt" => Some(Self::Prompt),
            "auto" => Some(Self::Auto),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Prompt => "prompt",
            Self::Auto => "auto",
        }
    }
}

static MODE: Lazy<std::sync::RwLock<RecordMode>> = Lazy::new(|| std::sync::RwLock::new(RecordMode::default()));
static CANCEL: Lazy<CancellationToken> = Lazy::new(CancellationToken::new);

fn current_mode() -> RecordMode {
    MODE.read().map(|m| *m).unwrap_or_default()
}

fn load_mode<R: Runtime>(app: &AppHandle<R>) -> RecordMode {
    app.store(STORE_FILE)
        .ok()
        .and_then(|store| store.get(STORE_KEY))
        .and_then(|v| v.as_str().and_then(RecordMode::parse))
        .unwrap_or_default()
}

#[tauri::command]
pub async fn get_calendar_record_mode<R: Runtime>(app: AppHandle<R>) -> String {
    let mode = load_mode(&app);
    if let Ok(mut m) = MODE.write() {
        *m = mode;
    }
    mode.as_str().to_string()
}

#[tauri::command]
pub async fn set_calendar_record_mode<R: Runtime>(app: AppHandle<R>, mode: String) -> Result<(), String> {
    let parsed = RecordMode::parse(&mode).ok_or_else(|| format!("invalid mode: {}", mode))?;
    let store = app.store(STORE_FILE).map_err(|e| format!("Failed to access store: {}", e))?;
    store.set(STORE_KEY, serde_json::json!(parsed.as_str()));
    store.save().map_err(|e| format!("Failed to save store: {}", e))?;
    if let Ok(mut m) = MODE.write() {
        *m = parsed;
    }
    log::info!("Calendar: record mode set to {}", parsed.as_str());
    Ok(())
}

// ---------------------------------------------------------------------------
// Pure decision logic (unit tested)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Start recording this event now (mode auto).
    AutoStart(CalendarEvent),
    /// Show the regular "meeting starting" prompt (mode prompt).
    PromptEvent(CalendarEvent),
    /// Unmatched call: ask "Record this call?".
    PromptAdhoc,
    /// Stop a recording this logic started.
    AutoStop { title: String },
}

#[derive(Debug, Clone)]
struct AutoSession {
    title: String,
    event_end: DateTime<Utc>,
    requested_at: DateTime<Utc>,
    confirmed: bool,
}

pub struct TickInput<'a> {
    pub now: DateTime<Utc>,
    pub mode: RecordMode,
    /// Debounced "in a call" level.
    pub joined: bool,
    pub transition: Option<MicTransition>,
    /// Continuous raw mic-active time (None when idle).
    pub mic_active_for: Option<Duration>,
    pub is_recording: bool,
    pub events: &'a [CalendarEvent],
    /// Event ids recorded/dismissed elsewhere this session.
    pub excluded: &'a HashSet<String>,
}

#[derive(Debug, Default)]
pub struct AutoRecorder {
    handled: HashSet<String>,
    adhoc_done: bool,
    session: Option<AutoSession>,
}

/// Video event whose [start-5min, end+5min] contains now; closest start wins.
pub fn matching_event(events: &[CalendarEvent], now: DateTime<Utc>) -> Option<&CalendarEvent> {
    events
        .iter()
        .filter(|e| e.video_url.is_some())
        .filter_map(|e| e.bounds().map(|b| (e, b)))
        .filter(|(_, (s, end))| *s - MATCH_SLACK <= now && now <= *end + MATCH_SLACK)
        .min_by_key(|(_, (s, _))| (*s - now).num_seconds().abs())
        .map(|(e, _)| e)
}

/// Default name for an unmatched call.
pub fn adhoc_title<Tz: chrono::TimeZone>(now: &DateTime<Tz>) -> String
where
    Tz::Offset: std::fmt::Display,
{
    format!("Call {}", now.format("%H:%M"))
}

impl AutoRecorder {
    pub fn is_auto_session(&self) -> bool {
        self.session.is_some()
    }

    pub fn tick(&mut self, i: &TickInput) -> Vec<Action> {
        let mut out = Vec::new();

        // 1. Track / stop our own auto-started recording.
        if let Some(mut s) = self.session.take() {
            let keep = if !s.confirmed {
                if i.is_recording {
                    s.confirmed = true;
                    true
                } else {
                    i.now - s.requested_at <= START_CONFIRM_TIMEOUT
                }
            } else {
                // Stopped by someone else (manual stop) -> forget it.
                i.is_recording
            };
            if keep && s.confirmed {
                if i.transition == Some(MicTransition::Left) || i.now >= s.event_end + AUTO_STOP_AFTER_END {
                    out.push(Action::AutoStop { title: s.title.clone() });
                    return out;
                }
            }
            if keep {
                self.session = Some(s);
            }
        }

        // 2. Once-per-continuous-mic-session ad-hoc prompt.
        if i.mic_active_for.is_none() {
            self.adhoc_done = false;
        }

        if i.is_recording || self.session.is_some() || i.mode == RecordMode::Off || !i.joined {
            return out;
        }

        // 3. Joined: calendar match?
        if let Some(ev) = matching_event(i.events, i.now) {
            // A call that belongs to a meeting never gets the ad-hoc prompt.
            self.adhoc_done = true;
            if self.handled.contains(&ev.id) || i.excluded.contains(&ev.id) {
                return out;
            }
            self.handled.insert(ev.id.clone());
            match i.mode {
                RecordMode::Auto => {
                    let event_end = ev.bounds().map(|(_, e)| e).unwrap_or(i.now);
                    self.session = Some(AutoSession {
                        title: ev.summary.clone(),
                        event_end,
                        requested_at: i.now,
                        confirmed: false,
                    });
                    out.push(Action::AutoStart(ev.clone()));
                }
                RecordMode::Prompt => out.push(Action::PromptEvent(ev.clone())),
                RecordMode::Off => {}
            }
            return out;
        }

        // 4. No match (huddle / ad-hoc): never auto-start; prompt once after 20s.
        if !self.adhoc_done && i.mic_active_for.map_or(false, |d| d >= ADHOC_PROMPT_AFTER) {
            self.adhoc_done = true;
            out.push(Action::PromptAdhoc);
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Runtime loop
// ---------------------------------------------------------------------------

pub fn stop() {
    CANCEL.cancel();
}

/// Spawn the mic watcher. No-op where mic activity can't be sampled.
pub fn start(app: AppHandle<Wry>) {
    if mic_activity::sample().is_none() {
        log::info!("Calendar: mic activity unavailable on this platform, auto-record off");
        return;
    }
    if let Ok(mut m) = MODE.write() {
        *m = load_mode(&app);
    }
    tauri::async_runtime::spawn(async move {
        let cancel = CANCEL.clone();
        let mut debounce = MicDebounce::default();
        let mut recorder = AutoRecorder::default();
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = tokio::time::sleep(std::time::Duration::from_secs(SAMPLE_INTERVAL_SECS)) => {}
            }
            let now = Utc::now();
            let is_recording = crate::audio::recording_commands::is_recording().await;

            // Device-level fallback can't distinguish Meetily's own capture:
            // hold state while we record.
            let transition = match mic_activity::sample() {
                Some(MicSample::OtherProcesses(active)) => debounce.update(active, now),
                Some(MicSample::DeviceOnly(active)) if !is_recording => debounce.update(active, now),
                _ => None,
            };
            if let Some(t) = transition {
                log::info!("Calendar: mic {:?}", t);
            }

            let events = STATE.upcoming.read().await.clone();
            let excluded = STATE.excluded.lock().await.clone();
            let actions = recorder.tick(&TickInput {
                now,
                mode: current_mode(),
                joined: debounce.is_joined(),
                transition,
                mic_active_for: debounce.active_for(now),
                is_recording,
                events: &events,
                excluded: &excluded,
            });
            for action in actions {
                perform(&app, action).await;
            }
        }
    });
}

async fn perform(app: &AppHandle<Wry>, action: Action) {
    match action {
        Action::AutoStart(event) => {
            log::info!("Calendar: auto-starting recording for event {}", event.id);
            let payload = serde_json::json!({ "event": &event, "title": &event.summary });
            if let Err(e) = app.emit(RECORD_REQUEST_EVENT, payload) {
                log::warn!("Calendar: failed to emit {}: {}", RECORD_REQUEST_EVENT, e);
            }
            notify(app, format!("Recording {} (click tray to stop)", event.summary)).await;
        }
        Action::PromptEvent(event) => {
            log::info!("Calendar: joined meeting, prompting (event {})", event.id);
            let _ = app.emit(MEETING_STARTING_EVENT, serde_json::json!({ "event": &event }));
            notify(app, format!("{} — open Meetily to record", event.summary)).await;
        }
        Action::PromptAdhoc => {
            let title = adhoc_title(&Local::now());
            log::info!("Calendar: unmatched call detected, prompting");
            let _ = app.emit(ADHOC_CALL_EVENT, serde_json::json!({ "title": &title }));
            notify(app, "Record this call? Open Meetily to start".to_string()).await;
        }
        Action::AutoStop { title } => {
            log::info!("Calendar: auto-stopping recording");
            match crate::tray::stop_recording_and_save(app).await {
                Ok(()) => notify(app, format!("Stopped recording {}", title)).await,
                Err(e) => log::error!("Calendar: auto-stop failed: {}", e),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    fn ev(id: &str, start: &str, end: &str, video: bool) -> CalendarEvent {
        CalendarEvent {
            id: id.to_string(),
            summary: format!("Mtg {}", id),
            start: start.to_string(),
            end: end.to_string(),
            attendees: vec![],
            organizer: None,
            html_link: None,
            location: None,
            description: None,
            video_url: video.then(|| "https://meet.google.com/x".to_string()),
        }
    }

    struct Sim {
        r: AutoRecorder,
        events: Vec<CalendarEvent>,
        excluded: HashSet<String>,
    }

    impl Sim {
        fn new(events: Vec<CalendarEvent>) -> Self {
            Sim { r: AutoRecorder::default(), events, excluded: HashSet::new() }
        }
        fn tick(
            &mut self,
            now: &str,
            mode: RecordMode,
            joined: bool,
            transition: Option<MicTransition>,
            active_secs: Option<i64>,
            is_recording: bool,
        ) -> Vec<Action> {
            self.r.tick(&TickInput {
                now: at(now),
                mode,
                joined,
                transition,
                mic_active_for: active_secs.map(Duration::seconds),
                is_recording,
                events: &self.events,
                excluded: &self.excluded,
            })
        }
    }

    const S: &str = "2026-11-12T15:00:00Z";
    const E: &str = "2026-11-12T15:30:00Z";

    #[test]
    fn match_window_and_video_only() {
        let events = vec![ev("v", S, E, true), ev("nv", S, E, false)];
        assert_eq!(matching_event(&events, at("2026-11-12T14:54:59Z")), None);
        assert_eq!(matching_event(&events, at("2026-11-12T14:55:00Z")).unwrap().id, "v");
        assert_eq!(matching_event(&events, at("2026-11-12T15:35:00Z")).unwrap().id, "v");
        assert_eq!(matching_event(&events, at("2026-11-12T15:35:01Z")), None);
        // Back-to-back: closest start wins.
        let b2b = vec![ev("a", "2026-11-12T14:30:00Z", S, true), ev("b", S, E, true)];
        assert_eq!(matching_event(&b2b, at("2026-11-12T15:02:00Z")).unwrap().id, "b");
    }

    #[test]
    fn auto_mode_starts_once_and_stops_on_left() {
        let mut sim = Sim::new(vec![ev("v", S, E, true)]);
        let a = sim.tick("2026-11-12T15:01:00Z", RecordMode::Auto, true, Some(MicTransition::Joined), Some(8), false);
        assert!(matches!(a.as_slice(), [Action::AutoStart(e)] if e.id == "v"));
        // Recording comes up; no further actions.
        assert!(sim.tick("2026-11-12T15:01:02Z", RecordMode::Auto, true, None, Some(10), true).is_empty());
        assert!(sim.r.is_auto_session());
        // Left -> stop.
        let a = sim.tick("2026-11-12T15:20:00Z", RecordMode::Auto, false, Some(MicTransition::Left), None, true);
        assert_eq!(a, vec![Action::AutoStop { title: "Mtg v".into() }]);
        assert!(!sim.r.is_auto_session());
        // Rejoin same meeting after stop: deduped, no restart, no ad-hoc prompt.
        assert!(sim.tick("2026-11-12T15:21:00Z", RecordMode::Auto, true, Some(MicTransition::Joined), Some(8), false).is_empty());
        assert!(sim.tick("2026-11-12T15:21:30Z", RecordMode::Auto, true, None, Some(38), false).is_empty());
    }

    #[test]
    fn auto_stop_at_event_end_plus_10() {
        let mut sim = Sim::new(vec![ev("v", S, E, true)]);
        sim.tick("2026-11-12T15:00:30Z", RecordMode::Auto, true, Some(MicTransition::Joined), Some(8), false);
        sim.tick("2026-11-12T15:00:32Z", RecordMode::Auto, true, None, Some(10), true);
        assert!(sim.tick("2026-11-12T15:39:58Z", RecordMode::Auto, true, None, Some(2000), true).is_empty());
        let a = sim.tick("2026-11-12T15:40:00Z", RecordMode::Auto, true, None, Some(2400), true);
        assert_eq!(a, vec![Action::AutoStop { title: "Mtg v".into() }]);
        // Still on the call afterwards: no ad-hoc prompt, no restart.
        assert!(sim.tick("2026-11-12T15:40:02Z", RecordMode::Auto, true, None, Some(2402), false).is_empty());
    }

    #[test]
    fn manual_recording_never_auto_stopped() {
        let mut sim = Sim::new(vec![ev("v", S, E, true)]);
        // User already recording when they join.
        assert!(sim.tick("2026-11-12T15:01:00Z", RecordMode::Auto, true, Some(MicTransition::Joined), Some(8), true).is_empty());
        assert!(sim.tick("2026-11-12T15:20:00Z", RecordMode::Auto, false, Some(MicTransition::Left), None, true).is_empty());
        assert!(sim.tick("2026-11-12T16:00:00Z", RecordMode::Auto, false, None, None, true).is_empty());
    }

    #[test]
    fn manual_stop_of_auto_session_forgets_it() {
        let mut sim = Sim::new(vec![ev("v", S, E, true)]);
        sim.tick("2026-11-12T15:01:00Z", RecordMode::Auto, true, Some(MicTransition::Joined), Some(8), false);
        sim.tick("2026-11-12T15:01:02Z", RecordMode::Auto, true, None, Some(10), true);
        // User stops manually.
        assert!(sim.tick("2026-11-12T15:05:00Z", RecordMode::Auto, true, None, Some(240), false).is_empty());
        assert!(!sim.r.is_auto_session());
        // Later user starts manually again; Left must not stop it.
        assert!(sim.tick("2026-11-12T15:06:00Z", RecordMode::Auto, true, None, Some(300), true).is_empty());
        assert!(sim.tick("2026-11-12T15:25:00Z", RecordMode::Auto, false, Some(MicTransition::Left), None, true).is_empty());
    }

    #[test]
    fn unconfirmed_start_times_out() {
        let mut sim = Sim::new(vec![ev("v", S, E, true)]);
        sim.tick("2026-11-12T15:01:00Z", RecordMode::Auto, true, Some(MicTransition::Joined), Some(8), false);
        assert!(sim.r.is_auto_session());
        sim.tick("2026-11-12T15:03:00Z", RecordMode::Auto, true, None, Some(128), false);
        assert!(!sim.r.is_auto_session());
    }

    #[test]
    fn prompt_mode_prompts_once_per_event() {
        let mut sim = Sim::new(vec![ev("v", S, E, true)]);
        let a = sim.tick("2026-11-12T14:58:00Z", RecordMode::Prompt, true, Some(MicTransition::Joined), Some(8), false);
        assert!(matches!(a.as_slice(), [Action::PromptEvent(e)] if e.id == "v"));
        assert!(sim.tick("2026-11-12T14:58:30Z", RecordMode::Prompt, true, None, Some(38), false).is_empty());
        assert!(!sim.r.is_auto_session());
    }

    #[test]
    fn excluded_events_and_off_mode_do_nothing() {
        let mut sim = Sim::new(vec![ev("v", S, E, true)]);
        assert!(sim.tick("2026-11-12T15:01:00Z", RecordMode::Off, true, Some(MicTransition::Joined), Some(60), false).is_empty());
        sim.excluded.insert("v".into());
        assert!(sim.tick("2026-11-12T15:01:02Z", RecordMode::Auto, true, None, Some(62), false).is_empty());
        assert!(sim.tick("2026-11-12T15:01:30Z", RecordMode::Auto, true, None, Some(90), false).is_empty());
    }

    #[test]
    fn adhoc_call_prompts_after_20s_never_autostarts() {
        // Only a non-video event, so no match.
        let mut sim = Sim::new(vec![ev("nv", S, E, false)]);
        for mode in [RecordMode::Auto, RecordMode::Prompt] {
            sim.r = AutoRecorder::default();
            assert!(sim.tick("2026-11-12T15:00:08Z", mode, true, Some(MicTransition::Joined), Some(8), false).is_empty());
            assert!(sim.tick("2026-11-12T15:00:18Z", mode, true, None, Some(18), false).is_empty());
            assert_eq!(sim.tick("2026-11-12T15:00:20Z", mode, true, None, Some(20), false), vec![Action::PromptAdhoc]);
            assert!(sim.tick("2026-11-12T15:05:00Z", mode, true, None, Some(300), false).is_empty(), "once per mic session");
            assert!(!sim.r.is_auto_session());
            // Mic goes idle (still debounced-joined), then a new continuous session.
            assert!(sim.tick("2026-11-12T15:05:02Z", mode, true, None, None, false).is_empty());
            assert_eq!(sim.tick("2026-11-12T15:05:30Z", mode, true, None, Some(26), false), vec![Action::PromptAdhoc]);
        }
        // Off: nothing.
        sim.r = AutoRecorder::default();
        assert!(sim.tick("2026-11-12T15:00:20Z", RecordMode::Off, true, None, Some(30), false).is_empty());
    }

    #[test]
    fn adhoc_title_and_mode_parse() {
        use chrono::{FixedOffset, TimeZone};
        let est = FixedOffset::west_opt(5 * 3600).unwrap();
        assert_eq!(adhoc_title(&est.with_ymd_and_hms(2026, 11, 12, 9, 5, 0).unwrap()), "Call 09:05");
        assert_eq!(RecordMode::parse("auto"), Some(RecordMode::Auto));
        assert_eq!(RecordMode::parse("bogus"), None);
        assert_eq!(RecordMode::default().as_str(), "prompt");
    }
}
