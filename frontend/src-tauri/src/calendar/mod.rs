//! Calendar integration (read-only): a private iCal link (preferred, no
//! cloud project) or gcloud Application Default Credentials (Google API).
//!
//! - Knows upcoming meetings so the app can prompt to record (see `poller`).
//! - Matches a finished recording to its calendar event and stores the event
//!   JSON on the meeting row + metadata.json.
//!
//! If credentials are missing/invalid the whole feature is silently off and
//! the app behaves exactly as before.

pub mod autorecord;
pub mod google;
pub mod ical;
pub mod mic_activity;
pub mod people;
pub mod poller;

use chrono::{DateTime, Duration, NaiveTime, TimeZone, Utc};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::SqlitePool;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

use google::GoogleCalendarClient;
use ical::IcalClient;

/// Where events come from. Both yield Google-API-shaped JSON.
pub(crate) enum Source {
    Google(Arc<GoogleCalendarClient>),
    Ical(Arc<IcalClient>),
}

impl Source {
    async fn list_events(&self, min: DateTime<Utc>, max: DateTime<Utc>) -> anyhow::Result<Vec<Value>> {
        match self {
            Source::Google(c) => c.list_events(min, max).await,
            Source::Ical(c) => c.list_events(min, max).await,
        }
    }

    async fn directory_name(&self, email: &str) -> anyhow::Result<Option<String>> {
        match self {
            Source::Google(c) => c.directory_name(email).await,
            Source::Ical(_) => Ok(None),
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Source::Google(_) => "google",
            Source::Ical(_) => "ical",
        }
    }
}

