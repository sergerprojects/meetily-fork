//! macOS privacy permissions the capture features need.
//!
//! Accessibility: read participant names / who is speaking from the call app.
//! Screen Recording: snapshots of a shared screen.
//! Other platforms report "not granted" so the features stay off.

#[cfg(target_os = "macos")]
mod mac {
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGPreflightScreenCaptureAccess() -> bool;
        fn CGRequestScreenCaptureAccess() -> bool;
    }

    pub fn accessibility(prompt: bool) -> bool {
        if prompt {
            cidre::ax::is_process_trusted_with_prompt(true)
        } else {
            cidre::ax::is_process_trusted()
        }
    }

    pub fn screen(request: bool) -> bool {
        // SAFETY: plain C calls with no arguments.
        unsafe {
            if request {
                CGRequestScreenCaptureAccess()
            } else {
                CGPreflightScreenCaptureAccess()
            }
        }
    }
}

#[cfg(target_os = "macos")]
pub fn accessibility_granted() -> bool {
    mac::accessibility(false)
}
#[cfg(target_os = "macos")]
pub fn request_accessibility() -> bool {
    mac::accessibility(true)
}
#[cfg(target_os = "macos")]
pub fn screen_granted() -> bool {
    mac::screen(false)
}
#[cfg(target_os = "macos")]
pub fn request_screen() -> bool {
    mac::screen(true)
}

#[cfg(not(target_os = "macos"))]
pub fn accessibility_granted() -> bool {
    false
}
#[cfg(not(target_os = "macos"))]
pub fn request_accessibility() -> bool {
    false
}
#[cfg(not(target_os = "macos"))]
pub fn screen_granted() -> bool {
    false
}
#[cfg(not(target_os = "macos"))]
pub fn request_screen() -> bool {
    false
}
