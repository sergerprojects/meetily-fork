//! Turning "who the call app said was speaking, and when" into names on transcript lines.
//!
//! The call app (Meet, Zoom, Teams) tells us who is speaking at each moment. We
//! collapse those samples into spans, then give every transcript line the name
//! that was speaking for most of it. Lines already named by a person are left alone.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct Sample {
    /// Seconds into the audio stream.
    pub t: f64,
    /// Names the app showed as speaking at that moment (usually one).
    pub speaking: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Span {
    pub start: f64,
    pub end: f64,
    pub name: String,
}

/// A sample only vouches for this long after it was taken.
const SAMPLE_REACH: f64 = 1.0;
/// Two spans of one name closer than this are one span.
const MERGE_GAP: f64 = 1.2;

/// Samples (in time order) to spans. Each sample covers up to the next sample, at
/// most `SAMPLE_REACH` seconds, so a gap in sampling never invents speech.
pub fn collapse(samples: &[Sample]) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();
    for (i, s) in samples.iter().enumerate() {
        let next_t = samples.get(i + 1).map(|n| n.t).unwrap_or(s.t + SAMPLE_REACH);
        let end = next_t.min(s.t + SAMPLE_REACH).max(s.t);
        for name in &s.speaking {
            match spans.iter_mut().rev().find(|sp| &sp.name == name) {
                Some(sp) if s.t - sp.end <= MERGE_GAP => sp.end = sp.end.max(end),
                _ => spans.push(Span { start: s.t, end, name: name.clone() }),
            }
        }
    }
    spans.sort_by(|a, b| a.start.total_cmp(&b.start));
    spans
}

fn overlap(a0: f64, a1: f64, b0: f64, b1: f64) -> f64 {
    (a1.min(b1) - a0.max(b0)).max(0.0)
}

/// Name that was speaking for most of [start, end], if it covers at least `min_share`
/// of the line and clearly beats the runner-up.
pub fn name_for(spans: &[Span], start: f64, end: f64, min_share: f64) -> Option<String> {
    let dur = (end - start).max(0.001);
    let mut by_name: HashMap<&str, f64> = HashMap::new();
    for sp in spans {
        let o = overlap(start, end, sp.start, sp.end);
        if o > 0.0 {
            *by_name.entry(sp.name.as_str()).or_default() += o;
        }
    }
    let mut ranked: Vec<(&str, f64)> = by_name.into_iter().collect();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
    let (best, best_o) = *ranked.first()?;
    if best_o / dur < min_share {
        return None;
    }
    if let Some((_, second)) = ranked.get(1) {
        if *second > best_o * 0.6 {
            return None; // two people talking over each other: do not guess
        }
    }
    Some(best.to_string())
}

#[derive(Debug, Clone)]
pub struct Line {
    pub id: String,
    pub start: f64,
    pub end: f64,
    pub speaker: String,
}

/// Lines whose current label is a placeholder ("Others", "Speaker N", empty).
pub fn is_placeholder(label: &str) -> bool {
    let l = label.trim();
    l.is_empty() || l == "Others" || l.starts_with("Speaker ")
}

/// New names for placeholder lines, as (line id, name). Lines labelled "You" or by a
/// real name are never touched. `skip` lists names that must not be given out (for
/// example your own name: your lines come from the microphone).
pub fn assign(lines: &[Line], spans: &[Span], skip: &[String]) -> Vec<(String, String)> {
    lines
        .iter()
        .filter(|l| is_placeholder(&l.speaker))
        .filter_map(|l| {
            let name = name_for(spans, l.start, l.end, 0.5)?;
            (!skip.iter().any(|s| s.eq_ignore_ascii_case(&name))).then(|| (l.id.clone(), name))
        })
        .collect()
}

