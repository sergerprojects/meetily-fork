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
    let model = match model.as_deref() {
        Some("haiku") => "haiku",
        Some("opus") => "opus",
        _ => "sonnet",
    };
    let prompt = build_prompt(&question, &transcript, &notes, &title, &attendees, &history);

    run_claude(&prompt, model, &[], TIMEOUT).await
}

/// Run the user's Claude Code CLI headlessly with a prompt on stdin. With no
/// `allowed_tools` every tool is disabled; otherwise only those (e.g. a Google
/// Calendar connector tool) may run. Minimal environment, scratch directory.
pub async fn run_claude(
    prompt: &str,
    model: &str,
    allowed_tools: &[&str],
    timeout: Duration,
) -> Result<String, String> {
    let claude = find_claude().ok_or_else(|| {
        "Claude Code isn't installed on this Mac. Install it from claude.com/claude-code, sign in once in Terminal, then try again."
            .to_string()
    })?;
    let h = home().unwrap_or_default();
    let path = format!(
        "{}:{}:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin",
        h.join(".local/bin").display(),
        h.join(".claude/local").display()
    );
    let mut cmd = Command::new(&claude);
    cmd.args(["-p", "--setting-sources", "user", "--model", model]);
    if allowed_tools.is_empty() {
        cmd.args(["--tools", ""]);
    } else {
        cmd.args(["--allowedTools", &allowed_tools.join(",")]);
    }
    cmd.env_clear()
        .env("HOME", &h)
        .env("PATH", path)
        .env("USER", std::env::var("USER").unwrap_or_default())
        .env("LOGNAME", std::env::var("LOGNAME").or_else(|_| std::env::var("USER")).unwrap_or_default())
        // Claude Code finds its saved connector sign-ins through these; without them
        // a headless call reports "needs authorization" even when signed in.
        .env("TMPDIR", std::env::temp_dir())
        .env("SHELL", "/bin/zsh")
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
    let out = tokio::time::timeout(timeout, child.wait_with_output())
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


/// Result of an MCP tool the model called, lifted verbatim from Claude Code's
/// stream-json output so the model never rewrites the data.
#[derive(Debug, PartialEq)]
pub struct ToolResult {
    pub tool: String,
    pub text: String,
    pub is_error: bool,
}

/// Parse `--output-format stream-json` lines into the MCP tool results they contain.
pub fn lift_tool_results(stdout: &str) -> Vec<ToolResult> {
    use serde_json::Value;
    let mut uses: Vec<(String, String)> = Vec::new(); // (tool_use_id, tool name)
    let mut results: Vec<(String, String, bool)> = Vec::new(); // (tool_use_id, text, is_error)
    for line in stdout.lines() {
        let Ok(ev) = serde_json::from_str::<Value>(line) else { continue };
        let Some(content) = ev.pointer("/message/content").and_then(|c| c.as_array()) else { continue };
        for b in content {
            match b.get("type").and_then(|t| t.as_str()) {
                Some("tool_use") => {
                    let name = b.get("name").and_then(|n| n.as_str()).unwrap_or("");
                    if name.starts_with("mcp__") {
                        if let Some(id) = b.get("id").and_then(|i| i.as_str()) {
                            uses.push((id.to_string(), name.to_string()));
                        }
                    }
                }
                Some("tool_result") => {
                    let id = b.get("tool_use_id").and_then(|i| i.as_str()).unwrap_or("").to_string();
                    let text = match b.get("content") {
                        Some(Value::String(s)) => s.clone(),
                        Some(Value::Array(a)) => a
                            .iter()
                            .filter_map(|x| x.get("text").and_then(|t| t.as_str()))
                            .collect::<Vec<_>>()
                            .join(""),
                        Some(other) => other.to_string(),
                        None => String::new(),
                    };
                    results.push((id, text, b.get("is_error").and_then(|e| e.as_bool()).unwrap_or(false)));
                }
                _ => {}
            }
        }
    }
    uses.into_iter()
        .filter_map(|(id, tool)| {
            results
                .iter()
                .find(|(rid, _, _)| *rid == id)
                .map(|(_, text, is_error)| ToolResult { tool, text: unpersist(text), is_error: *is_error })
        })
        .collect()
}

/// Claude Code swaps very large tool results for "<persisted-output> ... saved to: <file>".
fn unpersist(text: &str) -> String {
    let big = text.trim_start().starts_with("<persisted-output>") || text.contains("exceeds maximum");
    if !big {
        return text.to_string();
    }
    let re = regex::Regex::new(r"saved to:?\s*(/\S+?\.(?:txt|json))").unwrap();
    let Some(path) = re.captures(text).and_then(|c| c.get(1)).map(|m| m.as_str().to_string()) else {
        return text.to_string();
    };
    let Ok(body) = std::fs::read_to_string(&path) else { return text.to_string() };
    // The file may wrap the text as [{"type":"text","text":"..."}].
    if let Ok(serde_json::Value::Array(a)) = serde_json::from_str::<serde_json::Value>(&body) {
        if a.first().and_then(|x| x.get("type")).and_then(|t| t.as_str()) == Some("text") {
            return a.iter().filter_map(|x| x.get("text").and_then(|t| t.as_str())).collect::<Vec<_>>().join("");
        }
    }
    body
}

/// Ask Claude Code to make exactly one MCP tool call and return that call's raw result.
pub async fn call_mcp_tool(tool: &str, args: &serde_json::Value, timeout: Duration) -> Result<String, String> {
    let prompt = format!(
        "Make exactly this MCP tool call, once, with the arguments given verbatim. Do not call any other tool. Do not retry. When done reply with the single word DONE.\n1. {} with arguments {}",
        tool, args
    );
    let claude = find_claude().ok_or_else(|| {
        "Claude Code isn't installed on this Mac. Install it from claude.com/claude-code, sign in once in Terminal, then try again.".to_string()
    })?;
    let h = home().unwrap_or_default();
    let path = format!("{}:{}:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin", h.join(".local/bin").display(), h.join(".claude/local").display());
    let mut child = Command::new(&claude)
        .args(["-p", "--model", "haiku", "--setting-sources", "user", "--allowedTools", tool, "--output-format", "stream-json", "--verbose", "--max-turns", "6"])
        .env_clear()
        .env("HOME", &h)
        .env("PATH", path)
        .env("USER", std::env::var("USER").unwrap_or_default())
        .env("LOGNAME", std::env::var("LOGNAME").or_else(|_| std::env::var("USER")).unwrap_or_default())
        .env("TMPDIR", std::env::temp_dir())
        .env("SHELL", "/bin/zsh")
        .env("LANG", "en_US.UTF-8")
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("Could not start Claude Code: {}", e))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(prompt.as_bytes()).await.map_err(|e| e.to_string())?;
    }
    let out = tokio::time::timeout(timeout, child.wait_with_output())
        .await
        .map_err(|_| "Claude took too long to answer.".to_string())?
        .map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let results = lift_tool_results(&stdout);
    match results.into_iter().find(|r| r.tool == tool) {
        Some(r) if !r.is_error => Ok(r.text),
        Some(r) => Err(format!("The connector returned an error: {}", r.text.chars().take(200).collect::<String>())),
        None => Err("The Google Calendar connector wasn't reachable. If it needs signing in, run /mcp in Claude Code and authorize google-calendar, then try again.".to_string()),
    }
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
    fn lifts_raw_mcp_result_from_stream_json() {
        let stream = r#"{"type":"system","subtype":"init"}
{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t1","name":"mcp__google-calendar__list_events","input":{"startTime":"x"}}]}}
{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":[{"type":"text","text":"{\"events\":[{\"id\":\"a\"}]}"}]}]}}
{"type":"assistant","message":{"content":[{"type":"text","text":"DONE"}]}}"#;
        let r = lift_tool_results(stream);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].tool, "mcp__google-calendar__list_events");
        assert_eq!(r[0].text, "{\"events\":[{\"id\":\"a\"}]}");
        assert!(!r[0].is_error);
    }

    #[test]
    fn ignores_non_mcp_tools_and_unmatched_results() {
        let stream = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t9","name":"Bash","input":{}}]}}
{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t9","content":"x"}]}}"#;
        assert!(lift_tool_results(stream).is_empty());
    }

    #[test]
    fn open_external_rejects_non_https() {
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        assert!(rt.block_on(open_external("file:///etc/passwd".into())).is_err());
        assert!(rt.block_on(open_external("https://a b".into())).is_err());
    }
}
