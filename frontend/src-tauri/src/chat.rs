//! Chat with the meeting while it is happening ("what did I miss?", "suggest a
//! question"). Questions go to the user's own Claude Code CLI (`claude -p`),
//! signed in as them: no API key lives in this app and nothing is sent anywhere
//! the user has not already approved for Claude. The CLI runs with every tool
//! disabled, in a scratch directory, with a minimal environment.

use serde::Deserialize;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

const MAX_TRANSCRIPT_CHARS: usize = 60_000;
const TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Deserialize)]
pub struct ChatTurn {
    pub role: String,
    pub text: String,
}

fn home() -> Option<PathBuf> {
    dirs::home_dir()
}

fn find_claude() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("CLAUDE_BIN").map(PathBuf::from).filter(|p| p.exists()) {
        return Some(p);
    }
    let h = home()?;
    let candidates = [
        h.join(".local/bin/claude"),
        h.join(".claude/local/claude"),
        PathBuf::from("/opt/homebrew/bin/claude"),
        PathBuf::from("/usr/local/bin/claude"),
    ];
    if let Some(p) = candidates.iter().find(|p| p.exists()) {
        return Some(p.clone());
    }
    // Last resort: ask a login shell (finds nvm/volta/custom installs).
    let out = std::process::Command::new("/bin/zsh")
        .args(["-lc", "command -v claude"])
        .output()
        .ok()?;
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!path.is_empty() && PathBuf::from(&path).exists()).then(|| PathBuf::from(path))
}

/// Keep the most recent part of a long transcript (char-boundary safe).
fn tail_chars(s: &str, max: usize) -> &str {
    if s.chars().count() <= max {
        return s;
    }
    let skip = s.chars().count() - max;
    let idx = s.char_indices().nth(skip).map(|(i, _)| i).unwrap_or(0);
    &s[idx..]
}

pub fn build_prompt(
    question: &str,
    transcript: &str,
    notes: &str,
    title: &str,
    attendees: &[String],
    history: &[ChatTurn],
) -> String {
    let mut p = String::new();
    p.push_str(
        "You are a discreet assistant sitting in on a live meeting with the user. \
They glance at your answer mid-conversation, so be fast and brief: lead with the answer, \
use short bullets, no preamble, no restating the question. Use only what the transcript, notes \
and invite say; if something is not there, say so. Lines starting \"You:\" are the user speaking. \
The transcript is machine-made and may contain mishearings.\n\n",
    );
    if !title.trim().is_empty() {
        p.push_str(&format!("Meeting: {}\n", title.trim()));
    }
    if !attendees.is_empty() {
        p.push_str(&format!("Invited: {}\n", attendees.join(", ")));
    }
    if !notes.trim().is_empty() {
        p.push_str(&format!("\nThe user's own notes so far:\n{}\n", notes.trim()));
    }
    p.push_str(&format!(
        "\nTranscript so far (latest at the bottom):\n{}\n",
        tail_chars(transcript, MAX_TRANSCRIPT_CHARS).trim()
    ));
    if !history.is_empty() {
        p.push_str("\nEarlier in this chat:\n");
        for t in history.iter().rev().take(8).rev() {
            let who = if t.role == "user" { "User" } else { "You" };
            p.push_str(&format!("{}: {}\n", who, t.text.trim()));
        }
    }
    p.push_str(&format!("\nUser asks: {}\n", question.trim()));
    p
}

#[tauri::command]
pub async fn meeting_chat(
    question: String,
    transcript: String,
    notes: String,
    title: String,
    attendees: Vec<String>,
    history: Vec<ChatTurn>,
    model: Option<String>,
) -> Result<String, String> {
    if question.trim().is_empty() {
        return Err("Type a question first.".to_string());
    }
    let claude = find_claude().ok_or_else(|| {
        "Claude Code isn't installed on this Mac. Install it from claude.com/claude-code, sign in once in Terminal, then try again."
            .to_string()
    })?;
    let model = match model.as_deref() {
        Some("haiku") => "haiku",
        Some("opus") => "opus",
        _ => "sonnet",
    };
    let prompt = build_prompt(&question, &transcript, &notes, &title, &attendees, &history);

    let h = home().unwrap_or_default();
    let path = format!(
        "{}:{}:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin",
        h.join(".local/bin").display(),
        h.join(".claude/local").display()
    );
    let mut cmd = Command::new(&claude);
    cmd.args(["-p", "--setting-sources", "user", "--tools", "", "--model", model])
        .env_clear()
        .env("HOME", &h)
        .env("PATH", path)
        .env("USER", std::env::var("USER").unwrap_or_default())
        .env("LANG", "en_US.UTF-8")
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = cmd.spawn().map_err(|e| format!("Could not start Claude Code: {}", e))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(prompt.as_bytes()).await.map_err(|e| e.to_string())?;
    }
    let out = tokio::time::timeout(TIMEOUT, child.wait_with_output())
        .await
        .map_err(|_| "Claude took too long to answer. Try a shorter question.".to_string())?
        .map_err(|e| e.to_string())?;

    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if text.contains("Failed to authenticate") || text.contains("Invalid API key") {
        return Err("Claude Code isn't signed in. Open Terminal, run `claude` once to sign in, then try again.".to_string());
    }
    if !out.status.success() || text.is_empty() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("Claude didn't answer. {}", err.chars().take(200).collect::<String>()));
    }
    Ok(text)
}

/// Open an https link in the default browser (meeting links, calendar event).
#[tauri::command]
pub async fn open_external(url: String) -> Result<(), String> {
    if !url.starts_with("https://") || url.contains(char::is_whitespace) {
        return Err("Only https links can be opened.".to_string());
    }
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(&url).spawn();
    #[cfg(target_os = "windows")]
    let r = std::process::Command::new("cmd").args(["/C", "start", "", &url]).spawn();
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    let r = std::process::Command::new("xdg-open").arg(&url).spawn();
    r.map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_has_context_in_order_and_latest_question_last() {
        let p = build_prompt(
            "What did I miss?",
            "[00:01] You: hi\n[00:05] Susie: budget is flat",
            "ask about headcount",
            "Susie / Chris",
            &["Sam Rivera".to_string()],
            &[ChatTurn { role: "user".into(), text: "earlier q".into() }, ChatTurn { role: "assistant".into(), text: "earlier a".into() }],
        );
        assert!(p.contains("Meeting: Susie / Chris"));
        assert!(p.contains("Invited: Sam Rivera"));
        assert!(p.contains("ask about headcount"));
        assert!(p.find("budget is flat").unwrap() < p.find("earlier q").unwrap());
        assert!(p.trim_end().ends_with("User asks: What did I miss?"));
    }

    #[test]
    fn long_transcripts_keep_the_latest_part() {
        let long = format!("{}END", "x".repeat(MAX_TRANSCRIPT_CHARS + 500));
        let tail = tail_chars(&long, MAX_TRANSCRIPT_CHARS);
        assert_eq!(tail.chars().count(), MAX_TRANSCRIPT_CHARS);
        assert!(tail.ends_with("END"));
        let multibyte = "é".repeat(10);
        assert_eq!(tail_chars(&multibyte, 4).chars().count(), 4);
    }

    #[test]
    fn open_external_rejects_non_https() {
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        assert!(rt.block_on(open_external("file:///etc/passwd".into())).is_err());
        assert!(rt.block_on(open_external("https://a b".into())).is_err());
    }
}
