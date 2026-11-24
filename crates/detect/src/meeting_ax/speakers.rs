//! Active-speaker tags read from a meeting window's accessibility tree.
//!
//! Zoom, Teams and Meet announce who is talking to screen readers through
//! labels such as "Sam Lee, speaking" or "Sam Lee is talking". Those labels
//! are a real signal from the meeting platform, so a name read here may name
//! a remote voice. Nothing here guesses: a label that does not match one of
//! the explicit patterns below contributes nothing.
//!
//! The exact wording differs between platform versions and is not documented
//! by the vendors, so the patterns are deliberately generic and every match
//! is filtered through [`plausible_name`]. `examples/meeting_ax_probe.rs`
//! prints the candidate labels of a live call to recalibrate them.

use super::{AxNode, MeetingPlatform, node::node_labels};

/// More simultaneous "speaking" labels than this means the labels are not a
/// live indicator (for example a static legend), so the sample is dropped.
const MAX_SIMULTANEOUS_SPEAKERS: usize = 3;
const MAX_NAME_CHARS: usize = 64;
const MAX_NAME_WORDS: usize = 6;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActiveSpeakers {
    /// Remote people the platform marks as speaking right now, deduplicated,
    /// in tree order.
    pub names: Vec<String>,
    /// The platform marks the local participant ("(You)") as speaking.
    pub self_speaking: bool,
}

/// Words that mark a comma-separated status part as "is speaking".
const SPEAKING_PARTS: &[&str] = &[
    "speaking",
    "talking",
    "is speaking",
    "is talking",
    "currently speaking",
    "active speaker",
    "speaking now",
];

const SPEAKING_SUFFIXES: &[&str] = &[
    " is speaking",
    " is talking",
    " is currently speaking",
    " is the active speaker",
    " (speaking)",
    " (talking)",
    " - speaking",
    " – speaking",
    " speaking now",
];

const SPEAKING_PREFIXES: &[&str] = &[
    "speaking: ",
    "talking: ",
    "now speaking: ",
    "currently speaking: ",
    "active speaker: ",
    "active speaker ",
];

/// Labels that mention speaking but never name a speaker.
const NEGATIVE_MARKERS: &[&str] = &[
    "not speaking",
    "stopped speaking",
    "no one is speaking",
    "nobody is speaking",
    "you are muted",
    "are you talking",
    "speaking time",
    "speaking language",
    "speaker view",
    "active speaker view",
];

/// Words a real participant name does not contain; their presence means the
/// label is UI chrome ("Microphone speaking indicator").
const NON_NAME_WORDS: &[&str] = &[
    "microphone",
    "audio",
    "settings",
    "language",
    "indicator",
    "notification",
    "notifications",
    "view",
    "button",
    "toolbar",
    "meeting",
    "call",
    "speaker",
    "speakers",
    "participants",
    "everyone",
    "someone",
    "unmute",
    "mute",
    "muted",
    "camera",
    "video",
];

const SELF_MARKERS: &[&str] = &["(you)", "(me)", "(myself)"];

/// Parenthetical roles platforms append to a display name.
const ROLE_SUFFIXES: &[&str] = &[
    "(host)",
    "(co-host)",
    "(cohost)",
    "(guest)",
    "(external)",
    "(unverified)",
    "(organizer)",
    "(organiser)",
    "(presenter)",
    "(presentation)",
];

