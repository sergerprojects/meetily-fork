//! Calendar source from a private iCal link (Google Calendar: Settings →
//! Integrate calendar → "Secret address in iCal format"). No OAuth, no cloud
//! project: the link itself is the credential, so it is never logged and
//! never included in errors.
//!
//! The feed is parsed into the same JSON shape the Google Calendar API
//! returns, so `parse_event` and everything downstream is shared.
//!
//! Google refreshes these feeds slowly (hours), so meetings added or moved
//! very recently may be missing; the mic-activity prompt covers those.

use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, Duration, Local, NaiveDate, NaiveDateTime, TimeZone, Utc};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::time::Instant;
use tokio::sync::Mutex;

/// Re-download the feed at most this often (Google updates it slowly anyway).
const CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(300);
/// Safety cap on recurrence instances per series in one window.
const MAX_INSTANCES: u16 = 500;

pub const STORE_FILE: &str = "calendar.json";
pub const STORE_KEY: &str = "ical_url";

/// Accept https://… or webcal://… and return an https URL.
pub fn normalize_url(raw: &str) -> Result<String> {
    let t = raw.trim();
    let https = if let Some(rest) = t.strip_prefix("webcal://") {
        format!("https://{}", rest)
    } else {
        t.to_string()
    };
    if !https.starts_with("https://") || https.len() < 12 || https.contains(char::is_whitespace) {
        bail!("Paste the full https:// link from Google Calendar's \"Secret address in iCal format\"");
    }
    Ok(https)
}

/// Google's private feed URL embeds the calendar owner:
/// `/calendar/ical/<percent-encoded email>/private-<token>/basic.ics`.
pub fn owner_email_from_url(url: &str) -> Option<String> {
    let after = url.split("/ical/").nth(1)?;
    let seg = after.split('/').next()?;
    let decoded = percent_decode(seg);
    decoded.contains('@').then(|| decoded.to_lowercase())
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub struct IcalClient {
    url: String,
    owner: Option<String>,
    http: reqwest::Client,
    cache: Mutex<Option<(String, Instant)>>,
}

impl IcalClient {
    pub fn new(url: String) -> Self {
        let owner = owner_email_from_url(&url);
        Self {
            url,
            owner,
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("http client"),
            cache: Mutex::new(None),
        }
    }

    async fn feed(&self, force: bool) -> Result<String> {
        let mut guard = self.cache.lock().await;
        if !force {
            if let Some((text, at)) = guard.as_ref() {
                if at.elapsed() < CACHE_TTL {
                    return Ok(text.clone());
                }
            }
        }
        let resp = self
            .http
            .get(&self.url)
            .send()
            .await
            .map_err(|e| anyhow!("calendar link request failed: {}", e.without_url()))?;
        let status = resp.status();
        if !status.is_success() {
            // 404/403 usually means the link was reset or the admin disabled it.
            bail!("calendar link was rejected (HTTP {})", status.as_u16());
        }
        let text = resp
            .text()
            .await
            .map_err(|e| anyhow!("calendar link response unreadable: {}", e.without_url()))?;
        if !text.contains("BEGIN:VCALENDAR") {
            bail!("that link did not return a calendar (expected an .ics feed)");
        }
        *guard = Some((text.clone(), Instant::now()));
        Ok(text)
    }

    /// Events overlapping `[time_min, time_max)` in Google-API JSON shape.
    pub async fn list_events(&self, time_min: DateTime<Utc>, time_max: DateTime<Utc>) -> Result<Vec<Value>> {
        let text = self.feed(false).await?;
        Ok(events_from_ics(&text, self.owner.as_deref(), time_min, time_max))
    }

    /// Force a download and parse (used to validate a newly pasted link).
    pub async fn validate(&self) -> Result<usize> {
        let text = self.feed(true).await?;
        let now = Utc::now();
        Ok(events_from_ics(&text, self.owner.as_deref(), now - Duration::days(1), now + Duration::days(7)).len())
    }
}

// ---------------------------------------------------------------------------
// Parsing (pure, unit tested)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Prop {
    name: String,
    params: Vec<(String, String)>,
    value: String,
    /// Unfolded original line, fed verbatim to the recurrence engine.
    raw: String,
}

impl Prop {
    fn param(&self, key: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
    }
}

