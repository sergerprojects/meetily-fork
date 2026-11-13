//! Calendar source: your Google Calendar through the Claude connector that is already
//! signed in to your Google Workspace. Meetily asks your own Claude Code CLI (headless,
//! only the calendar tools allowed) for your meetings and reads the JSON it returns.
//! No Google Cloud project, no OAuth client, no key in this app.
//!
//! The model makes one `list_events` call and Meetily lifts the connector's RAW result
//! from Claude Code's stream output, so the model never rewrites your calendar.
//! Needs the Google Calendar connector authorized once (run `/mcp` in Claude Code).
//! Each refresh takes ~15-30 s, so results are cached for several minutes.

use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const CACHE_TTL: Duration = Duration::from_secs(8 * 60);
const FETCH_TIMEOUT: Duration = Duration::from_secs(150);

struct Cache {
    fetched: Instant,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    events: Vec<Value>,
}

pub struct ClaudeCalendar {
    cache: Mutex<Option<Cache>>,
}

impl ClaudeCalendar {
    pub fn new() -> Self {
        Self { cache: Mutex::new(None) }
    }

    /// Events overlapping `[min, max)`. Served from cache when it covers the window and is fresh.
    pub async fn list_events(&self, min: DateTime<Utc>, max: DateTime<Utc>) -> Result<Vec<Value>> {
        let mut guard = self.cache.lock().await;
        if let Some(c) = guard.as_ref() {
            if c.fetched.elapsed() < CACHE_TTL && c.from <= min && c.to >= max {
                return Ok(overlapping(&c.events, min, max));
            }
        }
        // Fetch a generous window so the poller's next minutes are all cache hits.
        let (from, to) = (min.min(Utc::now() - ChronoDuration::hours(1)), max.max(Utc::now() + ChronoDuration::hours(48)));
        let events = fetch(from, to).await?;
        let out = overlapping(&events, min, max);
        *guard = Some(Cache { fetched: Instant::now(), from, to, events });
        Ok(out)
    }
}

async fn fetch(from: DateTime<Utc>, to: DateTime<Utc>) -> Result<Vec<Value>> {
    let tool = "mcp__google-calendar__list_events";
    let mut all: Vec<Value> = Vec::new();
    let mut token: Option<String> = None;
    for _ in 0..6 {
        let mut args = json!({
            "startTime": from.to_rfc3339(),
            "endTime": to.to_rfc3339(),
            "orderBy": "startTime",
            "pageSize": 250,
        });
        if let Some(t) = &token {
            args["pageToken"] = json!(t);
        }
        let raw = crate::chat::call_mcp_tool(tool, &args, FETCH_TIMEOUT).await.map_err(|e| anyhow!(e))?;
        let (events, next) = events_from_result(&raw)?;
        all.extend(events);
        token = next;
        if token.is_none() {
            break;
        }
    }
    Ok(all)
}

/// The connector's raw `list_events` result: `{"events":[...Google Calendar v3 events...], "nextPageToken": ...}`.
pub fn events_from_result(raw: &str) -> Result<(Vec<Value>, Option<String>)> {
    let v: Value = serde_json::from_str(raw.trim()).map_err(|_| anyhow!("The calendar connector returned something unreadable"))?;
    let events = v
        .get("events")
        .and_then(|e| e.as_array())
        .ok_or_else(|| anyhow!("The calendar connector returned no event list"))?
        .clone();
    let next = v.get("nextPageToken").and_then(|t| t.as_str()).filter(|t| !t.is_empty()).map(String::from);
    Ok((events, next))
}

fn overlapping(events: &[Value], min: DateTime<Utc>, max: DateTime<Utc>) -> Vec<Value> {
    events
        .iter()
        .filter(|e| {
            let s = e.pointer("/start/dateTime").and_then(|x| x.as_str()).and_then(|x| DateTime::parse_from_rfc3339(x).ok());
            let en = e.pointer("/end/dateTime").and_then(|x| x.as_str()).and_then(|x| DateTime::parse_from_rfc3339(x).ok());
            matches!((s, en), (Some(s), Some(en)) if s.with_timezone(&Utc) < max && en.with_timezone(&Utc) > min)
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const RAW: &str = r#"{"events":[
      {"id":"a1","summary":"Susie / Chris","start":{"dateTime":"2026-11-13T12:00:00-05:00"},"end":{"dateTime":"2026-11-13T12:30:00-05:00"},
       "hangoutLink":"https://meet.google.com/abc-defg-hij","organizer":{"email":"you@example.com"},
       "attendees":[{"email":"sam@example.com","displayName":"Sam Rivera","responseStatus":"accepted"},
                    {"email":"you@example.com","self":true,"responseStatus":"accepted"}]},
      {"id":"allday","summary":"Holiday","start":{"date":"2026-11-13"},"end":{"date":"2026-11-14"}}],
      "nextPageToken":"abc"}"#;

    #[test]
    fn raw_result_flows_through_parse_event_unchanged() {
        let (events, next) = events_from_result(RAW).unwrap();
        assert_eq!(next.as_deref(), Some("abc"));
        assert_eq!(events.len(), 2);
        let parsed: Vec<_> = events.iter().filter_map(crate::calendar::parse_event).collect();
        assert_eq!(parsed.len(), 1, "all-day event is dropped by the shared parser");
        assert_eq!(parsed[0].video_url.as_deref(), Some("https://meet.google.com/abc-defg-hij"));
        assert_eq!(crate::calendar::one_other_name(&parsed[0]).as_deref(), Some("Sam Rivera"));
    }

    #[test]
    fn unreadable_results_are_plain_errors() {
        assert!(events_from_result("not json").is_err());
        assert!(events_from_result("{}").is_err());
    }

    #[test]
    fn overlap_filter() {
        let (events, _) = events_from_result(RAW).unwrap();
        let t = |s: &str| DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc);
        assert_eq!(overlapping(&events, t("2026-11-13T17:10:00Z"), t("2026-11-13T17:20:00Z")).len(), 1);
        assert!(overlapping(&events, t("2026-11-13T19:00:00Z"), t("2026-11-13T20:00:00Z")).is_empty());
    }
}