/// Minimum overlap (fraction of recording OR of event) to count as a match.
const MIN_OVERLAP_FRAC: f64 = 0.25;
const DESCRIPTION_MAX_CHARS: usize = 1500;
/// How far ahead the in-app "Upcoming" list looks (the poller itself caches
/// through the end of tomorrow for the tray menu).
pub const LOOKAHEAD_HOURS: i64 = 12;
/// Prompt when an event starts within this window...
const PROMPT_BEFORE: Duration = Duration::minutes(2);
/// ...or started less than this long ago.
const PROMPT_AFTER: Duration = Duration::minutes(10);
/// A pending (prompt-started) event is only trusted for recordings that began
/// within this long of the Record click.
const PENDING_MAX_AGE: Duration = Duration::minutes(5);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CalendarAttendee {
    pub name: String,
    pub email: String,
    pub response_status: Option<String>,
    pub organizer: bool,
    /// This attendee is the calendar owner (you).
    #[serde(default)]
    pub is_self: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CalendarEvent {
    pub id: String,
    pub summary: String,
    /// RFC3339
    pub start: String,
    /// RFC3339
    pub end: String,
    pub attendees: Vec<CalendarAttendee>,
    pub organizer: Option<String>,
    pub html_link: Option<String>,
    pub location: Option<String>,
    pub description: Option<String>,
    pub video_url: Option<String>,
}

impl CalendarEvent {
    pub fn bounds(&self) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
        let s = DateTime::parse_from_rfc3339(&self.start).ok()?.with_timezone(&Utc);
        let e = DateTime::parse_from_rfc3339(&self.end).ok()?.with_timezone(&Utc);
        Some((s, e))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarStatus {
    pub enabled: bool,
    pub reason: Option<String>,
    /// "ical" | "google" when enabled.
    #[serde(default)]
    pub source: Option<String>,
}

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

pub(crate) struct CalendarState {
    client: RwLock<Option<Arc<Source>>>,
    status: RwLock<CalendarStatus>,
    /// All timed events from now-15min through end of tomorrow (local).
    pub(crate) upcoming: RwLock<Vec<CalendarEvent>>,
    pending: Mutex<Option<(CalendarEvent, DateTime<Utc>)>>,
    /// Event ids recorded or dismissed this app session (auto-record skips them).
    excluded: Mutex<HashSet<String>>,
}

pub(crate) static STATE: Lazy<CalendarState> = Lazy::new(|| CalendarState {
    client: RwLock::new(None),
    status: RwLock::new(CalendarStatus {
        enabled: false,
        reason: Some("Calendar not initialized".to_string()),
        source: None,
    }),
    upcoming: RwLock::new(Vec::new()),
    pending: Mutex::new(None),
    excluded: Mutex::new(HashSet::new()),
});

/// Pick a source once: the pasted iCal link if there is one, otherwise gcloud
/// credentials. Returns false (feature off) when neither works.
pub(crate) async fn init_client(ical_url: Option<String>) -> bool {
    if let Some(url) = ical_url {
        *STATE.client.write().await = Some(Arc::new(Source::Ical(Arc::new(IcalClient::new(url)))));
        *STATE.status.write().await = CalendarStatus {
            enabled: true,
            reason: None,
            source: Some("ical".to_string()),
        };
        log::info!("Calendar: using private iCal link");
        return true;
    }
    match google::AdcCredentials::load() {
        Ok(creds) => {
            *STATE.client.write().await =
                Some(Arc::new(Source::Google(Arc::new(GoogleCalendarClient::new(creds)))));
            *STATE.status.write().await = CalendarStatus {
                enabled: true,
                reason: None,
                source: Some("google".to_string()),
            };
            log::info!("Calendar: Google credentials loaded, calendar integration enabled");
            true
        }
        Err(e) => {
            log::info!("Calendar: integration disabled ({})", e);
            *STATE.client.write().await = None;
            *STATE.status.write().await = CalendarStatus {
                enabled: false,
                reason: Some(e.to_string()),
                source: None,
            };
            false
        }
    }
}

async fn client() -> Option<Arc<Source>> {
    STATE.client.read().await.clone()
}

/// Look up real names for attendees that only have an email handle (max 15 per call).
/// Quietly gives up for the session on an email when the directory can't answer.
pub(crate) async fn resolve_attendee_names(events: &[CalendarEvent]) {
    people::reload();
    let pending = people::unresolved(
        events
            .iter()
            .flat_map(|e| e.attendees.iter())
            .filter(|a| !a.is_self)
            .map(|a| (a.name.as_str(), a.email.as_str())),
    );
    if pending.is_empty() {
        return;
    }
    let Some(source) = client().await else { return };
    for email in pending.into_iter().take(15) {
        people::mark_tried(&email);
        match source.directory_name(&email).await {
            Ok(Some(name)) => {
                log::info!("Calendar: resolved a real name from the directory");
                people::remember(&email, &name);
            }
            Ok(None) => {}
            Err(e) => {
                log::info!("Calendar: directory lookup unavailable ({})", e);
                break; // scope/API problem: don't hammer it
            }
        }
    }
}

/// Fetch and normalize events overlapping the window.
pub(crate) async fn fetch_events(
    time_min: DateTime<Utc>,
    time_max: DateTime<Utc>,
) -> anyhow::Result<Vec<CalendarEvent>> {
    let client = client()
        .await
        .ok_or_else(|| anyhow::anyhow!("calendar disabled"))?;
    let raw = client.list_events(time_min, time_max).await?;
    Ok(raw.iter().filter_map(parse_event).collect())
}

// ---------------------------------------------------------------------------
// Pure helpers (unit tested)
// ---------------------------------------------------------------------------

static VIDEO_URL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r#"(?i)https?://(?:[a-z0-9-]+\.)*(?:zoom\.us|zoomgov\.com|teams\.microsoft\.com|teams\.live\.com|meet\.google\.com)(?:/[^\s"'<>]*)?"#,
    )
    .expect("valid video url regex")
});

static DEFAULT_TITLE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^Meeting (?:\d{2}_\d{2}_\d{2}_\d{2}_\d{2}_\d{2}|\d{4}-\d{2}-\d{2}_\d{2}-\d{2}-\d{2})$").expect("valid title regex")
});

/// True when the title is still auto-generated: frontend "Meeting dd_mm_yy_hh_mm_ss"
/// or backend fallback "Meeting YYYY-MM-DD_HH-MM-SS".
pub fn is_default_title(title: &str) -> bool {
    DEFAULT_TITLE_RE.is_match(title.trim())
}

fn find_video_url_in_text(text: &str) -> Option<String> {
    let m = VIDEO_URL_RE.find(text)?;
    let url = m
        .as_str()
        .replace("&amp;", "&")
        .trim_end_matches(|c| matches!(c, '.' | ',' | ';' | ')' | ']'))
        .to_string();
    Some(url)
}