fn unfold(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for l in text.split('\n') {
        let l = l.strip_suffix('\r').unwrap_or(l);
        if (l.starts_with(' ') || l.starts_with('\t')) && !lines.is_empty() {
            lines.last_mut().unwrap().push_str(&l[1..]);
        } else {
            lines.push(l.to_string());
        }
    }
    lines
}

fn parse_prop(line: &str) -> Option<Prop> {
    // Split at the first ':' that is not inside a quoted parameter value.
    let mut in_quotes = false;
    let mut colon = None;
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_quotes = !in_quotes,
            ':' if !in_quotes => {
                colon = Some(i);
                break;
            }
            _ => {}
        }
    }
    let colon = colon?;
    let (head, value) = (&line[..colon], &line[colon + 1..]);

    let mut parts: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut q = false;
    for c in head.chars() {
        match c {
            '"' => {
                q = !q;
                cur.push(c);
            }
            ';' if !q => parts.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    parts.push(cur);

    let name = parts.remove(0).to_uppercase();
    let params = parts
        .into_iter()
        .filter_map(|p| {
            let (k, v) = p.split_once('=')?;
            Some((k.to_uppercase(), v.trim_matches('"').to_string()))
        })
        .collect();
    Some(Prop { name, params, value: value.to_string(), raw: line.to_string() })
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') | Some('N') => out.push('\n'),
                Some(o) => out.push(o),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

enum Dt {
    Time(DateTime<Utc>),
    AllDay(NaiveDate),
}

fn parse_dt(p: &Prop) -> Option<Dt> {
    let v = p.value.trim();
    if p.param("VALUE") == Some("DATE") || (v.len() == 8 && !v.contains('T')) {
        return NaiveDate::parse_from_str(v, "%Y%m%d").ok().map(Dt::AllDay);
    }
    if let Some(utc) = v.strip_suffix('Z') {
        let n = NaiveDateTime::parse_from_str(utc, "%Y%m%dT%H%M%S").ok()?;
        return Some(Dt::Time(Utc.from_utc_datetime(&n)));
    }
    let n = NaiveDateTime::parse_from_str(v, "%Y%m%dT%H%M%S").ok()?;
    let local = match p.param("TZID").and_then(|t| t.parse::<chrono_tz::Tz>().ok()) {
        Some(tz) => tz.from_local_datetime(&n).earliest()?.with_timezone(&Utc),
        None => Local.from_local_datetime(&n).earliest()?.with_timezone(&Utc),
    };
    Some(Dt::Time(local))
}

fn parse_duration(v: &str) -> Option<Duration> {
    // PT1H30M, P1D, PT45M ...
    let v = v.trim().strip_prefix('P')?;
    let (date_part, time_part) = v.split_once('T').unwrap_or((v, ""));
    let mut secs = 0i64;
    let mut num = String::new();
    for c in date_part.chars() {
        if c.is_ascii_digit() {
            num.push(c);
        } else {
            let n: i64 = num.parse().ok()?;
            num.clear();
            secs += match c {
                'W' => n * 7 * 86400,
                'D' => n * 86400,
                _ => return None,
            };
        }
    }
    for c in time_part.chars() {
        if c.is_ascii_digit() {
            num.push(c);
        } else {
            let n: i64 = num.parse().ok()?;
            num.clear();
            secs += match c {
                'H' => n * 3600,
                'M' => n * 60,
                'S' => n,
                _ => return None,
            };
        }
    }
    Some(Duration::seconds(secs))
}

fn email_of(value: &str) -> String {
    let v = value.trim();
    let v = v.strip_prefix("mailto:").or_else(|| v.strip_prefix("MAILTO:")).unwrap_or(v);
    v.trim().to_lowercase()
}

fn rfc3339(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn instance_id(uid: &str, start: DateTime<Utc>) -> String {
    format!("{}_{}", uid, start.format("%Y%m%dT%H%M%SZ"))
}

#[derive(Default)]
struct RawEvent {
    props: Vec<Prop>,
}

impl RawEvent {
    fn first(&self, name: &str) -> Option<&Prop> {
        self.props.iter().find(|p| p.name == name)
    }
    fn text(&self, name: &str) -> Option<String> {
        self.first(name)
            .map(|p| unescape(&p.value))
            .filter(|s| !s.is_empty())
    }
}

fn collect_events(text: &str) -> Vec<RawEvent> {
    let mut events = Vec::new();
    let mut cur: Option<RawEvent> = None;
    for line in unfold(text) {
        if line.eq_ignore_ascii_case("BEGIN:VEVENT") {
            cur = Some(RawEvent::default());
        } else if line.eq_ignore_ascii_case("END:VEVENT") {
            if let Some(e) = cur.take() {
                events.push(e);
            }
        } else if let Some(e) = cur.as_mut() {
            if let Some(p) = parse_prop(&line) {
                e.props.push(p);
            }
        }
    }
    events
}

/// Convert an ICS feed to Google-API-shaped event JSON overlapping the window.
/// All-day, cancelled, and zero-length events are dropped.
pub fn events_from_ics(
    text: &str,
    owner: Option<&str>,
    time_min: DateTime<Utc>,
    time_max: DateTime<Utc>,
) -> Vec<Value> {
    let raws = collect_events(text);

    // uid -> original start instants that were overridden / moved.
    let mut overridden: HashMap<String, HashSet<DateTime<Utc>>> = HashMap::new();
    for e in &raws {
        if let (Some(uid), Some(rid)) = (e.text("UID"), e.first("RECURRENCE-ID")) {
            if let Some(Dt::Time(t)) = parse_dt(rid) {
                overridden.entry(uid).or_default().insert(t);
            }
        }
    }

    let mut out = Vec::new();
    for e in &raws {
        let Some(uid) = e.text("UID") else { continue };
        if e.first("STATUS").map(|p| p.value.eq_ignore_ascii_case("CANCELLED")) == Some(true) {
            continue;
        }
        let Some(Dt::Time(start)) = e.first("DTSTART").and_then(parse_dt) else { continue };
        let length = match e.first("DTEND").and_then(parse_dt) {
            Some(Dt::Time(end)) => end - start,
            _ => e
                .first("DURATION")
                .and_then(|p| parse_duration(&p.value))
                .unwrap_or_else(Duration::zero),
        };
        if length <= Duration::zero() {
            continue;
        }

        let mut starts: Vec<(DateTime<Utc>, String)> = Vec::new();
        if let Some(rid) = e.first("RECURRENCE-ID") {
            // Modified single instance of a series: id keyed by original start.
            if let Some(Dt::Time(orig)) = parse_dt(rid) {
                starts.push((start, instance_id(&uid, orig)));
            }
        } else if e.first("RRULE").is_some() || e.first("RDATE").is_some() {
            let skip = overridden.get(&uid);
            for s in expand_recurrence(e, start, length, time_min, time_max) {
                if skip.map_or(false, |set| set.contains(&s)) {
                    continue;
                }
                starts.push((s, instance_id(&uid, s)));
            }
        } else {
            starts.push((start, uid.clone()));
        }

        for (s, id) in starts {
            let end = s + length;
            if !(s < time_max && end > time_min) {
                continue;
            }
            out.push(build_json(e, &id, s, end, owner));
        }
    }
    out.sort_by(|a, b| {
        a.pointer("/start/dateTime")
            .and_then(|v| v.as_str())
            .cmp(&b.pointer("/start/dateTime").and_then(|v| v.as_str()))
    });
    out
}

fn expand_recurrence(
    e: &RawEvent,
    start: DateTime<Utc>,
    length: Duration,
    time_min: DateTime<Utc>,
    time_max: DateTime<Utc>,
) -> Vec<DateTime<Utc>> {
    let mut lines: Vec<&str> = Vec::new();
    for name in ["DTSTART", "RRULE", "RDATE", "EXDATE"] {
        for p in e.props.iter().filter(|p| p.name == name) {
            lines.push(p.raw.as_str());
        }
    }
    let spec = lines.join("\n");
    let Ok(set) = spec.parse::<rrule::RRuleSet>() else {
        // Unparseable rule: treat as a single event rather than dropping it.
        return vec![start];
    };
    let after = (time_min - length - Duration::minutes(1)).with_timezone(&rrule::Tz::UTC);
    let before = time_max.with_timezone(&rrule::Tz::UTC);
    set.after(after)
        .before(before)
        .all(MAX_INSTANCES)
        .dates
        .into_iter()
        .map(|d| d.with_timezone(&Utc))
        .collect()
}

fn build_json(e: &RawEvent, id: &str, start: DateTime<Utc>, end: DateTime<Utc>, owner: Option<&str>) -> Value {
    let organizer = e.first("ORGANIZER").map(|p| email_of(&p.value));

    let attendees: Vec<Value> = e
        .props
        .iter()
        .filter(|p| p.name == "ATTENDEE")
        .map(|p| {
            let email = email_of(&p.value);
            let status = match p.param("PARTSTAT").map(|s| s.to_uppercase()).as_deref() {
                Some("ACCEPTED") => "accepted",
                Some("DECLINED") => "declined",
                Some("TENTATIVE") => "tentative",
                _ => "needsAction",
            };
            let mut a = json!({
                "email": email,
                "responseStatus": status,
                "self": owner == Some(email.as_str()),
                "resource": p.param("CUTYPE").map(|c| c.eq_ignore_ascii_case("RESOURCE")).unwrap_or(false),
                "organizer": organizer.as_deref() == Some(email.as_str()),
            });
            if let Some(cn) = p.param("CN").filter(|s| !s.is_empty()) {
                a["displayName"] = json!(cn);
            }
            a
        })
        .collect();

    let mut v = json!({
        "id": id,
        "status": "confirmed",
        "start": {"dateTime": rfc3339(start)},
        "end": {"dateTime": rfc3339(end)},
    });
    if let Some(s) = e.text("SUMMARY") {
        v["summary"] = json!(s);
    }
    if let Some(s) = e.text("LOCATION") {
        v["location"] = json!(s);
    }
    if let Some(s) = e.text("DESCRIPTION") {
        v["description"] = json!(s);
    }
    if let Some(o) = &organizer {
        v["organizer"] = json!({"email": o});
    }
    if !attendees.is_empty() {
        v["attendees"] = json!(attendees);
    }
    if let Some(link) = e.first("X-GOOGLE-CONFERENCE").map(|p| p.value.trim().to_string()) {
        if link.starts_with("http") {
            v["hangoutLink"] = json!(link);
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    const FEED: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\n\
BEGIN:VEVENT\r\nUID:one@google.com\r\nDTSTART:20261113T150000Z\r\nDTEND:20261113T153000Z\r\n\
SUMMARY:Pricing review\\, Q4\r\nLOCATION:https://zoom.us/j/123456789\r\n\
ORGANIZER;CN=Boss:mailto:boss@example.com\r\n\
ATTENDEE;CUTYPE=INDIVIDUAL;PARTSTAT=ACCEPTED;CN=Chris:mailto:chris@example.com\r\n\
ATTENDEE;PARTSTAT=NEEDS-ACTION;CN=Boss:mailto:boss@example.com\r\n\
ATTENDEE;CUTYPE=RESOURCE;PARTSTAT=ACCEPTED;CN=Room 4:mailto:room4@resource.example.com\r\n\
X-GOOGLE-CONFERENCE:https://meet.google.com/abc-defg-hij\r\n\
DESCRIPTION:Line one\\nLine two\r\n\
END:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:allday@google.com\r\nDTSTART;VALUE=DATE:20261113\r\nDTEND;VALUE=DATE:20261114\r\nSUMMARY:Holiday\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:cancelled@google.com\r\nSTATUS:CANCELLED\r\nDTSTART:20261113T160000Z\r\nDTEND:20261113T170000Z\r\nSUMMARY:Nope\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:declined@google.com\r\nDTSTART:20261113T180000Z\r\nDTEND:20261113T190000Z\r\nSUMMARY:Declined one\r\n\
ATTENDEE;PARTSTAT=DECLINED:mailto:chris@example.com\r\nEND:VEVENT\r\n\
END:VCALENDAR\r\n";

    fn day() -> (DateTime<Utc>, DateTime<Utc>) {
        (t("2026-11-13T00:00:00Z"), t("2026-11-14T00:00:00Z"))
    }

    #[test]
    fn parses_basic_event_into_google_shape() {
        let (a, b) = day();
        let evs = events_from_ics(FEED, Some("chris@example.com"), a, b);
        // all-day and cancelled dropped; declined kept here (parse_event drops it)
        assert_eq!(evs.len(), 2);
        let e = &evs[0];
        assert_eq!(e["id"], "one@google.com");
        assert_eq!(e["summary"], "Pricing review, Q4");
        assert_eq!(e["description"], "Line one\nLine two");
        assert_eq!(e["hangoutLink"], "https://meet.google.com/abc-defg-hij");
        assert_eq!(e["start"]["dateTime"], "2026-11-13T15:00:00Z");
        let atts = e["attendees"].as_array().unwrap();
        assert_eq!(atts.len(), 3);
        assert_eq!(atts[0]["self"], true);
        assert_eq!(atts[1]["organizer"], true);
        assert_eq!(atts[2]["resource"], true);
    }

    #[test]
    fn feeds_shared_parse_event_and_drops_declined() {
        let (a, b) = day();
        let evs = events_from_ics(FEED, Some("chris@example.com"), a, b);
        let parsed: Vec<_> = evs.iter().filter_map(crate::calendar::parse_event).collect();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].summary, "Pricing review, Q4");
        // hangoutLink wins over the zoom location text
        assert_eq!(parsed[0].video_url.as_deref(), Some("https://meet.google.com/abc-defg-hij"));
        assert_eq!(parsed[0].attendees.len(), 2, "resource room excluded");
    }

    #[test]
    fn window_filters_events() {
        let evs = events_from_ics(FEED, None, t("2026-11-14T00:00:00Z"), t("2026-11-15T00:00:00Z"));
        assert!(evs.is_empty());
    }

    #[test]
    fn folded_lines_and_tzid() {
        let feed = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:tz@x\r\n\
DTSTART;TZID=America/New_York:20261113T100000\r\nDTEND;TZID=America/New_York:20261113T110000\r\n\
SUMMARY:Long\r\n  title folded\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let evs = events_from_ics(feed, None, t("2026-11-13T00:00:00Z"), t("2026-11-14T00:00:00Z"));
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0]["summary"], "Long title folded");
        // 10:00 EST (UTC-5 in November) = 15:00Z
        assert_eq!(evs[0]["start"]["dateTime"], "2026-11-13T15:00:00Z");
    }

    #[test]
    fn weekly_recurrence_with_exdate_and_override() {
        let feed = "BEGIN:VCALENDAR\r\n\
BEGIN:VEVENT\r\nUID:weekly@x\r\nDTSTART:20261023T150000Z\r\nDTEND:20261023T160000Z\r\n\
RRULE:FREQ=WEEKLY;BYDAY=FR\r\nEXDATE:20261106T150000Z\r\nSUMMARY:Weekly sync\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:weekly@x\r\nRECURRENCE-ID:20261113T150000Z\r\nDTSTART:20261113T170000Z\r\nDTEND:20261113T180000Z\r\nSUMMARY:Weekly sync (moved)\r\nEND:VEVENT\r\n\
END:VCALENDAR\r\n";
        // Window over Oct 30 .. Nov 21: Oct30 yes, Nov6 excluded, Nov13 moved, Nov20 yes.
        let evs = events_from_ics(feed, None, t("2026-10-30T00:00:00Z"), t("2026-11-21T00:00:00Z"));
        let starts: Vec<String> = evs.iter().map(|e| e["start"]["dateTime"].as_str().unwrap().to_string()).collect();
        assert_eq!(
            starts,
            vec!["2026-10-30T15:00:00Z", "2026-11-13T17:00:00Z", "2026-11-20T15:00:00Z"]
        );
        assert_eq!(evs[1]["summary"], "Weekly sync (moved)");
        assert_eq!(evs[1]["id"], "weekly@x_20261113T150000Z");
        assert_eq!(evs[0]["id"], "weekly@x_20261030T150000Z");
    }

    #[test]
    fn normalizes_and_rejects_urls() {
        assert_eq!(normalize_url(" webcal://calendar.google.com/a.ics ").unwrap(), "https://calendar.google.com/a.ics");
        assert!(normalize_url("http://insecure.example.com/a.ics").is_err());
        assert!(normalize_url("hello").is_err());
    }

    #[test]
    fn owner_from_google_url() {
        assert_eq!(
            owner_email_from_url("https://calendar.google.com/calendar/ical/Jane.Doe%40example.com/private-abc123/basic.ics").as_deref(),
            Some("chris.serger@example.com")
        );
        assert_eq!(owner_email_from_url("https://example.com/feed.ics"), None);
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration("PT1H30M"), Some(Duration::minutes(90)));
        assert_eq!(parse_duration("P1D"), Some(Duration::days(1)));
        assert_eq!(parse_duration("junk"), None);
    }
}
