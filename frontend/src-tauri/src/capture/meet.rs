//! Reading Google Meet's accessibility tree: is someone presenting, who, and where
//! on screen the shared content sits. Pure functions over `ax::Node`, so they are
//! tested against trees shaped like the dumps saved from real calls.

use super::ax::Node;

#[derive(Debug, Clone, PartialEq)]
pub struct Share {
    /// Name shown in Meet's presentation bar ("Alex Kim").
    pub presenter: String,
    /// The share is ours (Meet says "You").
    pub own: bool,
    /// Where the shared content is, x y w h in screen points.
    pub stage: [f64; 4],
}

fn area(f: &[f64; 4]) -> f64 {
    f[2].max(0.0) * f[3].max(0.0)
}

fn find<'a>(root: &'a Node, pred: &impl Fn(&Node) -> bool) -> Option<&'a Node> {
    let mut hit = None;
    root.walk(&mut |n| {
        if hit.is_none() && pred(n) {
            hit = Some(n);
        }
    });
    hit
}

/// The Meet page inside a browser window, if this window shows one.
pub fn meet_page(window: &Node) -> Option<&Node> {
    find(window, &|n| n.role == "AXWebArea" && n.title.starts_with("Meet - "))
}

/// Split "Alex Morgan (You, presenting, annotating)" into name and own-flag.
pub fn parse_presenter(heading: &str) -> Option<(String, bool)> {
    let lower = heading.to_lowercase();
    if !lower.contains("presenting") && !lower.contains("presentation") {
        return None;
    }
    let name = heading.split(" (").next().unwrap_or(heading).trim().to_string();
    let own = lower.contains("(you") || lower.contains(" you,") || lower.contains(", you");
    (!name.is_empty()).then_some((name, own))
}

/// Sharing state of the Meet page in this window, or None when nobody presents.
pub fn detect(window: &Node) -> Option<Share> {
    let page = meet_page(window)?;
    let region = find(page, &|n| n.role == "AXGroup" && n.subrole == "AXLandmarkRegion" && n.title == "Presentation")?;
    let heading = find(region, &|n| n.role == "AXHeading" && parse_presenter(&n.title).is_some())?;
    let (presenter, own) = parse_presenter(&heading.title)?;

    let main = find(page, &|n| n.subrole == "AXLandmarkMain");
    let stage = main
        .and_then(|m| {
            let mut best: Option<[f64; 4]> = None;
            m.walk(&mut |n| {
                if n.role == "AXImage" {
                    if let Some(f) = n.frame {
                        if area(&f) >= 200.0 * 150.0 && best.map_or(true, |b| area(&f) > area(&b)) {
                            best = Some(f);
                        }
                    }
                }
            });
            best.or(m.frame)
        })
        .or(page.frame)?;
    Some(Share { presenter, own, stage })
}

/// Words in Meet's own interface that are never a person's name.
const NOT_NAMES: &[&str] = &[
    "you", "presentation audio", "pinned", "mute", "muted", "turn off microphone", "people", "chat", "more options",
    "meeting details", "leave call", "send a reaction", "host controls", "call controls",
];

fn is_marker(text: &str) -> bool {
    let t = text.to_lowercase();
    t.contains("is speaking") || t.contains("speaking now") || t.contains("currently speaking") || t == "speaking"
}

fn name_like(text: &str) -> Option<String> {
    let t = text.trim();
    // Your own tile ("Alex Morgan (You)"): your lines come from the microphone.
    if t.to_lowercase().ends_with("(you)") {
        return None;
    }
    let n = t.chars().count();
    if !(2..=40).contains(&n) || !t.chars().any(|c| c.is_alphabetic()) || is_marker(t) {
        return None;
    }
    if NOT_NAMES.contains(&t.to_lowercase().as_str()) || t.contains(':') {
        return None;
    }
    Some(t.to_string())
}

fn texts(n: &Node, out: &mut Vec<String>) {
    n.walk(&mut |x| {
        if x.role == "AXStaticText" || x.role == "AXHeading" {
            for v in [&x.value, &x.title] {
                if let Some(nm) = name_like(v) {
                    if !out.contains(&nm) {
                        out.push(nm);
                    }
                }
            }
        }
    });
}

fn node_has_marker(n: &Node) -> bool {
    [&n.title, &n.desc, &n.value, &n.help].iter().any(|t| is_marker(t))
}

/// (marker seen below, names resolved below)
fn speaking_walk(n: &Node) -> (bool, Vec<String>) {
    let mut marker = node_has_marker(n);
    let mut found: Vec<String> = Vec::new();
    for c in &n.children {
        let (m, f) = speaking_walk(c);
        marker |= m;
        for name in f {
            if !found.contains(&name) {
                found.push(name);
            }
        }
    }
    if !found.is_empty() || !marker {
        return (marker, found);
    }
    // A marker is somewhere below and no tile claimed it: if this node holds exactly
    // one name, that is the speaker; otherwise keep climbing.
    let mut names = Vec::new();
    texts(n, &mut names);
    if names.len() == 1 {
        (true, names)
    } else {
        (true, Vec::new())
    }
}

