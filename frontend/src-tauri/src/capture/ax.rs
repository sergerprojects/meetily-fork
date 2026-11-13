//! Read another app's Accessibility tree into plain Rust data.
//!
//! The walker is the only unsafe part. Everything that decides what the tree
//! means (who is speaking, who is sharing) works on `Node` values, so it can be
//! tested against dumps saved from real calls.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Node {
    pub role: String,
    pub subrole: String,
    pub title: String,
    pub desc: String,
    pub value: String,
    pub id: String,
    pub help: String,
    pub dom_id: String,
    pub dom_classes: Vec<String>,
    /// x, y, width, height in screen points.
    pub frame: Option<[f64; 4]>,
    pub children: Vec<Node>,
}

impl Node {
    /// Depth-first visit of this node and everything below it.
    pub fn walk<'a>(&'a self, f: &mut impl FnMut(&'a Node)) {
        f(self);
        for c in &self.children {
            c.walk(f);
        }
    }

    pub fn count(&self) -> usize {
        let mut n = 0;
        self.walk(&mut |_| n += 1);
        n
    }

    /// Readable text dump, one node per line, indented by depth.
    pub fn dump(&self) -> String {
        fn clip(s: &str) -> String {
            let one: String = s.chars().map(|c| if c == '\n' || c == '\r' { ' ' } else { c }).collect();
            if one.chars().count() > 140 {
                format!("{}…", one.chars().take(140).collect::<String>())
            } else {
                one
            }
        }
        fn rec(n: &Node, depth: usize, out: &mut String) {
            out.push_str(&"  ".repeat(depth));
            out.push_str(&n.role);
            let mut field = |k: &str, v: &str| {
                if !v.is_empty() {
                    out.push_str(&format!(" {k}=\"{}\"", clip(v)));
                }
            };
            field("sub", &n.subrole);
            field("title", &n.title);
            field("desc", &n.desc);
            field("value", &n.value);
            field("id", &n.id);
            field("help", &n.help);
            field("dom", &n.dom_id);
            if !n.dom_classes.is_empty() {
                out.push_str(&format!(" class=\"{}\"", clip(&n.dom_classes.join(" "))));
            }
            if let Some([x, y, w, h]) = n.frame {
                out.push_str(&format!(" @{x:.0},{y:.0} {w:.0}x{h:.0}"));
            }
            out.push('\n');
            for c in &n.children {
                rec(c, depth + 1, out);
            }
        }
        let mut s = String::new();
        rec(self, 0, &mut s);
        s
    }
}

/// Call apps we know how to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallApp {
    Zoom,
    Teams,
    /// Any Chromium browser (Google Meet runs in a tab).
    Chrome,
}

impl CallApp {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "zoom" => Some(Self::Zoom),
            "teams" => Some(Self::Teams),
            "chrome" | "meet" => Some(Self::Chrome),
            _ => None,
        }
    }

    /// Does this executable path belong to the app's main process (not a helper)?
    pub fn matches_exe(self, path: &str) -> bool {
        if path.contains("Helper") {
            return false;
        }
        match self {
            Self::Zoom => path.ends_with("/zoom.us.app/Contents/MacOS/zoom.us"),
            Self::Teams => {
                path.ends_with("/MSTeams.app/Contents/MacOS/MSTeams")
                    || path.ends_with("/Microsoft Teams.app/Contents/MacOS/Teams")
                    || path.ends_with("/Microsoft Teams (work or school).app/Contents/MacOS/MSTeams")
            }
            Self::Chrome => path.ends_with("/Google Chrome.app/Contents/MacOS/Google Chrome"),
        }
    }
}

#[cfg(target_os = "macos")]
pub use mac::{find_pid, snapshot_app, snapshot_windows, Limits};

#[cfg(target_os = "macos")]
mod mac {
    use super::{CallApp, Node};
    use std::ffi::{c_char, c_void, CString};
    use std::time::{Duration, Instant};

    type CFTypeRef = *const c_void;
    type AXError = i32;

