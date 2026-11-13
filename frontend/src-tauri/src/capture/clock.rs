//! Maps wall-clock instants to positions in the transcript's audio stream.
//!
//! Transcript lines are timed by their position in the recorded stream, which
//! does not advance while a recording is paused. Anything observed on the wall
//! clock (who was speaking in the call app, when a slide appeared) must be
//! converted before it can be matched to a transcript line.

use once_cell::sync::Lazy;
use std::sync::Mutex;

#[derive(Debug, Default, Clone)]
pub struct Clock {
    start_ms: Option<i64>,
    /// (paused_at, resumed_at); `None` while still paused.
    pauses: Vec<(i64, Option<i64>)>,
}

impl Clock {
    pub fn start(&mut self, now_ms: i64) {
        self.start_ms = Some(now_ms);
        self.pauses.clear();
    }

    pub fn pause(&mut self, now_ms: i64) {
        if self.start_ms.is_some() && !matches!(self.pauses.last(), Some((_, None))) {
            self.pauses.push((now_ms, None));
        }
    }

    pub fn resume(&mut self, now_ms: i64) {
        if let Some(last) = self.pauses.last_mut() {
            if last.1.is_none() {
                last.1 = Some(now_ms);
            }
        }
    }

    pub fn stop(&mut self) {
        self.start_ms = None;
        self.pauses.clear();
    }

    pub fn is_running(&self) -> bool {
        self.start_ms.is_some()
    }

    /// Seconds into the audio stream for a wall-clock instant, or `None` when
    /// no recording is running. Instants before the start clamp to 0.
    pub fn stream_secs(&self, wall_ms: i64) -> Option<f64> {
        let start = self.start_ms?;
        let wall = wall_ms.max(start);
        let mut paused = 0i64;
        for (p0, p1) in &self.pauses {
            let end = p1.unwrap_or(wall).min(wall);
            if end > *p0 {
                paused += end - *p0;
            }
        }
        Some((wall - start - paused).max(0) as f64 / 1000.0)
    }
}

static CLOCK: Lazy<Mutex<Clock>> = Lazy::new(|| Mutex::new(Clock::default()));

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

pub fn note_start() {
    CLOCK.lock().unwrap().start(now_ms());
}
pub fn note_pause() {
    CLOCK.lock().unwrap().pause(now_ms());
}
pub fn note_resume() {
    CLOCK.lock().unwrap().resume(now_ms());
}
pub fn note_stop() {
    CLOCK.lock().unwrap().stop();
}
pub fn is_running() -> bool {
    CLOCK.lock().unwrap().is_running()
}
/// Seconds into the stream right now.
pub fn stream_now() -> Option<f64> {
    CLOCK.lock().unwrap().stream_secs(now_ms())
}
pub fn stream_secs(wall_ms: i64) -> Option<f64> {
    CLOCK.lock().unwrap().stream_secs(wall_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_started_is_none() {
        assert_eq!(Clock::default().stream_secs(1000), None);
    }

    #[test]
    fn plain_elapsed_without_pauses() {
        let mut c = Clock::default();
        c.start(10_000);
        assert_eq!(c.stream_secs(12_500), Some(2.5));
        assert_eq!(c.stream_secs(5_000), Some(0.0), "before start clamps to 0");
    }

    #[test]
    fn closed_pause_is_removed() {
        let mut c = Clock::default();
        c.start(0);
        c.pause(10_000);
        c.resume(40_000);
        assert_eq!(c.stream_secs(5_000), Some(5.0));
        assert_eq!(c.stream_secs(10_000), Some(10.0));
        assert_eq!(c.stream_secs(25_000), Some(10.0), "inside the pause the stream is frozen");
        assert_eq!(c.stream_secs(45_000), Some(15.0));
    }

    #[test]
    fn open_pause_freezes_until_resumed() {
        let mut c = Clock::default();
        c.start(0);
        c.pause(3_000);
        assert_eq!(c.stream_secs(9_000), Some(3.0));
        c.pause(4_000); // double pause ignored
        c.resume(9_000);
        assert_eq!(c.stream_secs(11_000), Some(5.0));
    }

    #[test]
    fn restart_resets() {
        let mut c = Clock::default();
        c.start(0);
        c.pause(1_000);
        c.resume(2_000);
        c.start(100_000);
        assert_eq!(c.stream_secs(101_000), Some(1.0));
        c.stop();
        assert_eq!(c.stream_secs(101_000), None);
    }
}
