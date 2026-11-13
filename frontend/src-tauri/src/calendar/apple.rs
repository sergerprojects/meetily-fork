//! Calendar source: the Mac's own Calendar database (EventKit) through the bundled
//! `meetily-eventkit` helper. Any account added to the Calendar app works (Google,
//! Exchange, iCloud), with no Google Cloud project and no OAuth client. macOS asks
//! for Calendar access once, in Meetily's name.

use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

const TIMEOUT: Duration = Duration::from_secs(25);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(130);

pub struct AppleClient {
    helper: PathBuf,
}

/// Where the helper lives: next to the app binary in a bundle, or MEETILY_EVENTKIT for dev.
pub fn helper_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("MEETILY_EVENTKIT").map(PathBuf::from).filter(|p| p.exists()) {
        return Some(p);
    }
    let exe = std::env::current_exe().ok()?;
    let p = exe.parent()?.join("meetily-eventkit");
    p.exists().then_some(p)
}

impl AppleClient {
    pub fn new() -> Option<Self> {
        helper_path().map(|helper| Self { helper })
    }

    async fn run(&self, args: &[&str], timeout: Duration) -> Result<(i32, String)> {
        let child = Command::new(&self.helper)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| anyhow!("could not start the Calendar helper: {}", e))?;
        let out = tokio::time::timeout(timeout, child.wait_with_output())
            .await
            .map_err(|_| anyhow!("the Calendar helper timed out"))??;
        Ok((out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stdout).trim().to_string()))
    }

    /// "granted" | "undetermined" | "denied"
    pub async fn status(&self) -> String {
        self.run(&["--status"], TIMEOUT).await.map(|(_, s)| s).unwrap_or_else(|_| "denied".to_string())
    }

    /// Show the macOS permission prompt if needed. True when access is granted.
    pub async fn request(&self) -> bool {
        matches!(self.run(&["--request"], REQUEST_TIMEOUT).await, Ok((_, s)) if s == "granted")
    }

    pub async fn list_events(&self, min: DateTime<Utc>, max: DateTime<Utc>) -> Result<Vec<Value>> {
        let (code, out) = self.run(&["--events", &min.to_rfc3339(), &max.to_rfc3339()], TIMEOUT).await?;
        if code == 3 {
            bail!("Calendar access is off for Meetily: allow it in System Settings, Privacy & Security, Calendars");
        }
        if code != 0 {
            bail!("the Calendar helper failed (code {})", code);
        }
        serde_json::from_str::<Vec<Value>>(&out).map_err(|_| anyhow!("the Calendar helper returned unreadable data"))
    }
}