/// Detect a joinable video link: hangoutLink, conferenceData entry points,
/// then location / description text.
pub fn detect_video_url(raw: &Value) -> Option<String> {
    if let Some(link) = raw.get("hangoutLink").and_then(|v| v.as_str()) {
        if !link.is_empty() {
            return Some(link.to_string());
        }
    }

    if let Some(eps) = raw
        .pointer("/conferenceData/entryPoints")
        .and_then(|v| v.as_array())
    {
        let uri_of = |ep: &Value| ep.get("uri").and_then(|u| u.as_str()).map(String::from);
        if let Some(uri) = eps
            .iter()
            .find(|ep| ep.get("entryPointType").and_then(|t| t.as_str()) == Some("video"))
            .and_then(uri_of)
        {
            return Some(uri);
        }
        if let Some(uri) = eps
            .iter()
            .filter_map(uri_of)
            .find(|u| u.starts_with("http"))
        {
            return Some(uri);
        }
    }

    for field in ["location", "description"] {
        if let Some(text) = raw.get(field).and_then(|v| v.as_str()) {
            if let Some(url) = find_video_url_in_text(text) {
                return Some(url);
            }
        }
    }
    None
}

/// Normalize a raw Google event. Returns None for cancelled, all-day, and
/// events the user (attendee self=true) declined.
pub fn parse_event(raw: &Value) -> Option<CalendarEvent> {
    if raw.get("status").and_then(|v| v.as_str()) == Some("cancelled") {
        return None;
    }
    // All-day events have start.date, not start.dateTime.
    let start = raw.pointer("/start/dateTime")?.as_str()?.to_string();
    let end = raw.pointer("/end/dateTime")?.as_str()?.to_string();
    let id = raw.get("id")?.as_str()?.to_string();

    let raw_attendees = raw
        .get("attendees")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let declined = raw_attendees.iter().any(|a| {
        a.get("self").and_then(|v| v.as_bool()) == Some(true)
            && a.get("responseStatus").and_then(|v| v.as_str()) == Some("declined")
    });
    if declined {
        return None;
    }

    let attendees = raw_attendees
        .iter()
        .filter(|a| a.get("resource").and_then(|v| v.as_bool()) != Some(true))
        .filter_map(|a| {
            let email = a.get("email")?.as_str()?.to_string();
            let name = a
                .get("displayName")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from)
                .or_else(|| people::lookup(&email))
                .unwrap_or_else(|| email.split('@').next().unwrap_or(&email).to_string());
            Some(CalendarAttendee {
                name,
                email,
                response_status: a
                    .get("responseStatus")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                organizer: a.get("organizer").and_then(|v| v.as_bool()).unwrap_or(false),
                is_self: a.get("self").and_then(|v| v.as_bool()).unwrap_or(false),
            })
        })
        .collect();

    let str_field = |k: &str| {
        raw.get(k)
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from)
    };

    Some(CalendarEvent {
        id,
        summary: str_field("summary")
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "(No title)".to_string()),
        start,
        end,
        attendees,
        organizer: raw
            .pointer("/organizer/email")
            .and_then(|v| v.as_str())
            .map(String::from),
        html_link: str_field("htmlLink"),
        location: str_field("location"),
        description: str_field("description")
            .map(|d| d.chars().take(DESCRIPTION_MAX_CHARS).collect()),
        video_url: detect_video_url(raw),
    })
}

/// Best event for a recording window (same rule as import-meetily.py
/// match_event): overlap >= 25% of the recording or of the event; prefer
/// larger overlap, then events with attendees, then shorter events.
pub fn best_match(
    events: &[CalendarEvent],
    rec_start: DateTime<Utc>,
    rec_end: DateTime<Utc>,
) -> Option<CalendarEvent> {
    let rec_secs = (rec_end - rec_start).num_seconds().max(1) as f64;
    let mut best: Option<(&CalendarEvent, (i64, usize, i64))> = None;

    for e in events {
        let Some((es, ee)) = e.bounds() else { continue };
        let ov = (rec_end.min(ee) - rec_start.max(es)).num_seconds();
        if ov <= 0 {
            continue;
        }
        let ev_secs = (ee - es).num_seconds().max(1) as f64;
        let frac = (ov as f64 / rec_secs).max(ov as f64 / ev_secs);
        if frac < MIN_OVERLAP_FRAC {
            continue;
        }
        let score = (ov, e.attendees.len().min(2), -(ee - es).num_seconds());
        if best.as_ref().map_or(true, |(_, s)| score > *s) {
            best = Some((e, score));
        }
    }
    best.map(|(e, _)| e.clone())
}