pub(crate) fn active_speakers_from_nodes(
    platform: &MeetingPlatform,
    nodes: &[AxNode],
) -> ActiveSpeakers {
    if matches!(platform, MeetingPlatform::Unknown) {
        return ActiveSpeakers::default();
    }
    let mut result = ActiveSpeakers::default();
    for node in nodes {
        for label in node_labels(node) {
            match speaker_from_label(label) {
                Some(SpeakerLabel::SelfSpeaking) => result.self_speaking = true,
                Some(SpeakerLabel::Remote(name)) => {
                    if !result
                        .names
                        .iter()
                        .any(|existing| existing.eq_ignore_ascii_case(&name))
                    {
                        result.names.push(name);
                    }
                }
                None => {}
            }
        }
    }
    if result.names.len() > MAX_SIMULTANEOUS_SPEAKERS {
        result.names.clear();
    }
    result
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SpeakerLabel {
    SelfSpeaking,
    Remote(String),
}

pub(crate) fn speaker_from_label(label: &str) -> Option<SpeakerLabel> {
    let label = label.trim().replace('\u{2019}', "'");
    if label.is_empty() || label.chars().count() > 200 {
        return None;
    }
    let lower = label.to_lowercase();
    if NEGATIVE_MARKERS.iter().any(|marker| lower.contains(marker)) {
        return None;
    }
    let raw_name = name_before_suffix(&label, &lower)
        .or_else(|| name_after_prefix(&label, &lower))
        .or_else(|| name_from_status_parts(&label))?;
    classify_name(raw_name)
}

// Lowercasing can change a string's byte length for a few scripts; slicing
// the original by the lowercase offsets is only safe when it did not.
fn name_before_suffix<'a>(label: &'a str, lower: &str) -> Option<&'a str> {
    if label.len() != lower.len() {
        return None;
    }
    SPEAKING_SUFFIXES.iter().find_map(|suffix| {
        lower
            .strip_suffix(suffix)
            .and_then(|head| label.get(..head.len()))
    })
}

fn name_after_prefix<'a>(label: &'a str, lower: &str) -> Option<&'a str> {
    if label.len() != lower.len() {
        return None;
    }
    SPEAKING_PREFIXES.iter().find_map(|prefix| {
        lower
            .starts_with(prefix)
            .then(|| label.get(prefix.len()..))
            .flatten()
    })
}

/// "Sam Lee, Video on, Speaking": the first part is the name, any later part
/// must be exactly a speaking marker.
fn name_from_status_parts(label: &str) -> Option<&str> {
    let parts = split_outside_parens(label);
    let (name, rest) = parts.split_first()?;
    rest.iter()
        .any(|part| SPEAKING_PARTS.contains(&part.to_lowercase().as_str()))
        .then_some(*name)
}