/// For each voice label ("Speaker 2"), the name that matches most of its lines, when
/// at least `min_agree` of the lines that got a name agree. Used to name a whole
/// voice (and fill the "who is who" picker) from the lines we could match.
pub fn voice_names(lines: &[Line], named: &[(String, String)], min_agree: f64) -> HashMap<String, String> {
    let by_id: HashMap<&str, &str> = named.iter().map(|(i, n)| (i.as_str(), n.as_str())).collect();
    let mut per_label: HashMap<&str, HashMap<&str, usize>> = HashMap::new();
    for l in lines.iter().filter(|l| l.speaker.starts_with("Speaker ")) {
        if let Some(name) = by_id.get(l.id.as_str()) {
            *per_label.entry(l.speaker.as_str()).or_default().entry(name).or_default() += 1;
        }
    }
    per_label
        .into_iter()
        .filter_map(|(label, counts)| {
            let total: usize = counts.values().sum();
            let (name, n) = counts.into_iter().max_by_key(|(_, n)| *n)?;
            (total >= 2 && n as f64 / total as f64 >= min_agree).then(|| (label.to_string(), name.to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn samp(t: f64, who: &[&str]) -> Sample {
        Sample { t, speaking: who.iter().map(|s| s.to_string()).collect() }
    }
    fn line(id: &str, a: f64, b: f64, who: &str) -> Line {
        Line { id: id.into(), start: a, end: b, speaker: who.into() }
    }

    #[test]
    fn consecutive_samples_of_one_speaker_become_one_span() {
        let spans = collapse(&[samp(0.0, &["Priya"]), samp(0.5, &["Priya"]), samp(1.0, &["Priya"]), samp(1.5, &["Sam"])]);
        assert_eq!(spans.len(), 2);
        assert_eq!((spans[0].name.as_str(), spans[0].start, spans[0].end), ("Priya", 0.0, 1.5));
        assert_eq!(spans[1].name, "Sam");
    }

    #[test]
    fn a_sampling_gap_does_not_invent_speech() {
        let spans = collapse(&[samp(0.0, &["Priya"]), samp(30.0, &["Priya"])]);
        assert_eq!(spans.len(), 2, "30s apart is two separate moments");
        assert!(spans[0].end <= 1.0);
    }

    #[test]
    fn short_pauses_merge() {
        let spans = collapse(&[samp(0.0, &["Priya"]), samp(0.5, &[]), samp(1.0, &["Priya"]), samp(1.5, &["Priya"])]);
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn line_gets_the_name_that_covered_most_of_it() {
        let spans = vec![
            Span { start: 0.0, end: 3.0, name: "Priya".into() },
            Span { start: 3.0, end: 3.5, name: "Sam".into() },
        ];
        assert_eq!(name_for(&spans, 0.5, 3.5, 0.5).as_deref(), Some("Priya"));
    }

    #[test]
    fn talking_over_each_other_is_not_guessed() {
        let spans = vec![
            Span { start: 0.0, end: 2.0, name: "Priya".into() },
            Span { start: 0.0, end: 1.8, name: "Sam".into() },
        ];
        assert_eq!(name_for(&spans, 0.0, 2.0, 0.5), None);
    }

    #[test]
    fn too_little_coverage_gives_no_name() {
        let spans = vec![Span { start: 0.0, end: 1.0, name: "Priya".into() }];
        assert_eq!(name_for(&spans, 0.0, 10.0, 0.5), None);
    }

    #[test]
    fn only_placeholder_lines_are_renamed() {
        let spans = vec![Span { start: 0.0, end: 10.0, name: "Priya".into() }];
        let lines = vec![
            line("a", 0.0, 2.0, "Others"),
            line("b", 2.0, 4.0, "You"),
            line("c", 4.0, 6.0, "Speaker 2"),
            line("d", 6.0, 8.0, "Alex Kim"),
        ];
        let got = assign(&lines, &spans, &[]);
        assert_eq!(got, vec![("a".into(), "Priya".into()), ("c".into(), "Priya".into())]);
    }

    #[test]
    fn your_own_name_is_never_given_to_the_other_side() {
        let spans = vec![Span { start: 0.0, end: 10.0, name: "Alex Morgan".into() }];
        let lines = vec![line("a", 0.0, 2.0, "Others")];
        assert!(assign(&lines, &spans, &["alex morgan".into()]).is_empty());
    }

    #[test]
    fn a_voice_is_named_only_when_its_lines_agree() {
        let lines = vec![
            line("1", 0.0, 1.0, "Speaker 1"),
            line("2", 1.0, 2.0, "Speaker 1"),
            line("3", 2.0, 3.0, "Speaker 1"),
            line("4", 3.0, 4.0, "Speaker 2"),
            line("5", 4.0, 5.0, "Speaker 2"),
        ];
        let named = vec![
            ("1".into(), "Priya".into()),
            ("2".into(), "Priya".into()),
            ("3".into(), "Priya".into()),
            ("4".into(), "Sam".into()),
            ("5".into(), "Priya".into()),
        ];
        let m = voice_names(&lines, &named, 0.7);
        assert_eq!(m.get("Speaker 1").map(String::as_str), Some("Priya"));
        assert!(!m.contains_key("Speaker 2"), "50/50 is not enough");
    }
}