/// End of tomorrow (= start of the day after tomorrow) in `now`'s timezone,
/// as UTC. Used as the poller's fetch horizon.
pub fn end_of_tomorrow<Tz: TimeZone>(now: &DateTime<Tz>) -> DateTime<Utc> {
    let day_after = now.date_naive() + Duration::days(2);
    let midnight = day_after.and_time(NaiveTime::MIN);
    now.timezone()
        .from_local_datetime(&midnight)
        .earliest()
        // Midnight skipped by a DST jump: fall back to 1am.
        .or_else(|| now.timezone().from_local_datetime(&(midnight + Duration::hours(1))).earliest())
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|| now.with_timezone(&Utc) + Duration::hours(48))
}

/// The one other person on a 1:1: you are an attendee and exactly one other
/// real attendee exists. Lets "Others" be shown by name without any audio work.
pub fn one_other_name(event: &CalendarEvent) -> Option<String> {
    if !event.attendees.iter().any(|a| a.is_self) {
        return None;
    }
    let others: Vec<&CalendarAttendee> = event.attendees.iter().filter(|a| !a.is_self).collect();
    (others.len() == 1 && !others[0].name.trim().is_empty()).then(|| others[0].name.trim().to_string())
}

/// Should the poller prompt for this event right now?
pub fn should_prompt(event: &CalendarEvent, now: DateTime<Utc>) -> bool {
    if event.video_url.is_none() {
        return false;
    }
    let Some((start, end)) = event.bounds() else { return false };
    if end <= now {
        return false;
    }
    start <= now + PROMPT_BEFORE && now - start < PROMPT_AFTER
}

// ---------------------------------------------------------------------------
// Save-time attachment
// ---------------------------------------------------------------------------

/// Recording window from the meeting folder's metadata.json
/// (created_at .. completed_at, or created_at + duration_seconds).
fn recording_window_from_metadata(folder: &Path) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    let raw = std::fs::read_to_string(folder.join("metadata.json")).ok()?;
    let v: Value = serde_json::from_str(&raw).ok()?;
    let parse = |k: &str| {
        v.get(k)
            .and_then(|x| x.as_str())
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|d| d.with_timezone(&Utc))
    };
    let start = parse("created_at")?;
    let end = parse("completed_at")
        .or_else(|| {
            v.get("duration_seconds")
                .and_then(|d| d.as_f64())
                .map(|secs| start + Duration::milliseconds((secs * 1000.0) as i64))
        })
        .unwrap_or_else(Utc::now);
    Some((start, end.max(start)))
}

