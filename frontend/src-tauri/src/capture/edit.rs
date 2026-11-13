//! Small fixes to a saved transcript: remove a line, rename a speaker.

use crate::state::AppState;

/// Names shown on transcript lines: trimmed, not empty, not absurdly long.
pub fn clean_speaker(name: &str) -> Result<String, String> {
    let n = name.trim();
    if n.is_empty() {
        return Err("name is empty".into());
    }
    if n.chars().count() > 80 {
        return Err("name is too long".into());
    }
    Ok(n.to_string())
}

/// Remove one transcript line from a saved meeting.
#[tauri::command]
pub async fn delete_transcript_segment(
    meeting_id: String,
    segment_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<u64, String> {
    let r = sqlx::query("DELETE FROM transcripts WHERE id = ? AND meeting_id = ?")
        .bind(&segment_id)
        .bind(&meeting_id)
        .execute(state.db_manager.pool())
        .await
        .map_err(|e| e.to_string())?;
    Ok(r.rows_affected())
}

/// Give every line by `from` in this meeting the name `to`. Returns lines changed.
#[tauri::command]
pub async fn relabel_speaker(
    meeting_id: String,
    from: String,
    to: String,
    state: tauri::State<'_, AppState>,
) -> Result<u64, String> {
    let to = clean_speaker(&to)?;
    let r = sqlx::query("UPDATE transcripts SET speaker = ? WHERE meeting_id = ? AND speaker = ?")
        .bind(&to)
        .bind(&meeting_id)
        .bind(&from)
        .execute(state.db_manager.pool())
        .await
        .map_err(|e| e.to_string())?;
    Ok(r.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speaker_names_are_trimmed_and_bounded() {
        assert_eq!(clean_speaker("  Priya Shah ").unwrap(), "Priya Shah");
        assert!(clean_speaker("   ").is_err());
        assert!(clean_speaker(&"x".repeat(81)).is_err());
    }
}