    #[repr(C)]
    #[derive(Default)]
    struct CGRect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXUIElementCreateApplication(pid: i32) -> CFTypeRef;
        fn AXUIElementCopyAttributeValue(el: CFTypeRef, attr: CFTypeRef, value: *mut CFTypeRef) -> AXError;
        fn AXUIElementSetAttributeValue(el: CFTypeRef, attr: CFTypeRef, value: CFTypeRef) -> AXError;
        fn AXUIElementSetMessagingTimeout(el: CFTypeRef, secs: f32) -> AXError;
        fn AXValueGetTypeID() -> usize;
        fn AXValueGetValue(v: CFTypeRef, ty: u32, out: *mut c_void) -> bool;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        static kCFBooleanTrue: CFTypeRef;
        fn CFRelease(cf: CFTypeRef);
        fn CFGetTypeID(cf: CFTypeRef) -> usize;
        fn CFStringGetTypeID() -> usize;
        fn CFArrayGetTypeID() -> usize;
        fn CFBooleanGetTypeID() -> usize;
        fn CFNumberGetTypeID() -> usize;
        fn CFStringCreateWithCString(alloc: *const c_void, c: *const c_char, enc: u32) -> CFTypeRef;
        fn CFStringGetLength(s: CFTypeRef) -> isize;
        fn CFStringGetMaximumSizeForEncoding(len: isize, enc: u32) -> isize;
        fn CFStringGetCString(s: CFTypeRef, buf: *mut c_char, size: isize, enc: u32) -> bool;
        fn CFArrayGetCount(a: CFTypeRef) -> isize;
        fn CFArrayGetValueAtIndex(a: CFTypeRef, i: isize) -> CFTypeRef;
        fn CFBooleanGetValue(b: CFTypeRef) -> bool;
        fn CFNumberGetValue(n: CFTypeRef, ty: i32, out: *mut c_void) -> bool;
    }

    #[link(name = "System")]
    extern "C" {
        fn proc_listallpids(buffer: *mut c_void, buffersize: i32) -> i32;
        fn proc_pidpath(pid: i32, buffer: *mut c_void, buffersize: u32) -> i32;
    }

    const UTF8: u32 = 0x0800_0100;
    const K_AX_VALUE_CGRECT: u32 = 3;
    const K_CF_NUMBER_FLOAT64: i32 = 6;

    /// Owned CoreFoundation reference, released on drop.
    struct Cf(CFTypeRef);
    impl Drop for Cf {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: we own one retain count on this object.
                unsafe { CFRelease(self.0) }
            }
        }
    }

    fn cfstr(s: &str) -> Cf {
        let c = CString::new(s).unwrap_or_default();
        // SAFETY: valid NUL-terminated string; null allocator = default.
        Cf(unsafe { CFStringCreateWithCString(std::ptr::null(), c.as_ptr(), UTF8) })
    }

    fn string_of(v: CFTypeRef) -> Option<String> {
        // SAFETY: type is checked before use; buffer sized by CF itself.
        unsafe {
            if v.is_null() || CFGetTypeID(v) != CFStringGetTypeID() {
                return None;
            }
            let len = CFStringGetLength(v);
            let cap = CFStringGetMaximumSizeForEncoding(len, UTF8) + 1;
            let mut buf = vec![0u8; cap.max(1) as usize];
            if CFStringGetCString(v, buf.as_mut_ptr() as *mut c_char, cap, UTF8) {
                let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
                Some(String::from_utf8_lossy(&buf[..end]).into_owned())
            } else {
                None
            }
        }
    }

    /// String, bool or number rendered as text.
    fn text_of(v: CFTypeRef) -> String {
        if let Some(s) = string_of(v) {
            return s;
        }
        // SAFETY: type ids checked first.
        unsafe {
            if v.is_null() {
                return String::new();
            }
            let t = CFGetTypeID(v);
            if t == CFBooleanGetTypeID() {
                return CFBooleanGetValue(v).to_string();
            }
            if t == CFNumberGetTypeID() {
                let mut f = 0f64;
                if CFNumberGetValue(v, K_CF_NUMBER_FLOAT64, &mut f as *mut f64 as *mut c_void) {
                    return if f.fract() == 0.0 { format!("{}", f as i64) } else { format!("{f}") };
                }
            }
        }
        String::new()
    }

    fn copy_attr(el: CFTypeRef, name: &Cf) -> Option<Cf> {
        let mut out: CFTypeRef = std::ptr::null();
        // SAFETY: out-pointer is valid; on success we own the returned object.
        let err = unsafe { AXUIElementCopyAttributeValue(el, name.0, &mut out) };
        (err == 0 && !out.is_null()).then(|| Cf(out))
    }

    fn array_items(a: &Cf) -> Vec<CFTypeRef> {
        // SAFETY: type checked; indices within count; items borrowed from the array.
        unsafe {
            if CFGetTypeID(a.0) != CFArrayGetTypeID() {
                return vec![];
            }
            (0..CFArrayGetCount(a.0)).map(|i| CFArrayGetValueAtIndex(a.0, i)).collect()
        }
    }

    struct Attrs {
        role: Cf,
        subrole: Cf,
        title: Cf,
        desc: Cf,
        value: Cf,
        id: Cf,
        help: Cf,
        dom_id: Cf,
        dom_classes: Cf,
        frame: Cf,
        children: Cf,
    }

    impl Attrs {
        fn new() -> Self {
            Self {
                role: cfstr("AXRole"),
                subrole: cfstr("AXSubrole"),
                title: cfstr("AXTitle"),
                desc: cfstr("AXDescription"),
                value: cfstr("AXValue"),
                id: cfstr("AXIdentifier"),
                help: cfstr("AXHelp"),
                dom_id: cfstr("AXDOMIdentifier"),
                dom_classes: cfstr("AXDOMClassList"),
                frame: cfstr("AXFrame"),
                children: cfstr("AXChildren"),
            }
        }
    }

    pub struct Limits {
        pub max_depth: usize,
        pub max_nodes: usize,
        pub max_time: Duration,
    }

    impl Limits {
        pub fn probe() -> Self {
            Self { max_depth: 80, max_nodes: 30_000, max_time: Duration::from_secs(10) }
        }
        pub fn poll() -> Self {
            Self { max_depth: 60, max_nodes: 6_000, max_time: Duration::from_millis(800) }
        }
    }

    fn read(el: CFTypeRef, a: &Attrs, depth: usize, started: Instant, count: &mut usize, lim: &Limits) -> Node {
        *count += 1;
        let get = |n: &Cf| copy_attr(el, n).map(|v| text_of(v.0)).unwrap_or_default();
        let mut node = Node {
            role: get(&a.role),
            subrole: get(&a.subrole),
            title: get(&a.title),
            desc: get(&a.desc),
            value: get(&a.value),
            id: get(&a.id),
            help: get(&a.help),
            dom_id: get(&a.dom_id),
            ..Default::default()
        };
        if let Some(list) = copy_attr(el, &a.dom_classes) {
            node.dom_classes = array_items(&list).into_iter().filter_map(string_of).collect();
        }
        if let Some(f) = copy_attr(el, &a.frame) {
            let mut r = CGRect::default();
            // SAFETY: AXValue type checked before extracting a rect.
            let ok = unsafe {
                CFGetTypeID(f.0) == AXValueGetTypeID()
                    && AXValueGetValue(f.0, K_AX_VALUE_CGRECT, &mut r as *mut CGRect as *mut c_void)
            };
            if ok {
                node.frame = Some([r.x, r.y, r.w, r.h]);
            }
        }
        if depth < lim.max_depth {
            if let Some(kids) = copy_attr(el, &a.children) {
                for k in array_items(&kids) {
                    if *count >= lim.max_nodes || started.elapsed() > lim.max_time {
                        break;
                    }
                    node.children.push(read(k, a, depth + 1, started, count, lim));
                }
            }
        }
        node
    }

    /// Process id of the app's main process, if it is running.
    pub fn find_pid(app: CallApp) -> Option<i32> {
        // SAFETY: buffers are sized from the kernel's own answers.
        unsafe {
            let n = proc_listallpids(std::ptr::null_mut(), 0);
            if n <= 0 {
                return None;
            }
            let mut pids = vec![0i32; n as usize + 64];
            let got = proc_listallpids(pids.as_mut_ptr() as *mut c_void, (pids.len() * 4) as i32);
            if got <= 0 {
                return None;
            }
            for pid in pids.into_iter().take(got as usize).filter(|p| *p > 0) {
                let mut buf = vec![0u8; 4096];
                let len = proc_pidpath(pid, buf.as_mut_ptr() as *mut c_void, buf.len() as u32);
                if len > 0 {
                    let path = String::from_utf8_lossy(&buf[..len as usize]).into_owned();
                    if app.matches_exe(&path) {
                        return Some(pid);
                    }
                }
            }
            None
        }
    }

    /// Snapshot the whole accessibility tree of a running app. Chromium only
    /// exposes web page contents once an assistive app asks, so for browsers we
    /// flip `AXManualAccessibility` on first (the same switch screen readers use).
    pub fn snapshot_app(pid: i32, wake_browser: bool, lim: &Limits) -> Option<Node> {
        // SAFETY: standard AX calls on an element we create and release.
        unsafe {
            let app = Cf(AXUIElementCreateApplication(pid));
            if app.0.is_null() {
                return None;
            }
            AXUIElementSetMessagingTimeout(app.0, 1.0);
            if wake_browser {
                for name in ["AXManualAccessibility", "AXEnhancedUserInterface"] {
                    let n = cfstr(name);
                    AXUIElementSetAttributeValue(app.0, n.0, kCFBooleanTrue);
                }
            }
            let attrs = Attrs::new();
            let mut count = 0;
            let tree = read(app.0, &attrs, 0, Instant::now(), &mut count, lim);
            (!tree.role.is_empty() || !tree.children.is_empty()).then_some(tree)
        }
    }

    /// Just the app's windows (no menu bar), one tree per window. Much lighter than
    /// `snapshot_app` for polling.
    pub fn snapshot_windows(pid: i32, wake_browser: bool, lim: &Limits) -> Vec<Node> {
        // SAFETY: standard AX calls on elements we create and release.
        unsafe {
            let app = Cf(AXUIElementCreateApplication(pid));
            if app.0.is_null() {
                return vec![];
            }
            AXUIElementSetMessagingTimeout(app.0, 1.0);
            if wake_browser {
                for name in ["AXManualAccessibility", "AXEnhancedUserInterface"] {
                    let n = cfstr(name);
                    AXUIElementSetAttributeValue(app.0, n.0, kCFBooleanTrue);
                }
            }
            let attrs = Attrs::new();
            let Some(list) = copy_attr(app.0, &cfstr("AXWindows")) else { return vec![] };
            let started = Instant::now();
            let mut count = 0;
            array_items(&list)
                .into_iter()
                .map(|w| read(w, &attrs, 0, started, &mut count, lim))
                .collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(role: &str, desc: &str, kids: Vec<Node>) -> Node {
        Node { role: role.into(), desc: desc.into(), children: kids, ..Default::default() }
    }

    #[test]
    fn dump_indents_and_skips_empty_fields() {
        let t = n("AXWindow", "", vec![n("AXGroup", "Alice (speaking)", vec![])]);
        let d = t.dump();
        assert_eq!(d, "AXWindow\n  AXGroup desc=\"Alice (speaking)\"\n");
    }

    #[test]
    fn walk_and_count_visit_every_node() {
        let t = n("A", "", vec![n("B", "", vec![n("C", "", vec![])]), n("D", "", vec![])]);
        assert_eq!(t.count(), 4);
    }

    #[test]
    fn exe_matching_skips_helpers() {
        assert!(CallApp::Zoom.matches_exe("/Applications/zoom.us.app/Contents/MacOS/zoom.us"));
        assert!(!CallApp::Chrome.matches_exe(
            "/Applications/Google Chrome.app/Contents/Frameworks/Google Chrome Framework.framework/Helpers/Google Chrome Helper.app/Contents/MacOS/Google Chrome Helper"
        ));
        assert!(CallApp::Chrome.matches_exe("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"));
        assert!(CallApp::Teams.matches_exe("/Applications/Microsoft Teams.app/Contents/MacOS/Teams"));
        assert_eq!(CallApp::parse("Meet"), Some(CallApp::Chrome));
    }
}