fn write_event_to_metadata(folder: &Path, event_json: &Value) -> anyhow::Result<()> {
    let path = folder.join("metadata.json");
    if !path.exists() {
        return Ok(());
    }
    let raw = std::fs::read_to_string(&path)?;
    let mut v: Value = serde_json::from_str(&raw)?;
    let Some(obj) = v.as_object_mut() else {
        anyhow::bail!("metadata.json root is not an object");
    };
    obj.insert("calendar_event".to_string(), event_json.clone());
    let tmp = folder.join(format!(".metadata.json.calendar.{}.tmp", uuid::Uuid::new_v4()));
    std::fs::write(&tmp, serde_json::to_string_pretty(&v)?)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// Called after a meeting row is created. Attaches the prompt-started event
/// (if recent) or the best window match. Renames the meeting if its title is
/// still the auto default. Returns the new title when renamed.
/// Never fails the save: all errors are logged and swallowed.
pub async fn attach_to_saved_meeting(
    pool: &SqlitePool,
    meeting_id: &str,
    meeting_title: &str,
    folder_path: Option<&str>,
) -> Option<String> {
    let pending = STATE.pending.lock().await.take();
    if pending.is_none() && client().await.is_none() {
        return None;
    }

    let folder = folder_path.map(Path::new);
    let window = folder.and_then(recording_window_from_metadata);

    let event = match pending {
        Some((event, clicked_at))
            if window.map_or(true, |(start, _)| {
                (start - clicked_at).num_seconds().abs() <= PENDING_MAX_AGE.num_seconds()
            }) =>
        {
            Some(event)
        }
        _ => match window {
            Some((start, end)) => match fetch_events(start, end).await {
                Ok(events) => best_match(&events, start, end),
                Err(e) => {
                    log::warn!("Calendar: window match failed for {}: {}", meeting_id, e);
                    None
                }
            },
            None => None,
        },
    }?;

    let event_json = serde_json::to_value(&event).ok()?;
    let event_str = event_json.to_string();

    if let Err(e) = crate::database::repositories::meeting::MeetingsRepository::set_calendar_event(
        pool, meeting_id, &event_str,
    )
    .await
    {
        log::warn!("Calendar: failed to store event on meeting {}: {}", meeting_id, e);
    }

    if let Some(folder) = folder {
        if let Err(e) = write_event_to_metadata(folder, &event_json) {
            log::warn!("Calendar: failed to write calendar_event to metadata.json: {}", e);
        }
    }

    log::info!("Calendar: attached event {} to meeting {}", event.id, meeting_id);

    // 1:1: show the other person by name instead of "Others".
    if let Some(name) = one_other_name(&event) {
        match sqlx::query("UPDATE transcripts SET speaker = ? WHERE meeting_id = ? AND speaker = 'Others'")
            .bind(&name)
            .bind(meeting_id)
            .execute(pool)
            .await
        {
            Ok(r) => log::info!("Calendar: labelled {} 'Others' lines as the 1:1 attendee", r.rows_affected()),
            Err(e) => log::warn!("Calendar: failed to name 1:1 speaker for {}: {}", meeting_id, e),
        }
    }

    if is_default_title(meeting_title) && !event.summary.is_empty() {
        match crate::database::repositories::meeting::MeetingsRepository::update_meeting_name(
            pool,
            meeting_id,
            &event.summary,
        )
        .await
        {
            Ok(true) => return Some(event.summary.clone()),
            Ok(false) => {}
            Err(e) => log::warn!("Calendar: failed to rename meeting {}: {}", meeting_id, e),
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

/// Saved iCal link from the app store, if any. Never logged.
pub(crate) fn load_ical_url<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Option<String> {
    use tauri_plugin_store::StoreExt;
    app.store(ical::STORE_FILE)
        .ok()?
        .get(ical::STORE_KEY)?
        .as_str()
        .map(String::from)
        .filter(|u| !u.is_empty())
}

/// Connect (Some(url)) or disconnect (None) the private iCal link. A new link
/// is downloaded and parsed first; nothing is saved if that fails.
#[tauri::command]
pub async fn calendar_set_ical_url(
    app: tauri::AppHandle<tauri::Wry>,
    url: Option<String>,
) -> Result<CalendarStatus, String> {
    use tauri_plugin_store::StoreExt;
    let store = app
        .store(ical::STORE_FILE)
        .map_err(|e| format!("Failed to access store: {}", e))?;

    match url.filter(|u| !u.trim().is_empty()) {
        Some(raw) => {
            let url = ical::normalize_url(&raw).map_err(|e| e.to_string())?;
            let probe = IcalClient::new(url.clone());
            let count = probe.validate().await.map_err(|e| e.to_string())?;
            store.set(ical::STORE_KEY, serde_json::json!(url));
            store.save().map_err(|e| format!("Failed to save store: {}", e))?;
            init_client(Some(url)).await;
            log::info!("Calendar: iCal link connected ({} events in the next week)", count);
            // Show meetings right away instead of waiting for the next poll.
            let now = Utc::now();
            let horizon = end_of_tomorrow(&now.with_timezone(&chrono::Local));
            if let Ok(events) = fetch_events(now - Duration::minutes(15), horizon).await {
                *STATE.upcoming.write().await = events.clone();
                crate::tray::set_calendar_events(&app, true, &events);
            }
        }
        None => {
            store.delete(ical::STORE_KEY);
            store.save().map_err(|e| format!("Failed to save store: {}", e))?;
            init_client(None).await;
            if !STATE.status.read().await.enabled {
                STATE.upcoming.write().await.clear();
                crate::tray::set_calendar_events(&app, false, &[]);
            }
            log::info!("Calendar: iCal link removed");
        }
    }
    Ok(STATE.status.read().await.clone())
}

#[tauri::command]
pub async fn calendar_status() -> CalendarStatus {
    STATE.status.read().await.clone()
}

/// Upcoming (not yet ended) events in the next 12h, from the poller cache.
#[tauri::command]
pub async fn calendar_upcoming() -> Vec<CalendarEvent> {
    let now = Utc::now();
    let horizon = now + Duration::hours(LOOKAHEAD_HOURS);
    STATE
        .upcoming
        .read()
        .await
        .iter()
        .filter(|e| e.bounds().map_or(false, |(start, end)| end > now && start < horizon))
        .cloned()
        .collect()
}

/// Everything not yet ended through the end of tomorrow, from the poller cache
/// (the Home screen's "Coming up").
#[tauri::command]
pub async fn calendar_agenda() -> Vec<CalendarEvent> {
    let now = Utc::now();
    STATE
        .upcoming
        .read()
        .await
        .iter()
        .filter(|e| e.bounds().map_or(false, |(_, end)| end > now))
        .cloned()
        .collect()
}

#[derive(Debug, Serialize)]
pub struct HomeMeeting {
    pub id: String,
    pub title: String,
    /// Event start when the meeting matched a calendar event, else when it was saved. RFC3339.
    pub when: String,
    /// Other attendees' names (you excluded), empty when no calendar match.
    pub attendees: Vec<String>,
}

/// Recent saved meetings with who was in them, for the Home list.
#[tauri::command]
pub async fn calendar_home_meetings(
    state: tauri::State<'_, crate::state::AppState>,
    limit: Option<i64>,
) -> Result<Vec<HomeMeeting>, String> {
    use sqlx::Row;
    let rows = sqlx::query(
        "SELECT id, title, created_at, calendar_event_json FROM meetings ORDER BY created_at DESC LIMIT ?",
    )
    .bind(limit.unwrap_or(60).clamp(1, 500))
    .fetch_all(state.db_manager.pool())
    .await
    .map_err(|e| e.to_string())?;

    Ok(rows
        .into_iter()
        .filter_map(|r| {
            let id: String = r.try_get("id").ok()?;
            let title: String = r.try_get("title").ok()?;
            let created: DateTime<Utc> = r.try_get("created_at").ok()?;
            let event: Option<CalendarEvent> = r
                .try_get::<Option<String>, _>("calendar_event_json")
                .ok()
                .flatten()
                .and_then(|j| serde_json::from_str(&j).ok());
            let (when, attendees) = match event {
                Some(e) => (
                    e.start.clone(),
                    e.attendees.iter().filter(|a| !a.is_self).map(|a| people::lookup(&a.email).unwrap_or_else(|| a.name.clone())).collect(),
                ),
                None => (created.to_rfc3339(), Vec::new()),
            };
            Some(HomeMeeting { id, title, when, attendees })
        })
        .collect())
}

/// Name to show instead of "Others" while recording a 1:1 (the recording's
/// calendar event, else the single event happening right now). None when unsure.
#[tauri::command]
pub async fn calendar_current_other_name() -> Option<String> {
    if let Some((event, _)) = STATE.pending.lock().await.as_ref() {
        return one_other_name(event);
    }
    let upcoming = STATE.upcoming.read().await;
    // Same pick as auto-record, so the live label matches the recording's title.
    autorecord::matching_event(&upcoming, Utc::now()).and_then(one_other_name)
}

#[tauri::command]
pub async fn calendar_event_for_window(
    start_rfc3339: String,
    end_rfc3339: String,
) -> Result<Option<CalendarEvent>, String> {
    if client().await.is_none() {
        return Ok(None);
    }
    let start = DateTime::parse_from_rfc3339(&start_rfc3339)
        .map_err(|e| format!("invalid start: {}", e))?
        .with_timezone(&Utc);
    let end = DateTime::parse_from_rfc3339(&end_rfc3339)
        .map_err(|e| format!("invalid end: {}", e))?
        .with_timezone(&Utc);
    let events = fetch_events(start, end.max(start)).await.map_err(|e| e.to_string())?;
    Ok(best_match(&events, start, end.max(start)))
}

/// Remember (or clear) the event a recording is being started for, so the
/// save path can attach it.
#[tauri::command]
pub async fn calendar_set_pending_event(event: Option<CalendarEvent>) {
    if let Some(e) = &event {
        STATE.excluded.lock().await.insert(e.id.clone());
    }
    *STATE.pending.lock().await = event.map(|e| (e, Utc::now()));
}

/// The user dismissed the prompt for this event: don't auto-record or
/// re-prompt it on join this session.
#[tauri::command]
pub async fn calendar_dismiss_event(event_id: String) {
    STATE.excluded.lock().await.insert(event_id);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn timed(id: &str, start: &str, end: &str) -> Value {
        json!({
            "id": id,
            "status": "confirmed",
            "summary": format!("Event {}", id),
            "htmlLink": "https://www.google.com/calendar/event?eid=x",
            "start": {"dateTime": start},
            "end": {"dateTime": end},
            "organizer": {"email": "boss@example.com"},
        })
    }

    #[test]
    fn zoom_link_in_description() {
        let mut raw = timed("z", "2026-11-12T10:00:00-05:00", "2026-11-12T10:30:00-05:00");
        raw["description"] =
            json!("Join: <a href=\"https://example.zoom.us/j/123456?pwd=abc&amp;x=1\">Zoom</a>.");
        let e = parse_event(&raw).unwrap();
        assert_eq!(e.video_url.as_deref(), Some("https://example.zoom.us/j/123456?pwd=abc&x=1"));
    }

    #[test]
    fn teams_link_in_location() {
        let mut raw = timed("t", "2026-11-12T10:00:00Z", "2026-11-12T11:00:00Z");
        raw["location"] = json!("Microsoft Teams Meeting https://teams.microsoft.com/l/meetup-join/19%3ameeting_abc");
        let e = parse_event(&raw).unwrap();
        assert!(e.video_url.unwrap().starts_with("https://teams.microsoft.com/l/meetup-join/"));

        let mut raw2 = timed("t2", "2026-11-12T10:00:00Z", "2026-11-12T11:00:00Z");
        raw2["description"] = json!("Join https://teams.live.com/meet/9999");
        assert_eq!(
            parse_event(&raw2).unwrap().video_url.as_deref(),
            Some("https://teams.live.com/meet/9999")
        );
    }

    #[test]
    fn meet_hangout_link_and_conference_data() {
        let mut raw = timed("m", "2026-11-12T10:00:00Z", "2026-11-12T11:00:00Z");
        raw["hangoutLink"] = json!("https://meet.google.com/abc-defg-hij");
        assert_eq!(
            parse_event(&raw).unwrap().video_url.as_deref(),
            Some("https://meet.google.com/abc-defg-hij")
        );

        let mut raw2 = timed("c", "2026-11-12T10:00:00Z", "2026-11-12T11:00:00Z");
        raw2["conferenceData"] = json!({"entryPoints": [
            {"entryPointType": "phone", "uri": "tel:+1-555-0100"},
            {"entryPointType": "video", "uri": "https://example.zoom.us/j/42"}
        ]});
        assert_eq!(
            parse_event(&raw2).unwrap().video_url.as_deref(),
            Some("https://example.zoom.us/j/42")
        );
    }

    #[test]
    fn one_other_name_only_for_true_1on1() {
        let mut ev = parse_event(&timed("a", "2026-11-13T16:30:00Z", "2026-11-13T17:00:00Z")).unwrap();
        let mk = |name: &str, is_self: bool| CalendarAttendee {
            name: name.to_string(),
            email: format!("{}@example.com", name.to_lowercase()),
            response_status: None,
            organizer: false,
            is_self,
        };
        ev.attendees = vec![mk("Me", true), mk("Amelia", false)];
        assert_eq!(one_other_name(&ev).as_deref(), Some("Amelia"));
        ev.attendees = vec![mk("Me", true), mk("Amelia", false), mk("Sam", false)];
        assert_eq!(one_other_name(&ev), None, "group meeting");
        ev.attendees = vec![mk("Amelia", false)];
        assert_eq!(one_other_name(&ev), None, "no self marker: not sure it is a 1:1");
    }

    #[test]
    fn declined_event_skipped() {
        let mut raw = timed("d", "2026-11-12T10:00:00Z", "2026-11-12T11:00:00Z");
        raw["attendees"] = json!([
            {"email": "me@example.com", "self": true, "responseStatus": "declined"},
            {"email": "other@example.com", "responseStatus": "accepted"}
        ]);
        assert!(parse_event(&raw).is_none());
    }

    #[test]
    fn all_day_event_skipped() {
        let raw = json!({
            "id": "a", "status": "confirmed", "summary": "Holiday",
            "start": {"date": "2026-11-12"}, "end": {"date": "2026-11-13"}
        });
        assert!(parse_event(&raw).is_none());
    }

    #[test]
    fn cancelled_event_skipped() {
        let mut raw = timed("x", "2026-11-12T10:00:00Z", "2026-11-12T11:00:00Z");
        raw["status"] = json!("cancelled");
        assert!(parse_event(&raw).is_none());
    }

    #[test]
    fn no_video_event_parsed_without_url_and_not_prompted() {
        let mut raw = timed("n", "2026-11-12T10:00:00Z", "2026-11-12T11:00:00Z");
        raw["location"] = json!("Conference Room 4");
        raw["description"] = json!("x".repeat(3000));
        raw["attendees"] = json!([
            {"email": "me@example.com", "self": true, "responseStatus": "accepted", "organizer": true},
            {"email": "room@resource.calendar.google.com", "resource": true},
            {"email": "pat@example.com", "displayName": "Pat", "responseStatus": "tentative"}
        ]);
        let e = parse_event(&raw).unwrap();
        assert!(e.video_url.is_none());
        assert_eq!(e.description.as_ref().unwrap().chars().count(), 1500);
        assert_eq!(e.attendees.len(), 2);
        assert_eq!(e.attendees[0].name, "me");
        assert!(e.attendees[0].organizer);
        assert_eq!(e.attendees[1].name, "Pat");
        let now = DateTime::parse_from_rfc3339("2026-11-12T10:00:00Z").unwrap().with_timezone(&Utc);
        assert!(!should_prompt(&e, now));
    }

    #[test]
    fn prompt_window() {
        let mut raw = timed("p", "2026-11-12T10:00:00Z", "2026-11-12T11:00:00Z");
        raw["hangoutLink"] = json!("https://meet.google.com/abc");
        let e = parse_event(&raw).unwrap();
        let at = |s: &str| DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc);
        assert!(!should_prompt(&e, at("2026-11-12T09:57:00Z")));
        assert!(should_prompt(&e, at("2026-11-12T09:58:00Z")));
        assert!(should_prompt(&e, at("2026-11-12T10:09:00Z")));
        assert!(!should_prompt(&e, at("2026-11-12T10:10:00Z")));
    }

    #[test]
    fn window_match_rules() {
        let mk = |id: &str, s: &str, e: &str, attendees: usize| {
            let mut ev = parse_event(&timed(id, s, e)).unwrap();
            ev.attendees = (0..attendees)
                .map(|i| CalendarAttendee {
                    name: format!("p{}", i),
                    email: format!("p{}@example.com", i),
                    response_status: None,
                    organizer: false,
                    is_self: false,
                })
                .collect();
            ev
        };
        let at = |s: &str| DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc);
        let block = mk("block", "2026-11-12T09:00:00Z", "2026-11-12T12:00:00Z", 0);
        let mtg = mk("mtg", "2026-11-12T10:00:00Z", "2026-11-12T10:30:00Z", 3);
        let other = mk("other", "2026-11-12T10:25:00Z", "2026-11-12T11:00:00Z", 2);
        let events = vec![block.clone(), mtg, other];

        // Recording 10:00-10:30: block and mtg tie on overlap; attendees win.
        let m = best_match(&events, at("2026-11-12T10:00:00Z"), at("2026-11-12T10:30:00Z")).unwrap();
        assert_eq!(m.id, "mtg");

        // No overlap -> None.
        assert!(best_match(&events, at("2026-11-12T13:00:00Z"), at("2026-11-12T13:30:00Z")).is_none());

        // Tiny overlap (<25% of both) -> not a match.
        let only_block = vec![mk("long", "2026-11-12T09:00:00Z", "2026-11-12T17:00:00Z", 0)];
        assert!(best_match(&only_block, at("2026-11-12T08:00:00Z"), at("2026-11-12T09:05:00Z")).is_none());
        // Short recording fully inside a long event -> match (100% of recording).
        assert!(best_match(&only_block, at("2026-11-12T10:00:00Z"), at("2026-11-12T10:10:00Z")).is_some());
    }

    #[test]
    fn fetch_horizon_is_end_of_tomorrow_local() {
        use chrono::FixedOffset;
        let est = FixedOffset::west_opt(5 * 3600).unwrap();
        // 11pm local on the 12th -> horizon is local midnight starting the 14th.
        let now = est.with_ymd_and_hms(2026, 11, 12, 23, 0, 0).unwrap();
        assert_eq!(end_of_tomorrow(&now).to_rfc3339(), "2026-11-14T05:00:00+00:00");
        // 00:30 local on the 13th (still the 13th locally, 05:30Z).
        let now = est.with_ymd_and_hms(2026, 11, 13, 0, 30, 0).unwrap();
        assert_eq!(end_of_tomorrow(&now).to_rfc3339(), "2026-11-15T05:00:00+00:00");
    }

    #[test]
    fn default_title_detection() {
        assert!(is_default_title("Meeting 12_11_26_10_00_05"));
        assert!(is_default_title("Meeting 2026-11-12_10-00-05"));
        assert!(!is_default_title("Weekly sync"));
        assert!(!is_default_title("Meeting 12_11_26"));
    }
}
