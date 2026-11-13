//! Minimal read-only Google Calendar client.
//!
//! Auth reuses gcloud Application Default Credentials (`authorized_user`
//! type) from `~/.config/gcloud/application_default_credentials.json`.
//! Tokens and the client secret are never logged or included in errors.

use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const EVENTS_URL: &str = "https://www.googleapis.com/calendar/v3/calendars/primary/events";
/// Refresh the access token this long before Google says it expires.
const TOKEN_EXPIRY_MARGIN: Duration = Duration::from_secs(60);
/// Safety cap on pagination.
const MAX_PAGES: usize = 10;

#[derive(Deserialize)]
struct AdcFile {
    #[serde(rename = "type")]
    kind: Option<String>,
    client_id: Option<String>,
    client_secret: Option<String>,
    refresh_token: Option<String>,
    quota_project_id: Option<String>,
}

/// Credentials loaded from the ADC file. Debug is redacted on purpose.
pub struct AdcCredentials {
    client_id: String,
    client_secret: String,
    refresh_token: String,
    quota_project_id: String,
}

impl std::fmt::Debug for AdcCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdcCredentials")
            .field("quota_project_id", &self.quota_project_id)
            .finish_non_exhaustive()
    }
}

pub fn adc_path() -> Option<PathBuf> {
    const FILE: &str = "application_default_credentials.json";
    if let Some(dir) = std::env::var_os("CLOUDSDK_CONFIG") {
        return Some(PathBuf::from(dir).join(FILE));
    }
    #[cfg(windows)]
    if let Some(appdata) = std::env::var_os("APPDATA") {
        return Some(PathBuf::from(appdata).join("gcloud").join(FILE));
    }
    dirs::home_dir().map(|h| h.join(".config/gcloud").join(FILE))
}

impl AdcCredentials {
    /// Load and validate the ADC file. Errors never contain secret values.
    pub fn load() -> Result<Self> {
        let path = adc_path().ok_or_else(|| anyhow!("home directory not found"))?;
        if !path.exists() {
            bail!("Google credentials not found at {}", path.display());
        }
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        Self::from_json(&raw)
    }

