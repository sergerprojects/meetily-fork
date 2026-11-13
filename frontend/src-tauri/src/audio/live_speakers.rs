//! Live "who is speaking" for the call side ("Others"): tells voices apart as people talk.
//!
//! A small Python helper (`tools/live_speakers.py`, embedded in this binary) runs next to
//! the app for the length of a recording. Each transcribed stretch of call audio is sent to
//! it in memory and it answers with a speaker number; the same voice keeps its number all
//! meeting. Nothing is written to disk and nothing leaves the Mac.
//!
//! Optional: it needs Python with `sherpa-onnx` and the voice model (see
//! scripts/setup-speakers.sh). If either is missing the feature stays off and call audio
//! is simply labelled "Others", as before.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;

const SCRIPT: &str = include_str!("../../../../tools/live_speakers.py");
const READY_TIMEOUT: Duration = Duration::from_secs(40);
const REPLY_TIMEOUT: Duration = Duration::from_secs(4);
/// Fewer samples than this (at 16 kHz about 0.6 s) is not worth sending.
const MIN_SAMPLES: usize = 9_600;

static ENABLED: AtomicBool = AtomicBool::new(false);
static HELPER: Mutex<Option<Helper>> = Mutex::const_new(None);

struct Helper {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

fn meetily_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".meetily"))
}

fn python() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("MEETILY_PYTHON").map(PathBuf::from).filter(|p| p.exists()) {
        return Some(p);
    }
    let p = meetily_dir()?.join("venv/bin/python");
    p.exists().then_some(p)
}

/// Start the helper for a new recording. Quietly leaves the feature off if it cannot run.
pub async fn start() {
    stop().await;
    let (Some(py), Some(dir)) = (python(), meetily_dir()) else {
        log::info!("Live speakers: off (no ~/.meetily/venv; run scripts/setup-speakers.sh to enable)");
        return;
    };
    let script = dir.join("live_speakers.py");
    if std::fs::write(&script, SCRIPT).is_err() {
        return;
    }
    let spawned = Command::new(&py)
        .arg(&script)
        .env_clear()
        .env("HOME", dirs::home_dir().unwrap_or_default())
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn();
    let Ok(mut child) = spawned else { return };
    let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else { return };
    let mut stdout = BufReader::new(stdout);
    let mut line = String::new();
    let ready = tokio::time::timeout(READY_TIMEOUT, stdout.read_line(&mut line)).await;
    if !matches!(ready, Ok(Ok(n)) if n > 0) || !line.contains("\"ready\"") {
        log::info!("Live speakers: helper did not start ({})", line.trim());
        let _ = child.kill().await;
        return;
    }
    *HELPER.lock().await = Some(Helper { child, stdin, stdout });
    ENABLED.store(true, Ordering::Relaxed);
    log::info!("Live speakers: on");
}

pub async fn stop() {
    ENABLED.store(false, Ordering::Relaxed);
    if let Some(mut h) = HELPER.lock().await.take() {
        drop(h.stdin);
        let _ = h.child.kill().await;
    }
}

/// "Speaker N" for this stretch of call audio, or None when unsure / off.
pub async fn label(samples: &[f32], sample_rate: u32) -> Option<String> {
    if !enabled() || samples.len() < MIN_SAMPLES * sample_rate as usize / 16_000 {
        return None;
    }
    let mut guard = HELPER.lock().await;
    let h = guard.as_mut()?;
    let mut frame = Vec::with_capacity(8 + samples.len() * 4);
    frame.extend_from_slice(&(samples.len() as u32).to_le_bytes());
    frame.extend_from_slice(&sample_rate.to_le_bytes());
    for s in samples {
        frame.extend_from_slice(&s.to_le_bytes());
    }
    let result = tokio::time::timeout(REPLY_TIMEOUT, async {
        h.stdin.write_all(&frame).await.ok()?;
        h.stdin.flush().await.ok()?;
        let mut line = String::new();
        h.stdout.read_line(&mut line).await.ok()?;
        parse_reply(&line)
    })
    .await;
    match result {
        Ok(Some(n)) => n.map(|n| format!("Speaker {}", n + 1)),
        _ => {
            // Helper died or stalled: stop using it for this recording.
            ENABLED.store(false, Ordering::Relaxed);
            None
        }
    }
}

/// `{"speaker": 2, ...}` -> Some(Some(2)); `{"speaker": -1}` -> Some(None); garbage -> None.
fn parse_reply(line: &str) -> Option<Option<usize>> {
    let v: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    let n = v.get("speaker")?.as_i64()?;
    Some((n >= 0).then_some(n as usize))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_helper_replies() {
        assert_eq!(parse_reply("{\"speaker\": 2, \"sim\": 0.9}\n"), Some(Some(2)));
        assert_eq!(parse_reply("{\"speaker\": -1, \"sim\": 0.1}"), Some(None));
        assert_eq!(parse_reply("not json"), None);
        assert_eq!(parse_reply("{\"error\": \"x\"}"), None);
    }
}
