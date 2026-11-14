//! Real names for attendees whose calendar entry has only an email address.
//!
//! Sources, in order:
//! 1. `~/.graphite/people.json` — `{"email": "Full Name"}`; you can edit it by hand and
//!    it is also where looked-up names are cached.
//! 2. Google Workspace directory (People API `searchDirectoryPeople`), when the
//!    gcloud credentials include the `directory.readonly` scope. One lookup per
//!    unknown email, cached on success; a failed lookup is not retried this session.
//!
//! Names are never sent anywhere except to Google's own API for the lookup.

use once_cell::sync::Lazy;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::RwLock;

static CACHE: Lazy<RwLock<HashMap<String, String>>> = Lazy::new(|| RwLock::new(load_file()));
static TRIED: Lazy<RwLock<HashSet<String>>> = Lazy::new(|| RwLock::new(HashSet::new()));

pub fn people_path() -> Option<PathBuf> {
    std::env::var_os("GRAPHITE_PEOPLE_FILE")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".graphite").join("people.json")))
}

fn load_file() -> HashMap<String, String> {
    let Some(path) = people_path() else { return HashMap::new() };
    let Ok(raw) = std::fs::read_to_string(&path) else { return HashMap::new() };
    serde_json::from_str::<HashMap<String, String>>(&raw)
        .map(|m| m.into_iter().map(|(k, v)| (k.trim().to_lowercase(), v.trim().to_string())).collect())
        .unwrap_or_default()
}

/// Re-read the file (picks up hand edits).
pub fn reload() {
    if let Ok(mut c) = CACHE.write() {
        *c = load_file();
    }
}

/// Known real name for this email, if any.
pub fn lookup(email: &str) -> Option<String> {
    CACHE
        .read()
        .ok()?
        .get(&email.trim().to_lowercase())
        .filter(|n| !n.is_empty())
        .cloned()
}

/// True when `name` is just the email's local part (no real name from Google).
pub fn is_handle(name: &str, email: &str) -> bool {
    let local = email.split('@').next().unwrap_or(email);
    name.eq_ignore_ascii_case(local) || !name.contains(' ')
}

/// Emails not yet resolved and not tried this session.
pub fn unresolved<'a>(pairs: impl Iterator<Item = (&'a str, &'a str)>) -> Vec<String> {
    let tried = TRIED.read().map(|t| t.clone()).unwrap_or_default();
    pairs
        .filter(|(name, email)| {
            is_handle(name, email) && lookup(email).is_none() && !tried.contains(&email.to_lowercase())
        })
        .map(|(_, email)| email.to_lowercase())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect()
}

pub fn mark_tried(email: &str) {
    if let Ok(mut t) = TRIED.write() {
        t.insert(email.to_lowercase());
    }
}

/// Remember a resolved name (memory + file).
pub fn remember(email: &str, name: &str) {
    if let Ok(mut c) = CACHE.write() {
        c.insert(email.trim().to_lowercase(), name.trim().to_string());
        if let Some(path) = people_path() {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let tmp = path.with_extension("json.tmp");
            if let Ok(json) = serde_json::to_string_pretty(&*c) {
                if std::fs::write(&tmp, json).is_ok() {
                    let _ = std::fs::rename(&tmp, &path);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_vs_real_names() {
        assert!(is_handle("akleymann", "akleymann@example.com"));
        assert!(is_handle("Amelia", "akleymann@example.com"), "single word is still not a full name");
        assert!(!is_handle("Amelia Ochodnicky", "akleymann@example.com"));
    }

    #[test]
    fn unresolved_skips_real_names_and_tried() {
        mark_tried("tried@example.com");
        let pairs = vec![
            ("Real Person", "real@example.com"),
            ("handle1", "handle1@example.com"),
            ("tried", "tried@example.com"),
        ];
        let out = unresolved(pairs.into_iter());
        assert_eq!(out, vec!["handle1@example.com".to_string()]);
    }
}
