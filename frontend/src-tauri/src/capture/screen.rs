//! Screen pictures and text recognition (macOS).
//!
//! - list a process's windows and capture one as a `Frame` (ScreenCaptureKit, needs
//!   Screen Recording permission),
//! - read the text out of a saved PNG (Apple Vision, on-device).

use super::frame::Frame;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct WindowInfo {
    pub id: u32,
    pub title: String,
    pub pid: i32,
    /// x, y, width, height in screen points.
    pub frame: [f64; 4],
    pub on_screen: bool,
}

#[cfg(target_os = "macos")]
pub async fn windows_of_pid(pid: i32) -> Result<Vec<WindowInfo>, String> {
    use cidre::sc;
    let content = sc::ShareableContent::current().await.map_err(|e| format!("{e:?}"))?;
    let mut out = Vec::new();
    for w in content.windows().iter() {
        let Some(app) = w.owning_app() else { continue };
        if app.process_id() != pid {
            continue;
        }
        let f = w.frame();
        out.push(WindowInfo {
            id: w.id(),
            title: w.title().map(|t| t.to_string()).unwrap_or_default(),
            pid,
            frame: [f.origin.x, f.origin.y, f.size.width, f.size.height],
            on_screen: w.is_on_screen(),
        });
    }
    Ok(out)
}

/// Picture of one window at its pixel size. The window must be on screen.
#[cfg(target_os = "macos")]
pub async fn capture_window(id: u32, width_px: usize, height_px: usize) -> Result<Frame, String> {
    use cidre::{cv, sc};
    let content = sc::ShareableContent::current().await.map_err(|e| format!("{e:?}"))?;
    let window = content
        .windows()
        .iter()
        .find(|w| w.id() == id)
        .ok_or_else(|| "window is gone".to_string())?
        .retained();
    let filter = sc::ContentFilter::with_desktop_independent_window(&window);
    let mut cfg = sc::StreamCfg::new();
    cfg.set_width(width_px);
    cfg.set_height(height_px);
    cfg.set_pixel_format(cv::PixelFormat::_32_BGRA);
    cfg.set_shows_cursor(false);
    let buf = sc::ScreenshotManager::capture_sample_buf(&filter, &cfg).await.map_err(|e| format!("{e:?}"))?;
    let image = buf.image_buf().ok_or("no image in capture")?;
    // SAFETY: the sample buffer of a screenshot holds a pixel buffer.
    let pixels: &cv::PixelBuf = unsafe { &*(image as *const cv::ImageBuf as *const cv::PixelBuf) };
    pixel_buf_to_frame(pixels)
}

#[cfg(target_os = "macos")]
#[link(name = "CoreVideo", kind = "framework")]
extern "C" {
    fn CVPixelBufferLockBaseAddress(pb: *const std::ffi::c_void, flags: u64) -> i32;
    fn CVPixelBufferUnlockBaseAddress(pb: *const std::ffi::c_void, flags: u64) -> i32;
    fn CVPixelBufferGetBaseAddress(pb: *const std::ffi::c_void) -> *const u8;
    fn CVPixelBufferGetBytesPerRow(pb: *const std::ffi::c_void) -> usize;
}

#[cfg(target_os = "macos")]
fn pixel_buf_to_frame(pb: &cidre::cv::PixelBuf) -> Result<Frame, String> {
    use std::ffi::c_void;
    const READ_ONLY: u64 = 1;
    let (w, h) = (pb.width(), pb.height());
    let raw = pb as *const cidre::cv::PixelBuf as *const c_void;
    // SAFETY: `raw` is a live CVPixelBuffer; the base address is only read between
    // lock and unlock, and every row read stays inside `bytes_per_row * h`.
    unsafe {
        if CVPixelBufferLockBaseAddress(raw, READ_ONLY) != 0 {
            return Err("could not lock the capture".into());
        }
        let stride = CVPixelBufferGetBytesPerRow(raw);
        let base = CVPixelBufferGetBaseAddress(raw);
        let result = if base.is_null() || stride < w * 4 {
            Err("unexpected pixel layout".to_string())
        } else {
            let mut data = Vec::with_capacity(w * h * 4);
            for row in 0..h {
                data.extend_from_slice(std::slice::from_raw_parts(base.add(row * stride), w * 4));
            }
            Frame::new(w, h, data).ok_or_else(|| "empty capture".to_string())
        };
        CVPixelBufferUnlockBaseAddress(raw, READ_ONLY);
        result
    }
}

/// Text found in a PNG, top to bottom, one line per recognised line. None when
/// nothing readable or Vision fails.
#[cfg(target_os = "macos")]
pub fn ocr_png(path: &Path) -> Option<String> {
    use cidre::{ns, vn};
    let url = ns::Url::with_fs_path_str(path.to_str()?, false);
    let handler = vn::ImageRequestHandler::with_url(&url, None);
    let mut req = vn::RecognizeTextRequest::new();
    req.set_recognition_level(vn::RequestTextRecognitionLevel::Accurate);
    req.set_uses_lang_correction(true);
    let base: &vn::Request = &req;
    let reqs = ns::Array::from_slice(&[base]);
    handler.perform(&reqs).ok()?;
    let results = req.results()?;
    let lines: Vec<String> = results
        .iter()
        .filter_map(|obs| {
            let cands = obs.top_candidates(1);
            cands.iter().next().map(|c| c.string().to_string())
        })
        .collect();
    let text = lines.join("\n");
    (!text.trim().is_empty()).then_some(text)
}

#[cfg(not(target_os = "macos"))]
pub async fn windows_of_pid(_pid: i32) -> Result<Vec<WindowInfo>, String> {
    Err("only on macOS".into())
}
#[cfg(not(target_os = "macos"))]
pub async fn capture_window(_id: u32, _w: usize, _h: usize) -> Result<Frame, String> {
    Err("only on macOS".into())
}
#[cfg(not(target_os = "macos"))]
pub fn ocr_png(_path: &Path) -> Option<String> {
    None
}
