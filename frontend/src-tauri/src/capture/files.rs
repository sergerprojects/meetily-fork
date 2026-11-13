//! Read per-meeting files (snapshot images, notes) from a recording folder.
//!
//! The folder comes from the frontend, so every path is checked to sit inside
//! the recordings root and names may not contain separators.

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    crate::audio::recording_preferences::get_default_recordings_folder()
}

/// Folder must exist and live inside the recordings root.
pub fn checked_folder(folder: &str) -> Result<PathBuf, String> {
    checked_folder_in(&root(), folder)
}

pub fn checked_folder_in(root: &Path, folder: &str) -> Result<PathBuf, String> {
    let root = root.canonicalize().map_err(|e| format!("recordings folder: {e}"))?;
    let dir = Path::new(folder).canonicalize().map_err(|e| format!("meeting folder: {e}"))?;
    if !dir.starts_with(&root) || dir == root {
        return Err("folder is outside the recordings folder".into());
    }
    Ok(dir)
}

pub fn safe_name(name: &str) -> Result<&str, String> {
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.starts_with('.') {
        return Err("bad file name".into());
    }
    Ok(name)
}

#[derive(serde::Serialize, Clone, Debug)]
pub struct MeetingImage {
    pub name: String,
    pub path: String,
    /// Seconds into the recording when the image was taken, if known.
    pub at: Option<f64>,
    pub text: Option<String>,
}

#[derive(serde::Deserialize)]
struct IndexEntry {
    name: String,
    at: Option<f64>,
}

/// Snapshot images of a meeting, oldest first.
pub fn list_images(dir: &Path) -> Vec<MeetingImage> {
    let screens = dir.join("screens");
    let index: Vec<IndexEntry> = std::fs::read_to_string(screens.join("index.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let mut out: Vec<MeetingImage> = std::fs::read_dir(&screens)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            // Hidden names are temporary files (".ocr.png") that may vanish at any moment.
            if name.starts_with('.') || !name.to_lowercase().ends_with(".png") {
                return None;
            }
            let at = index.iter().find(|i| i.name == name).and_then(|i| i.at);
            let text = std::fs::read_to_string(screens.join(name.replace(".png", ".txt"))).ok();
            Some(MeetingImage { path: e.path().to_string_lossy().into_owned(), name, at, text })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

#[tauri::command]
pub async fn list_meeting_images(folder: String) -> Result<Vec<MeetingImage>, String> {
    Ok(list_images(&checked_folder(&folder)?))
}

/// Bytes of one image (used for copy and download).
#[tauri::command]
pub async fn read_meeting_image(folder: String, name: String) -> Result<Vec<u8>, String> {
    let dir = checked_folder(&folder)?;
    let name = safe_name(&name)?;
    std::fs::read(dir.join("screens").join(name)).map_err(|e| e.to_string())
}

/// Copy one image into the user's Downloads folder; returns the new path.
#[tauri::command]
pub async fn export_meeting_image(folder: String, name: String) -> Result<String, String> {
    let dir = checked_folder(&folder)?;
    let name = safe_name(&name)?;
    let src = dir.join("screens").join(name);
    let downloads = dirs::download_dir().ok_or("no Downloads folder")?;
    let stem = dir.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "meeting".into());
    let dest = unique_path(&downloads, &format!("{stem}-{name}"));
    std::fs::copy(&src, &dest).map_err(|e| e.to_string())?;
    Ok(dest.to_string_lossy().into_owned())
}

/// `dir/file`, or `dir/file (2)` etc. when it already exists.
fn unique_path(dir: &Path, file: &str) -> PathBuf {
    let first = dir.join(file);
    if !first.exists() {
        return first;
    }
    let p = Path::new(file);
    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = p.extension().map(|s| format!(".{}", s.to_string_lossy())).unwrap_or_default();
    (2..).map(|n| dir.join(format!("{stem} ({n}){ext}"))).find(|c| !c.exists()).unwrap()
}

/// Remove one image and its text.
#[tauri::command]
pub async fn delete_meeting_image(folder: String, name: String) -> Result<(), String> {
    let dir = checked_folder(&folder)?;
    let name = safe_name(&name)?;
    let screens = dir.join("screens");
    std::fs::remove_file(screens.join(name)).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(screens.join(name.replace(".png", ".txt")));
    Ok(())
}

/// The user's own notes (`notes.md`) for the saved-meeting view.
#[tauri::command]
pub async fn read_meeting_notes(folder: String) -> Result<String, String> {
    let dir = checked_folder(&folder)?;
    Ok(std::fs::read_to_string(dir.join("notes.md")).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_must_sit_inside_root() {
        let td = tempfile::tempdir().unwrap();
        let root = td.path().join("rec");
        let inside = root.join("m1");
        let outside = td.path().join("other");
        std::fs::create_dir_all(&inside).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        assert!(checked_folder_in(&root, inside.to_str().unwrap()).is_ok());
        assert!(checked_folder_in(&root, outside.to_str().unwrap()).is_err());
        assert!(checked_folder_in(&root, root.to_str().unwrap()).is_err(), "root itself is not a meeting");
        let sneaky = format!("{}/../other", inside.display());
        assert!(checked_folder_in(&root, &sneaky).is_err());
    }

    #[test]
    fn unique_path_avoids_overwrite() {
        let td = tempfile::tempdir().unwrap();
        assert_eq!(unique_path(td.path(), "a.png"), td.path().join("a.png"));
        std::fs::write(td.path().join("a.png"), b"x").unwrap();
        assert_eq!(unique_path(td.path(), "a.png"), td.path().join("a (2).png"));
        std::fs::write(td.path().join("a (2).png"), b"x").unwrap();
        assert_eq!(unique_path(td.path(), "a.png"), td.path().join("a (3).png"));
    }

    #[test]
    fn names_cannot_escape() {
        assert!(safe_name("001.png").is_ok());
        assert!(safe_name("../x.png").is_err());
        assert!(safe_name(".hidden").is_err());
        assert!(safe_name("").is_err());
    }

    #[test]
    fn lists_images_with_text_and_time() {
        let td = tempfile::tempdir().unwrap();
        let s = td.path().join("screens");
        std::fs::create_dir_all(&s).unwrap();
        std::fs::write(s.join("002.png"), b"x").unwrap();
        std::fs::write(s.join("001.png"), b"x").unwrap();
        std::fs::write(s.join(".ocr.png"), b"x").unwrap();
        std::fs::write(s.join("001.txt"), "Q3 plan").unwrap();
        std::fs::write(s.join("index.json"), r#"[{"name":"001.png","at":12.5}]"#).unwrap();
        let v = list_images(td.path());
        assert_eq!(v.len(), 2, "temporary .ocr.png is not listed");
        assert_eq!(v[0].name, "001.png");
        assert_eq!(v[0].at, Some(12.5));
        assert_eq!(v[0].text.as_deref(), Some("Q3 plan"));
        assert_eq!(v[1].at, None);
    }
}
