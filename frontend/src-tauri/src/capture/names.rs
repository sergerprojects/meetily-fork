//! While a recording runs: watch who the call app marks as speaking (Google Meet in
//! Chrome) and keep a timeline in `active_speakers.json` next to the recording. When
//! the meeting is saved, `apply` writes those names onto the transcript lines that
//! only say "Others" or "Speaker N".

use super::ax::{self, CallApp, Limits};
use super::speakers::{assign, collapse, voice_names, Line, Sample, Span};
use super::{clock, meet, perms};
use sqlx::{Row, SqlitePool};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Wry};

const FAST: Duration = Duration::from_millis(500);
const IDLE: Duration = Duration::from_secs(3);
const WRITE_EVERY: Duration = Duration::from_secs(10);

pub const FILE: &str = "active_speakers.json";

pub fn start(app: AppHandle<Wry>) {
    let _ = std::thread::Builder::new()
        .name("speaker-names".into())
        .spawn(move || tauri::async_runtime::block_on(run(app)));
}

async fn run(app: AppHandle<Wry>) {
    let mut samples: Vec<Sample> = Vec::new();
    let mut folder: Option<PathBuf> = None;
    let mut last_write = Instant::now();
    let mut dirty = false;
    let mut was_recording = false;
    let mut delay = IDLE;
    loop {
        tokio::time::sleep(delay).await;
        delay = IDLE;
        let recording = crate::audio::recording_commands::is_recording().await;
        if !recording {
            if dirty {
                if let Some(f) = &folder {
                    write(f, &samples);
                }
                dirty = false;
            }
            was_recording = false;
            continue;
        }
        if !was_recording {
            was_recording = true;
            samples.clear();
            folder = None;
            dirty = false;
        }
        if crate::audio::recording_commands::is_recording_paused().await
            || !super::names_enabled(&app)
            || !perms::accessibility_granted()
        {
            continue;
        }
        if folder.is_none() {
            folder = crate::audio::recording_commands::get_meeting_folder_path()
                .await
                .ok()
                .flatten()
                .map(PathBuf::from);
        }
        let Some(dir) = folder.clone() else { continue };
        let Some(pid) = ax::find_pid(CallApp::Chrome) else { continue };
        let windows = ax::snapshot_windows(pid, true, &Limits::poll());
        let Some(speaking) = windows.iter().find(|w| meet::meet_page(w).is_some()).map(meet::speaking) else {
            continue;
        };
        delay = FAST;
        if let Some(t) = clock::stream_now() {
            samples.push(Sample { t, speaking });
            dirty = true;
        }
        if dirty && last_write.elapsed() >= WRITE_EVERY {
            write(&dir, &samples);
            last_write = Instant::now();
            dirty = false;
        }
    }
}

fn write(dir: &Path, samples: &[Sample]) {
    let spans = collapse(samples);
    if spans.is_empty() {
        return;
    }
    match serde_json::to_vec_pretty(&spans) {
        Ok(json) => {
            let tmp = dir.join(".active_speakers.tmp");
            if std::fs::write(&tmp, json).is_ok() {
                let _ = std::fs::rename(&tmp, dir.join(FILE));
            }
        }
        Err(e) => log::warn!("speaker names: {e}"),
    }
}

pub fn read_spans(dir: &Path) -> Vec<Span> {
    std::fs::read_to_string(dir.join(FILE))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Put the call app's names on the saved transcript. Never overwrites "You" or a name
/// a person already chose. Returns how many lines were renamed.
pub async fn apply(pool: &SqlitePool, meeting_id: &str, folder: &Path) -> usize {
    let spans = read_spans(folder);
    if spans.is_empty() {
        return 0;
    }
    let rows = match sqlx::query(
        "SELECT id, audio_start_time, audio_end_time, speaker FROM transcripts WHERE meeting_id = ? AND audio_start_time IS NOT NULL",
    )
    .bind(meeting_id)
    .fetch_all(pool)
    .await
    {
        Ok(r) => r,
        Err(e) => {
            log::warn!("speaker names: {e}");
            return 0;
        }
    };
    let lines: Vec<Line> = rows
        .iter()
        .map(|r| {
            let start: f64 = r.get("audio_start_time");
            let end: Option<f64> = r.get("audio_end_time");
            Line {
                id: r.get::<String, _>("id"),
                start,
                end: end.unwrap_or(start + 2.0),
                speaker: r.get::<Option<String>, _>("speaker").unwrap_or_default(),
            }
        })
        .collect();
    let named = assign(&lines, &spans, &[]);
    let mut changed = 0;
    for (id, name) in &named {
        if sqlx::query("UPDATE transcripts SET speaker = ? WHERE id = ? AND meeting_id = ?")
            .bind(name)
            .bind(id)
            .bind(meeting_id)
            .execute(pool)
            .await
            .is_ok()
        {
            changed += 1;
        }
    }
    // A voice whose lines mostly agree on one name gets that name on its other lines too.
    for (label, name) in voice_names(&lines, &named, 0.7) {
        let _ = sqlx::query("UPDATE transcripts SET speaker = ? WHERE meeting_id = ? AND speaker = ?")
            .bind(&name)
            .bind(meeting_id)
            .bind(&label)
            .execute(pool)
            .await;
    }
    changed
}
