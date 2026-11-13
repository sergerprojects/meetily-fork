//! "Joined a call" signal from microphone activity.
//!
//! macOS: reads CoreAudio state directly (no extra crate). Preferred source is
//! the per-process list (macOS 14+, `kAudioProcessPropertyIsRunningInput`),
//! which lets us ignore Meetily's own capture. Older systems fall back to
//! `kAudioDevicePropertyDeviceIsRunningSomewhere` on the default input device,
//! which can't tell us apart, so callers must ignore it while recording.
//! Other OSes: `sample()` returns None (feature off).
//!
//! Always-on system listeners (e.g. `corespeechd` for "Hey Siri", dictation)
//! hold input permanently and are ignored by executable path.

use chrono::{DateTime, Duration, Utc};
use once_cell::sync::Lazy;
use std::sync::RwLock;

/// Mic must be in use continuously this long to count as "joined".
pub const JOIN_AFTER: Duration = Duration::seconds(8);
/// Mic must be idle continuously this long to count as "left".
pub const LEAVE_AFTER: Duration = Duration::seconds(90);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicSample {
    /// Some process other than Meetily is capturing input.
    OtherProcesses(bool),
    /// Default input device is running (may be Meetily itself).
    DeviceOnly(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicTransition {
    Joined,
    Left,
}

/// Debounce raw mic samples into joined/left transitions. Pure; driven by
/// timestamps so it is testable without audio hardware.
#[derive(Debug, Default, Clone)]
pub struct MicDebounce {
    active: bool,
    since: Option<DateTime<Utc>>,
    joined: bool,
}

impl MicDebounce {
    pub fn update(&mut self, active: bool, now: DateTime<Utc>) -> Option<MicTransition> {
        if self.since.is_none() || active != self.active {
            self.active = active;
            self.since = Some(now);
        }
        let held = now - self.since.unwrap_or(now);
        if !self.joined && self.active && held >= JOIN_AFTER {
            self.joined = true;
            return Some(MicTransition::Joined);
        }
        if self.joined && !self.active && held >= LEAVE_AFTER {
            self.joined = false;
            return Some(MicTransition::Left);
        }
        None
    }

    pub fn is_joined(&self) -> bool {
        self.joined
    }

    /// How long the mic has been continuously active (None when idle).
    pub fn active_for(&self, now: DateTime<Utc>) -> Option<Duration> {
        match (self.active, self.since) {
            (true, Some(since)) => Some(now - since),
            _ => None,
        }
    }
}

/// System daemons that keep the mic open outside of calls (Siri / dictation
/// listeners live under these). User apps (/Applications, /System/Applications
/// like FaceTime) still count.
const IGNORED_PATH_PREFIXES: &[&str] = &["/System/Library/", "/usr/libexec/", "/usr/sbin/"];

/// Note-takers that hold the mic open on their own (Granola keeps a helper on
/// the mic even when not recording) must not count as "joined a call".
const IGNORED_PATH_SUBSTRINGS: &[&str] = &["/Granola.app/", "/Granola Helper"];

/// App names the user never wants join prompts for (lowercase substrings of the
/// executable path), e.g. "discord", "facetime".
static USER_IGNORED: Lazy<RwLock<Vec<String>>> = Lazy::new(|| RwLock::new(Vec::new()));

pub fn set_user_ignored(apps: &[String]) {
    if let Ok(mut v) = USER_IGNORED.write() {
        *v = apps.iter().map(|a| a.trim().to_lowercase()).filter(|a| !a.is_empty()).collect();
    }
}

pub fn is_ignored_input_process(path: &str) -> bool {
    IGNORED_PATH_PREFIXES.iter().any(|p| path.starts_with(p))
        || IGNORED_PATH_SUBSTRINGS.iter().any(|s| path.contains(s))
        || {
            let lower = path.to_lowercase();
            USER_IGNORED.read().map_or(false, |v| v.iter().any(|a| lower.contains(a.as_str())))
        }
}

#[cfg(target_os = "macos")]
pub fn sample() -> Option<MicSample> {
    macos::other_process_input_running()
        .map(MicSample::OtherProcesses)
        .or_else(|| macos::default_input_running_somewhere().map(MicSample::DeviceOnly))
}

#[cfg(not(target_os = "macos"))]
pub fn sample() -> Option<MicSample> {
    None
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::c_void;
    use std::mem::size_of;
    use std::ptr;

    type AudioObjectID = u32;
    type OSStatus = i32;

    #[repr(C)]
    struct AudioObjectPropertyAddress {
        selector: u32,
        scope: u32,
        element: u32,
    }

    extern "C" {
        // libproc (libSystem)
        fn proc_pidpath(pid: i32, buffer: *mut c_void, buffersize: u32) -> i32;
    }

    fn exe_path(pid: i32) -> Option<String> {
        let mut buf = vec![0u8; 4096];
        // SAFETY: buffer is valid for its length.
        let n = unsafe { proc_pidpath(pid, buf.as_mut_ptr() as *mut c_void, buf.len() as u32) };
        (n > 0).then(|| String::from_utf8_lossy(&buf[..n as usize]).into_owned())
    }

    #[link(name = "CoreAudio", kind = "framework")]
    extern "C" {
        fn AudioObjectGetPropertyDataSize(
            object: AudioObjectID,
            address: *const AudioObjectPropertyAddress,
            qualifier_size: u32,
            qualifier: *const c_void,
            out_size: *mut u32,
        ) -> OSStatus;
        fn AudioObjectGetPropertyData(
            object: AudioObjectID,
            address: *const AudioObjectPropertyAddress,
            qualifier_size: u32,
            qualifier: *const c_void,
            io_size: *mut u32,
            out_data: *mut c_void,
        ) -> OSStatus;
    }

    const fn fourcc(s: &[u8; 4]) -> u32 {
        u32::from_be_bytes(*s)
    }

    const SYSTEM_OBJECT: AudioObjectID = 1;
    const SCOPE_GLOBAL: u32 = fourcc(b"glob");
    const ELEMENT_MAIN: u32 = 0;
    const HW_DEFAULT_INPUT_DEVICE: u32 = fourcc(b"dIn ");
    const HW_PROCESS_OBJECT_LIST: u32 = fourcc(b"prs#");
    const DEVICE_IS_RUNNING_SOMEWHERE: u32 = fourcc(b"gone");
    const PROCESS_PID: u32 = fourcc(b"ppid");
    const PROCESS_IS_RUNNING_INPUT: u32 = fourcc(b"piri");

    fn addr(selector: u32) -> AudioObjectPropertyAddress {
        AudioObjectPropertyAddress { selector, scope: SCOPE_GLOBAL, element: ELEMENT_MAIN }
    }

    /// Read a fixed-size scalar property.
    fn get_scalar<T: Copy + Default>(object: AudioObjectID, selector: u32) -> Option<T> {
        let a = addr(selector);
        let mut value = T::default();
        let mut size = size_of::<T>() as u32;
        // SAFETY: `value` is a valid, writable buffer of `size` bytes.
        let status = unsafe {
            AudioObjectGetPropertyData(object, &a, 0, ptr::null(), &mut size, &mut value as *mut T as *mut c_void)
        };
        (status == 0 && size as usize == size_of::<T>()).then_some(value)
    }

    /// macOS 14+: is any process other than this one (and other than ignored
    /// system listeners) capturing input?
    /// None when the process-object API is unavailable.
    pub fn other_process_input_running() -> Option<bool> {
        let a = addr(HW_PROCESS_OBJECT_LIST);
        let mut size: u32 = 0;
        // SAFETY: out pointer is valid.
        let status = unsafe { AudioObjectGetPropertyDataSize(SYSTEM_OBJECT, &a, 0, ptr::null(), &mut size) };
        if status != 0 {
            return None;
        }
        let count = size as usize / size_of::<AudioObjectID>();
        let mut ids: Vec<AudioObjectID> = vec![0; count];
        if count > 0 {
            // SAFETY: `ids` holds `size` bytes.
            let status = unsafe {
                AudioObjectGetPropertyData(SYSTEM_OBJECT, &a, 0, ptr::null(), &mut size, ids.as_mut_ptr() as *mut c_void)
            };
            if status != 0 {
                return None;
            }
            ids.truncate(size as usize / size_of::<AudioObjectID>());
        }
        let own_pid = std::process::id() as i32;
        Some(ids.iter().any(|&id| {
            let Some(pid) = get_scalar::<i32>(id, PROCESS_PID) else { return false };
            pid != own_pid
                && get_scalar::<u32>(id, PROCESS_IS_RUNNING_INPUT).map_or(false, |v| v != 0)
                // Unknown path (permission) counts as a real app.
                && !exe_path(pid).map_or(false, |p| super::is_ignored_input_process(&p))
        }))
    }

    /// Default input device in use by any process (including this one).
    pub fn default_input_running_somewhere() -> Option<bool> {
        let device = get_scalar::<AudioObjectID>(SYSTEM_OBJECT, HW_DEFAULT_INPUT_DEVICE)?;
        if device == 0 {
            return Some(false);
        }
        get_scalar::<u32>(device, DEVICE_IS_RUNNING_SOMEWHERE).map(|v| v != 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(secs: i64) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-11-12T15:00:00Z").unwrap().with_timezone(&Utc)
            + Duration::seconds(secs)
    }

    #[test]
    fn joins_after_8s_continuous() {
        let mut d = MicDebounce::default();
        assert_eq!(d.update(true, t(0)), None);
        assert_eq!(d.update(true, t(6)), None);
        assert_eq!(d.update(true, t(8)), Some(MicTransition::Joined));
        assert!(d.is_joined());
        assert_eq!(d.update(true, t(10)), None, "Joined fires once");
        assert_eq!(d.active_for(t(20)), Some(Duration::seconds(20)));
    }

    #[test]
    fn blip_resets_join_timer() {
        let mut d = MicDebounce::default();
        d.update(true, t(0));
        assert_eq!(d.update(true, t(6)), None);
        assert_eq!(d.update(false, t(7)), None);
        assert_eq!(d.active_for(t(7)), None);
        assert_eq!(d.update(true, t(8)), None);
        assert_eq!(d.update(true, t(14)), None);
        assert_eq!(d.update(true, t(16)), Some(MicTransition::Joined));
    }

    #[test]
    fn leaves_after_90s_idle_and_short_gaps_dont_leave() {
        let mut d = MicDebounce::default();
        d.update(true, t(0));
        d.update(true, t(10));
        assert!(d.is_joined());
        // 60s mute/idle gap, then active again: still joined.
        assert_eq!(d.update(false, t(20)), None);
        assert_eq!(d.update(false, t(80)), None);
        assert_eq!(d.update(true, t(82)), None);
        assert!(d.is_joined());
        // Real leave.
        assert_eq!(d.update(false, t(100)), None);
        assert_eq!(d.update(false, t(189)), None);
        assert_eq!(d.update(false, t(190)), Some(MicTransition::Left));
        assert!(!d.is_joined());
        assert_eq!(d.update(false, t(300)), None, "Left fires once");
    }

    #[test]
    fn system_listeners_ignored() {
        assert!(is_ignored_input_process("/System/Library/PrivateFrameworks/CoreSpeech.framework/corespeechd"));
        assert!(is_ignored_input_process("/usr/libexec/somed"));
        assert!(is_ignored_input_process(
            "/Applications/Granola.app/Contents/Frameworks/Granola Helper.app/Contents/MacOS/Granola Helper"
        ));
        assert!(!is_ignored_input_process("/Applications/zoom.us.app/Contents/MacOS/zoom.us"));
        assert!(!is_ignored_input_process("/Applications/zoom.us.app/Contents/MacOS/zoom.us"));
        assert!(!is_ignored_input_process("/System/Applications/FaceTime.app/Contents/MacOS/FaceTime"));
        assert!(!is_ignored_input_process(
            "/Applications/Google Chrome.app/Contents/Frameworks/Google Chrome Framework.framework/Helpers/Google Chrome Helper.app/Contents/MacOS/Google Chrome Helper"
        ));
    }

    #[test]
    fn user_ignored_apps_match_path_case_insensitively() {
        let discord = "/Applications/Discord.app/Contents/MacOS/Discord";
        assert!(!is_ignored_input_process(discord));
        set_user_ignored(&[" Discord ".to_string(), "".to_string()]);
        assert!(is_ignored_input_process(discord));
        assert!(!is_ignored_input_process("/Applications/zoom.us.app/Contents/MacOS/zoom.us"));
        set_user_ignored(&[]);
        assert!(!is_ignored_input_process(discord));
    }

    #[test]
    fn idle_from_start_never_transitions() {
        let mut d = MicDebounce::default();
        for s in (0..300).step_by(2) {
            assert_eq!(d.update(false, t(s)), None);
        }
    }
}