/// Names the page marks as speaking right now. Empty when nothing is marked (Meet's
/// markup for this is read from the dump of a real multi-person call; unverified).
pub fn speaking(window: &Node) -> Vec<String> {
    match meet_page(window) {
        Some(page) => speaking_walk(page).1,
        None => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(role: &str, sub: &str, title: &str, frame: Option<[f64; 4]>, kids: Vec<Node>) -> Node {
        Node { role: role.into(), subrole: sub.into(), title: title.into(), frame, children: kids, ..Default::default() }
    }

    /// Shaped like a real Meet window while presenting.
    fn meet_window(heading: Option<&str>, image: Option<[f64; 4]>) -> Node {
        let region = heading.map(|h| {
            n("AXGroup", "AXLandmarkRegion", "Presentation", None, vec![n("AXHeading", "", h, None, vec![])])
        });
        let main = n(
            "AXGroup",
            "AXLandmarkMain",
            "",
            Some([-1204.0, -1195.0, 1564.0, 1091.0]),
            image.map(|f| vec![n("AXImage", "", "", Some(f), vec![])]).unwrap_or_default(),
        );
        let page = n(
            "AXWebArea",
            "",
            "Meet - abc-defg-hij",
            Some([-1220.0, -1259.0, 1596.0, 1243.0]),
            region.into_iter().chain(std::iter::once(main)).collect(),
        );
        n("AXWindow", "AXStandardWindow", "Meet - Google Chrome", None, vec![n("AXGroup", "", "", None, vec![page])])
    }

    #[test]
    fn own_presentation_is_recognised() {
        let w = meet_window(Some("Alex Morgan (You, presenting, annotating)"), Some([-1203.0, -1065.0, 1563.0, 814.0]));
        let s = detect(&w).unwrap();
        assert_eq!(s.presenter, "Alex Morgan");
        assert!(s.own);
        assert_eq!(s.stage, [-1203.0, -1065.0, 1563.0, 814.0]);
    }

    #[test]
    fn someone_elses_presentation_is_not_own() {
        let w = meet_window(Some("Priya Shah (presenting)"), Some([0.0, 0.0, 1200.0, 700.0]));
        let s = detect(&w).unwrap();
        assert_eq!(s.presenter, "Priya Shah");
        assert!(!s.own);
    }

    #[test]
    fn no_presentation_region_means_no_share() {
        assert!(detect(&meet_window(None, Some([0.0, 0.0, 1200.0, 700.0]))).is_none());
    }

    #[test]
    fn falls_back_to_the_main_area_without_an_image() {
        let w = meet_window(Some("Priya Shah (presenting)"), None);
        assert_eq!(detect(&w).unwrap().stage, [-1204.0, -1195.0, 1564.0, 1091.0]);
    }

    #[test]
    fn small_images_are_ignored_for_the_stage() {
        let w = meet_window(Some("Priya Shah (presenting)"), Some([0.0, 0.0, 40.0, 40.0]));
        assert_eq!(detect(&w).unwrap().stage, [-1204.0, -1195.0, 1564.0, 1091.0]);
    }

    #[test]
    fn other_pages_are_not_meet() {
        let page = n("AXWebArea", "", "Slack", None, vec![]);
        assert!(detect(&n("AXWindow", "", "", None, vec![page])).is_none());
    }

    #[test]
    fn presenter_parsing() {
        assert_eq!(parse_presenter("Alex Kim (presenting)"), Some(("Alex Kim".into(), false)));
        assert_eq!(parse_presenter("Alex Morgan (You, presenting)"), Some(("Alex Morgan".into(), true)));
        assert_eq!(parse_presenter("Meeting details"), None);
    }

    #[test]
    fn marked_tile_gives_its_single_name() {
        let tile = |name: &str, speaking: bool| {
            let mut kids = vec![n("AXStaticText", "", "", None, vec![])];
            kids[0].value = name.into();
            if speaking {
                let mut m = n("AXGroup", "", "", None, vec![]);
                m.desc = "Priya Shah is speaking".into();
                kids.push(m);
            }
            n("AXGroup", "", "", None, kids)
        };
        let page = n(
            "AXWebArea",
            "",
            "Meet - abc",
            None,
            vec![tile("Alex Morgan", false), tile("Sam Lee", false), tile("Priya Shah", true)],
        );
        let w = n("AXWindow", "", "", None, vec![page]);
        // The marker text itself names no one; the tile's own label does.
        assert_eq!(speaking(&w), vec!["Priya Shah".to_string()]);
    }

    #[test]
    fn nothing_marked_means_nobody() {
        let page = n("AXWebArea", "", "Meet - abc", None, vec![]);
        assert!(speaking(&n("AXWindow", "", "", None, vec![page])).is_empty());
    }

    #[test]
    fn interface_words_are_not_names() {
        assert_eq!(name_like("Presentation audio"), None);
        assert_eq!(name_like("4:54"), None);
        assert_eq!(name_like("Alex Morgan (You)"), None, "own tile is never a remote speaker");
        assert_eq!(name_like("Priya Shah").as_deref(), Some("Priya Shah"));
    }
}
