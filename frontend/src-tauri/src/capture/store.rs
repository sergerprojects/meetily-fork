//! Writes snapshots into `<meeting folder>/screens/`: `NNN.png`, `NNN.txt` (text
//! read off the picture) and `index.json` (when each was taken).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct IndexEntry {
    pub name: String,
    /// Seconds into the recording.
    pub at: f64,
    pub w: usize,
    pub h: usize,
}

pub fn screens_dir(meeting_folder: &Path) -> PathBuf {
    meeting_folder.join("screens")
}

fn read_index(dir: &Path) -> Vec<IndexEntry> {
    std::fs::read_to_string(dir.join("index.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Next free file stem: 001, 002, ... never reusing a number, even after a delete.
fn next_stem(dir: &Path, index: &[IndexEntry]) -> String {
    let from_files = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().to_string_lossy().split('.').next().and_then(|s| s.parse::<u32>().ok()))
        .max()
        .unwrap_or(0);
    let from_index = index.iter().filter_map(|i| i.name.split('.').next()?.parse::<u32>().ok()).max().unwrap_or(0);
    format!("{:03}", from_files.max(from_index) + 1)
}

/// Save one snapshot. Returns the image file name (for example `003.png`).
pub fn save(
    meeting_folder: &Path,
    png: &[u8],
    text: Option<&str>,
    at_secs: f64,
    size: (usize, usize),
) -> Result<String, String> {
    let dir = screens_dir(meeting_folder);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut index = read_index(&dir);
    let stem = next_stem(&dir, &index);
    let name = format!("{stem}.png");
    write_atomic(&dir.join(&name), png)?;
    if let Some(t) = text.map(str::trim).filter(|t| !t.is_empty()) {
        write_atomic(&dir.join(format!("{stem}.txt")), t.as_bytes())?;
    }
    index.push(IndexEntry { name: name.clone(), at: (at_secs * 10.0).round() / 10.0, w: size.0, h: size.1 });
    let json = serde_json::to_vec_pretty(&index).map_err(|e| e.to_string())?;
    write_atomic(&dir.join("index.json"), &json)?;
    Ok(name)
}

/// Number of snapshots already saved for this meeting.
pub fn count(meeting_folder: &Path) -> usize {
    read_index(&screens_dir(meeting_folder)).len()
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_image_text_and_index_with_increasing_numbers() {
        let td = tempfile::tempdir().unwrap();
        let a = save(td.path(), b"png-a", Some("Q3 plan"), 12.34, (100, 50)).unwrap();
        let b = save(td.path(), b"png-b", None, 70.0, (100, 50)).unwrap();
        assert_eq!((a.as_str(), b.as_str()), ("001.png", "002.png"));
        let dir = screens_dir(td.path());
        assert_eq!(std::fs::read_to_string(dir.join("001.txt")).unwrap(), "Q3 plan");
        assert!(!dir.join("002.txt").exists(), "no text, no file");
        let idx = read_index(&dir);
        assert_eq!(idx.len(), 2);
        assert_eq!(idx[0].at, 12.3);
        assert_eq!(count(td.path()), 2);
    }

    #[test]
    fn numbers_are_not_reused_after_a_delete() {
        let td = tempfile::tempdir().unwrap();
        save(td.path(), b"1", None, 1.0, (1, 1)).unwrap();
        save(td.path(), b"2", None, 2.0, (1, 1)).unwrap();
        std::fs::remove_file(screens_dir(td.path()).join("002.png")).unwrap();
        // 002 stays in the index, so the next one is 003, never a clash.
        assert_eq!(save(td.path(), b"3", None, 3.0, (1, 1)).unwrap(), "003.png");
    }

    #[test]
    fn blank_text_is_not_saved() {
        let td = tempfile::tempdir().unwrap();
        save(td.path(), b"1", Some("   \n "), 1.0, (1, 1)).unwrap();
        assert!(!screens_dir(td.path()).join("001.txt").exists());
    }
}