/// Splits on commas that are not inside parentheses, so "Ali (Host, me)"
/// stays one part.
fn split_outside_parens(label: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    for (index, ch) in label.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(label[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push(label[start..].trim());
    parts
}

fn classify_name(raw: &str) -> Option<SpeakerLabel> {
    let mut name = raw
        .trim()
        .trim_end_matches([',', '.', ':', ';'])
        .trim()
        .to_string();
    let lower = name.to_lowercase();
    if matches!(lower.as_str(), "you" | "me" | "myself") {
        return Some(SpeakerLabel::SelfSpeaking);
    }
    let mut is_self = false;
    loop {
        let lower = name.to_lowercase();
        let Some(open) = lower.rfind(" (").filter(|_| lower.ends_with(')')) else {
            break;
        };
        let tail = &lower[open + 1..];
        let parts = tail
            .trim_start_matches('(')
            .trim_end_matches(')')
            .split(',')
            .map(|part| format!("({})", part.trim()))
            .collect::<Vec<_>>();
        if parts
            .iter()
            .any(|part| SELF_MARKERS.contains(&part.as_str()))
        {
            is_self = true;
        } else if !parts
            .iter()
            .all(|part| ROLE_SUFFIXES.contains(&part.as_str()))
        {
            break;
        }
        name.truncate(open);
        name = name.trim().to_string();
    }
    if is_self {
        return Some(SpeakerLabel::SelfSpeaking);
    }
    plausible_name(&name).then(|| SpeakerLabel::Remote(name))
}

pub(crate) fn plausible_name(name: &str) -> bool {
    let chars = name.chars().count();
    if chars < 2 || chars > MAX_NAME_CHARS {
        return false;
    }
    if !name.chars().any(char::is_alphabetic) {
        return false;
    }
    if name.contains([':', '\n', '/', '\\', '@', '|']) {
        return false;
    }
    let words: Vec<String> = name
        .split_whitespace()
        .map(|word| {
            word.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .collect();
    if words.is_empty() || words.len() > MAX_NAME_WORDS {
        return false;
    }
    !words
        .iter()
        .any(|word| NON_NAME_WORDS.contains(&word.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remote(label: &str) -> Option<String> {
        match speaker_from_label(label) {
            Some(SpeakerLabel::Remote(name)) => Some(name),
            _ => None,
        }
    }

    #[test]
    fn reads_common_speaking_label_shapes() {
        assert_eq!(remote("Sam Lee is speaking").as_deref(), Some("Sam Lee"));
        assert_eq!(remote("Sam Lee is talking").as_deref(), Some("Sam Lee"));
        assert_eq!(remote("Sam Lee, speaking").as_deref(), Some("Sam Lee"));
        assert_eq!(
            remote("Sam Lee, Video on, Unmuted, Speaking").as_deref(),
            Some("Sam Lee")
        );
        assert_eq!(remote("Sam Lee (speaking)").as_deref(), Some("Sam Lee"));
        assert_eq!(remote("Speaking: Sam Lee").as_deref(), Some("Sam Lee"));
        assert_eq!(
            remote("Active speaker: Sam Lee").as_deref(),
            Some("Sam Lee")
        );
        assert_eq!(
            remote("María José is speaking").as_deref(),
            Some("María José")
        );
    }

    #[test]
    fn strips_roles_and_detects_the_local_participant() {
        assert_eq!(
            remote("Sam Lee (Host) is speaking").as_deref(),
            Some("Sam Lee")
        );
        assert_eq!(
            remote("Sam Lee (Guest), speaking").as_deref(),
            Some("Sam Lee")
        );
        assert_eq!(
            speaker_from_label("Ali Aunas (You) is speaking"),
            Some(SpeakerLabel::SelfSpeaking)
        );
        assert_eq!(
            speaker_from_label("Ali Aunas (Host, me), speaking"),
            Some(SpeakerLabel::SelfSpeaking)
        );
        assert_eq!(
            speaker_from_label("You are speaking"),
            None,
            "not a recognised shape; never a name"
        );
        assert_eq!(
            speaker_from_label("You, speaking"),
            Some(SpeakerLabel::SelfSpeaking)
        );
    }

    #[test]
    fn ignores_chrome_and_negative_labels() {
        for label in [
            "Speaker view",
            "Active speaker view",
            "Microphone, speaking indicator, speaking",
            "Sam Lee is not speaking",
            "Sam Lee stopped speaking",
            "No one is speaking",
            "Speaking language: English",
            "Mute my audio",
            "Speakers, speaking",
            "Leave meeting",
            "Sam Lee",
            "Sam Lee, Video on, Muted",
            "https://meet.example/abc is speaking",
            "1234 is speaking",
            "This is a much longer sentence that keeps going and going is speaking",
        ] {
            assert_eq!(remote(label), None, "{label}");
        }
    }

    fn node(description: &str) -> AxNode {
        AxNode {
            index: 0,
            tree_path: Vec::new(),
            element_hash: None,
            role: Some("AXGroup".into()),
            identifier: None,
            title: None,
            value: None,
            description: Some(description.into()),
            placeholder: None,
            enabled: None,
            settable_value: false,
            bounds: None,
            text: description.to_lowercase(),
            within_zoom_meeting_scope: false,
            within_zoom_chat_scope: false,
            within_slack_huddle_scope: false,
        }
    }

    #[test]
    fn collects_distinct_speakers_from_a_window() {
        let nodes = vec![
            node("Sam Lee, speaking"),
            node("sam lee is speaking"),
            node("Ali Aunas (You), speaking"),
            node("Priya Raman, Video on, Muted"),
        ];
        let speakers = active_speakers_from_nodes(&MeetingPlatform::Zoom, &nodes);
        assert_eq!(speakers.names, vec!["Sam Lee".to_string()]);
        assert!(speakers.self_speaking);
    }

    #[test]
    fn too_many_simultaneous_speakers_is_not_a_live_indicator() {
        let nodes = ["A One", "B Two", "C Three", "D Four"]
            .iter()
            .map(|name| node(&format!("{name}, speaking")))
            .collect::<Vec<_>>();
        let speakers = active_speakers_from_nodes(&MeetingPlatform::MicrosoftTeams, &nodes);
        assert!(speakers.names.is_empty());
    }

    #[test]
    fn unknown_platform_reads_nothing() {
        let nodes = vec![node("Sam Lee, speaking")];
        assert_eq!(
            active_speakers_from_nodes(&MeetingPlatform::Unknown, &nodes),
            ActiveSpeakers::default()
        );
    }
}
