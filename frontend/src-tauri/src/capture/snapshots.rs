//! While a recording runs: notice when someone shares their screen in Google Meet
//! (Chrome), take a picture of the shared content whenever it changes, read its
//! text, and save both with the meeting. Needs Accessibility and Screen Recording
//! and the "Snapshots" switch in Settings.

use super::ax::{self, CallApp, Limits};
use super::keeper::{KeepRules, Keeper};
use super::{clock, meet, perms, screen, store};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Wry};

const IDLE: Duration = Duration::from_secs(2);
const ACTIVE: Duration = Duration::from_secs(1);

/// The two rectangles describe the same window (screen points, a little slack).
pub fn same_window(a: [f64; 4], b: [f64; 4]) -> bool {
    a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() <= 3.0)
}

/// Pixel rectangle inside a captured window picture for a stage given in screen points.
pub fn stage_in_pixels(window: [f64; 4], stage: [f64; 4], pic_w: usize, pic_h: usize) -> (usize, usize, usize, usize) {
    let sx = pic_w as f64 / window[2].max(1.0);
    let sy = pic_h as f64 / window[3].max(1.0);
    let x = ((stage[0] - window[0]) * sx).max(0.0);
    let y = ((stage[1] - window[1]) * sy).max(0.0);
    (x as usize, y as usize, (stage[2] * sx) as usize, (stage[3] * sy) as usize)
}

pub fn start(app: AppHandle<Wry>) {
    // Apple objects are not Send, so everything lives on one dedicated thread.
    let _ = std::thread::Builder::new()
        .name("snapshots".into())
        .spawn(move || tauri::async_runtime::block_on(run(app)));
}

async fn run(app: AppHandle<Wry>) {
    let mut keeper = Keeper::new(KeepRules::default());
    let mut was_recording = false;
    let mut folder: Option<PathBuf> = None;
    let mut sharing = false;
    loop {
        tokio::time::sleep(if sharing { ACTIVE } else { IDLE }).await;
        let recording = crate::audio::recording_commands::is_recording().await;
        if !recording {
            was_recording = false;
            sharing = false;
            continue;
        }
        if !was_recording {
            was_recording = true;
            keeper = Keeper::new(KeepRules::default());
            folder = None;
            sharing = false;
        }
        if crate::audio::recording_commands::is_recording_paused().await
            || !super::snapshots_enabled(&app)
            || !perms::screen_granted()
            || !perms::accessibility_granted()
        {
            sharing = false;
            continue;
        }
        if folder.is_none() {
            folder = crate::audio::recording_commands::get_meeting_folder_path()
                .await
                .ok()
                .flatten()
                .map(PathBuf::from);
            if let Some(f) = &folder {
                keeper.set_kept(store::count(f));
            }
        }
        let Some(dir) = folder.clone() else { continue };
        match tick(&app, &dir, &mut keeper).await {
            Ok(active) => sharing = active,
            Err(e) => {
                log::debug!("snapshots: {e}");
                sharing = false;
            }
        }
    }
}

/// One look at Chrome. Returns whether someone is currently sharing.
async fn tick(app: &AppHandle<Wry>, dir: &Path, keeper: &mut Keeper) -> Result<bool, String> {
    let Some(pid) = ax::find_pid(CallApp::Chrome) else { return Ok(false) };
    let windows = ax::snapshot_windows(pid, true, &Limits::poll());
    let own_ok = super::own_shares_enabled(app);
    let found = windows.iter().find_map(|w| meet::detect(w).filter(|s| own_ok || !s.own).map(|s| (w, s)));
    let Some((win, share)) = found else {
        keeper.reset_picture();
        return Ok(false);
    };
    let wf = win.frame.ok_or("window has no frame")?;
    let sc_windows = screen::windows_of_pid(pid).await?;
    let target = sc_windows.iter().find(|s| same_window(s.frame, wf)).ok_or("window not found")?;
    let scale = (2560.0 / wf[2].max(1.0)).min(2.0);
    let pic = screen::capture_window(target.id, (wf[2] * scale) as usize, (wf[3] * scale) as usize).await?;
    // Meet's page is dark with the shared content as one big light block (plus its own
    // buttons around it). Take the whole page and keep that block, so a few points of
    // error in the page's reported positions cannot cut the content off. If the page has
    // no clear block (a dark share), fall back to the reported stage rectangle.
    let (px, py, pw, ph) = stage_in_pixels(wf, share.page, pic.w, pic.h);
    let page_pic = pic.crop(px, py, pw, ph);
    let from_page = page_pic.as_ref().and_then(|p| {
        let (x, y, w, h) = p.dense_bounds(40.0, 0.4)?;
        p.crop(x, y, w, h)
    });
    let shared = match from_page {
        Some(f) => f,
        None => {
            let (x, y, w, h) = stage_in_pixels(wf, share.stage, pic.w, pic.h);
            let Some(f) = pic.crop(x, y, w, h) else { return Ok(true) };
            f
        }
    };
    if shared.is_blank() {
        return Ok(true);
    }
    let t = clock::stream_now().unwrap_or(0.0);
    if keeper.offer(shared.ahash(), t) {
        let png = shared.to_png()?;
        let text = ocr_bytes(dir, &png);
        let name = store::save(dir, &png, text.as_deref(), t, (shared.w, shared.h))?;
        log::info!("snapshots: saved {name} ({} shared by {})", if share.own { "own screen" } else { "screen" }, share.presenter);
        let _ = app.emit("snapshot-added", name);
    }
    Ok(true)
}

/// Text in a PNG, via a short-lived file inside the meeting's screens folder.
fn ocr_bytes(dir: &Path, png: &[u8]) -> Option<String> {
    let screens = store::screens_dir(dir);
    std::fs::create_dir_all(&screens).ok()?;
    let tmp = screens.join(".ocr.png");
    std::fs::write(&tmp, png).ok()?;
    let text = screen::ocr_png(&tmp);
    let _ = std::fs::remove_file(&tmp);
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_match_allows_small_differences() {
        let a = [-1520.0, -1384.0, 2797.0, 1360.0];
        assert!(same_window(a, [-1519.0, -1383.5, 2797.0, 1361.0]));
        assert!(!same_window(a, [-1520.0, -1384.0, 1200.0, 800.0]));
    }

    #[test]
    fn stage_maps_into_the_picture_at_2x() {
        // Window at (-1520,-1384) 2797x1360 points, captured at 2x.
        let win = [-1520.0, -1384.0, 2797.0, 1360.0];
        let stage = [-1203.0, -1065.0, 1563.0, 814.0];
        let (x, y, w, h) = stage_in_pixels(win, stage, 5594, 2720);
        assert_eq!((x, y), (634, 638));
        assert_eq!((w, h), (3126, 1628));
    }

    #[test]
    fn stage_left_of_the_window_clamps_to_zero() {
        let (x, y, _, _) = stage_in_pixels([100.0, 100.0, 800.0, 600.0], [50.0, 90.0, 100.0, 100.0], 800, 600);
        assert_eq!((x, y), (0, 0));
    }
}
