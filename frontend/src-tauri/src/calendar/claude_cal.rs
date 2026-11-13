//! Calendar source: your Google Calendar through the Claude connector that is already
//! signed in to your Google Workspace. Meetily asks your own Claude Code CLI (headless,
//! only the calendar tools allowed) for your meetings and reads the JSON it returns.
//! No Google Cloud project, no OAuth client, no key in this app.
//!
//! Needs the Google Calendar connector authorized once (run `/mcp` in Claude Code).
//! Each refresh takes ~15-30 s and a small model call, so results are cached for
//! several minutes; the poller keeps asking, the cache answers.

use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const CACHE_TTL: Duration = Duration::from_secs(8 * 60);
const FETCH_TIMEOUT: Duration = Duration::from_secs(150);
/// Calendar tools Claude may call (read-only).
const TOOLS: &[&str] = &[
    "mcp__google-calendar__list_events",
    "mcp__google-calendar__get_event",
    "mcp__google-calendar__search_events",
    "mcp__google-calendar__list_calendars",
];

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
    let prompt = build_prompt(from, to);
    let text = crate::chat::run_claude(&prompt, "haiku", TOOLS, FETCH_TIMEOUT)
        .await
        .map_err(|e| anyhow!(e))?;
    parse_events(&text)
}

pub fn build_prompt(from: DateTime<Utc>, to: DateTime<Utc>) -> String {
    format!(
        "Read my primary Google Calendar with the google-calendar tools (list every page; expand recurring events into single instances) \
for events between {} and {}. Skip all-day and cancelled events. \
Output ONLY a JSON array, no prose and no code fences. Each item: \
{{\"id\": string, \"summary\": string, \"start\": RFC3339, \"end\": RFC3339, \"location\": string|null, \"description\": string|null, \
\"hangoutLink\": string|null, \"htmlLink\": string|null, \"organizer\": string|null (email), \
\"attendees\": [{{\"email\": string, \"displayName\": string|null, \"responseStatus\": string|null, \"self\": bool}}]}}. \
If a tool says it needs authorization, output exactly: AUTH_NEEDED",
        from.to_rfc3339(),
        to.to_rfc3339()
    )
}

/// Turn Claude's reply into Google-API-shaped event JSON (what `parse_event` expects).
pub fn parse_events(text: &str) -> Result<Vec<Value>> {
    if text.contains("AUTH_NEEDED") || text.to_lowercase().contains("needs") && text.contains("/mcp") {
        bail!("The Google Calendar connector needs authorizing: run /mcp in Claude Code and sign in to google-calendar, then try again");
    }
    let (a, b) = (text.find('['), text.rfind(']'));
    let (Some(a), Some(b)) = (a, b) else {
        bail!("Claude did not return a calendar list");
    };
    let items: Vec<Value> = serde_json::from_str(&text[a..=b]).map_err(|_| anyhow!("Claude's calendar list was not valid JSON"))?;
    Ok(items.into_iter().filter_map(to_google_shape).collect())
}

fn to_google_shape(i: Value) -> Option<Value> {
    let start = i.get("start")?.as_str()?;
    let end = i.get("end")?.as_str()?;
    DateTime::parse_from_rfc3339(start).ok()?;
    DateTime::parse_from_rfc3339(end).ok()?;
    let id = i
        .get("id")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| format!("{}_{}", i.get("summary").and_then(|v| v.as_str()).unwrap_or("event"), start));
    let mut v = json!({
        "id": id,
        "status": "confirmed",
        "start": {"dateTime": start},
        "end": {"dateTime": end},
    });
    for (src, dst) in [("summary", "summary"), ("location", "location"), ("description", "description"), ("hangoutLink", "hangoutLink"), ("htmlLink", "htmlLink")] {
        if let Some(s) = i.get(src).and_then(|x| x.as_str()).filter(|s| !s.is_empty()) {
            v[dst] = json!(s);
        }
    }
    if let Some(o) = i.get("organizer").and_then(|x| x.as_str()).filter(|s| !s.is_empty()) {
        v["organizer"] = json!({"email": o});
    }
    if let Some(list) = i.get("attendees").and_then(|x| x.as_array()) {
        let attendees: Vec<Value> = list
            .iter()
            .filter_map(|a| {
                let email = a.get("email")?.as_str()?;
                let mut o = json!({"email": email, "self": a.get("self").and_then(|x| x.as_bool()).unwrap_or(false)});
                if let Some(n) = a.get("displayName").and_then(|x| x.as_str()).filter(|s| !s.is_empty()) {
                    o["displayName"] = json!(n);
                }
                if let Some(r) = a.get("responseStatus").and_then(|x| x.as_str()) {
                    o["responseStatus"] = json!(r);
                }
                Some(o)
            })
            .collect();
        if !attendees.is_empty() {
            v["attendees"] = json!(attendees);
        }
    }
    Some(v)
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

    const REPLY: &str = r#"Here you go:
```json
[{"id":"a1","summary":"Susie / Chris","start":"2026-11-13T12:00:00-05:00","end":"2026-11-13T12:30:00-05:00",
  "hangoutLink":"https://meet.google.com/abc-defg-hij","organizer":"you@example.com",
  "attendees":[{"email":"sam@example.com","displayName":"Sam Rivera","responseStatus":"accepted","self":false},
               {"email":"you@example.com","self":true}]},
 {"summary":"bad","start":"not a date","end":"x"}]
```"#;

    #[test]
    fn parses_fenced_reply_into_google_shape_and_feeds_parse_event() {
        let events = parse_events(REPLY).unwrap();
        assert_eq!(events.len(), 1, "malformed item dropped");
        let ev = crate::calendar::parse_event(&events[0]).unwrap();
        assert_eq!(ev.summary, "Susie / Chris");
        assert_eq!(ev.video_url.as_deref(), Some("https://meet.google.com/abc-defg-hij"));
        assert_eq!(ev.attendees.len(), 2);
        assert!(ev.attendees.iter().any(|a| a.is_self));
        assert_eq!(crate::calendar::one_other_name(&ev).as_deref(), Some("Sam Rivera"));
    }

    #[test]
    fn auth_needed_and_garbage_are_errors_with_a_plain_message() {
        assert!(parse_events("AUTH_NEEDED").unwrap_err().to_string().contains("/mcp"));
        assert!(parse_events("I could not read it").is_err());
        assert!(parse_events("[not json]").is_err());
    }

    #[test]
    fn overlap_filter() {
        let events = parse_events(REPLY).unwrap();
        let t = |s: &str| DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc);
        assert_eq!(overlapping(&events, t("2026-11-13T17:10:00Z"), t("2026-11-13T17:20:00Z")).len(), 1);
        assert!(overlapping(&events, t("2026-11-13T19:00:00Z"), t("2026-11-13T20:00:00Z")).is_empty());
    }

    #[test]
    fn prompt_names_the_window_and_the_auth_marker() {
        let t = DateTime::parse_from_rfc3339("2026-11-13T17:00:00Z").unwrap().with_timezone(&Utc);
        let p = build_prompt(t, t + ChronoDuration::hours(48));
        assert!(p.contains("2026-11-13T17:00:00+00:00") && p.contains("AUTH_NEEDED"));
    }
}