    pub fn from_json(raw: &str) -> Result<Self> {
        let file: AdcFile =
            serde_json::from_str(raw).map_err(|_| anyhow!("credentials file is not valid JSON"))?;
        if file.kind.as_deref() != Some("authorized_user") {
            bail!("credentials type must be authorized_user");
        }
        let need = |v: Option<String>, name: &str| -> Result<String> {
            v.filter(|s| !s.trim().is_empty())
                .ok_or_else(|| anyhow!("credentials missing {}", name))
        };
        Ok(Self {
            client_id: need(file.client_id, "client_id")?,
            client_secret: need(file.client_secret, "client_secret")?,
            refresh_token: need(file.refresh_token, "refresh_token")?,
            quota_project_id: need(file.quota_project_id, "quota_project_id")?,
        })
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: Option<u64>,
}

pub struct GoogleCalendarClient {
    creds: AdcCredentials,
    http: reqwest::Client,
    token: Mutex<Option<(String, Instant)>>,
}

impl GoogleCalendarClient {
    pub fn new(creds: AdcCredentials) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            creds,
            http,
            token: Mutex::new(None),
        }
    }

    async fn access_token(&self) -> Result<String> {
        let mut guard = self.token.lock().await;
        if let Some((token, valid_until)) = guard.as_ref() {
            if Instant::now() < *valid_until {
                return Ok(token.clone());
            }
        }

        let resp = self
            .http
            .post(TOKEN_URL)
            .form(&[
                ("grant_type", "refresh_token"),
                ("client_id", self.creds.client_id.as_str()),
                ("client_secret", self.creds.client_secret.as_str()),
                ("refresh_token", self.creds.refresh_token.as_str()),
            ])
            .send()
            .await
            .map_err(|e| anyhow!("token request failed: {}", e.without_url()))?;

        let status = resp.status();
        if !status.is_success() {
            // Only surface Google's error code, never the request/response body.
            let code = resp
                .json::<Value>()
                .await
                .ok()
                .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(String::from))
                .unwrap_or_default();
            bail!("token refresh failed: HTTP {} {}", status.as_u16(), code);
        }

        let body: TokenResponse = resp
            .json()
            .await
            .map_err(|_| anyhow!("token response was not valid JSON"))?;
        let ttl = Duration::from_secs(body.expires_in.unwrap_or(3600));
        let valid_until = Instant::now() + ttl.saturating_sub(TOKEN_EXPIRY_MARGIN);
        *guard = Some((body.access_token.clone(), valid_until));
        Ok(body.access_token)
    }

    /// Display name for an email from the Google Workspace directory.
    /// Ok(None) = not found; Err = missing scope / API disabled / network.
    pub async fn directory_name(&self, email: &str) -> Result<Option<String>> {
        let token = self.access_token().await?;
        let resp = self
            .http
            .get("https://people.googleapis.com/v1/people:searchDirectoryPeople")
            .bearer_auth(&token)
            .header("x-goog-user-project", &self.creds.quota_project_id)
            .query(&[
                ("query", email),
                ("readMask", "names,emailAddresses"),
                ("sources", "DIRECTORY_SOURCE_TYPE_DOMAIN_PROFILE"),
                ("pageSize", "5"),
            ])
            .send()
            .await
            .map_err(|e| anyhow!("directory request failed: {}", e.without_url()))?;
        let status = resp.status();
        if !status.is_success() {
            bail!("directory lookup failed: HTTP {}", status.as_u16());
        }
        let body: Value = resp.json().await.map_err(|_| anyhow!("directory response was not valid JSON"))?;
        let wanted = email.trim().to_lowercase();
        Ok(body
            .get("people")
            .and_then(|p| p.as_array())
            .into_iter()
            .flatten()
            .find(|p| {
                p.get("emailAddresses")
                    .and_then(|e| e.as_array())
                    .map_or(false, |a| a.iter().any(|x| x.get("value").and_then(|v| v.as_str()).map(|v| v.to_lowercase()) == Some(wanted.clone())))
            })
            .and_then(|p| p.pointer("/names/0/displayName").and_then(|v| v.as_str()).map(String::from)))
    }

    /// List raw event JSON objects on the primary calendar overlapping
    /// `[time_min, time_max)`. Read-only GET.
    pub async fn list_events(
        &self,
        time_min: DateTime<Utc>,
        time_max: DateTime<Utc>,
    ) -> Result<Vec<Value>> {
        let token = self.access_token().await?;
        let mut items = Vec::new();
        let mut page_token: Option<String> = None;

        for _ in 0..MAX_PAGES {
            let mut query: Vec<(&str, String)> = vec![
                ("singleEvents", "true".to_string()),
                ("orderBy", "startTime".to_string()),
                ("timeMin", time_min.to_rfc3339()),
                ("timeMax", time_max.to_rfc3339()),
                ("maxResults", "250".to_string()),
            ];
            if let Some(pt) = &page_token {
                query.push(("pageToken", pt.clone()));
            }

            let resp = self
                .http
                .get(EVENTS_URL)
                .bearer_auth(&token)
                .header("x-goog-user-project", &self.creds.quota_project_id)
                .query(&query)
                .send()
                .await
                .map_err(|e| anyhow!("events request failed: {}", e.without_url()))?;

            let status = resp.status();
            if status.as_u16() == 401 {
                // Force a refresh on the next call.
                *self.token.lock().await = None;
            }
            if !status.is_success() {
                let msg = resp
                    .json::<Value>()
                    .await
                    .ok()
                    .and_then(|v| {
                        v.pointer("/error/message")
                            .and_then(|m| m.as_str())
                            .map(String::from)
                    })
                    .unwrap_or_default();
                bail!("events list failed: HTTP {} {}", status.as_u16(), msg);
            }

            let body: Value = resp
                .json()
                .await
                .map_err(|_| anyhow!("events response was not valid JSON"))?;
            if let Some(arr) = body.get("items").and_then(|v| v.as_array()) {
                items.extend(arr.iter().cloned());
            }
            page_token = body
                .get("nextPageToken")
                .and_then(|v| v.as_str())
                .map(String::from);
            if page_token.is_none() {
                break;
            }
        }

        Ok(items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adc_parses_authorized_user() {
        let c = AdcCredentials::from_json(
            r#"{"type":"authorized_user","client_id":"id","client_secret":"s","refresh_token":"r","quota_project_id":"p"}"#,
        )
        .unwrap();
        assert_eq!(c.quota_project_id, "p");
        let dbg = format!("{:?}", c);
        assert!(!dbg.contains("\"s\"") && !dbg.contains("\"r\""));
    }

    #[test]
    fn adc_rejects_missing_fields_and_wrong_type() {
        assert!(AdcCredentials::from_json(r#"{"type":"service_account"}"#).is_err());
        assert!(AdcCredentials::from_json(
            r#"{"type":"authorized_user","client_id":"id","client_secret":"s","refresh_token":"r"}"#
        )
        .is_err());
        assert!(AdcCredentials::from_json("not json").is_err());
    }
}
