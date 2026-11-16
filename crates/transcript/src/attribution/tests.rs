use serde_json::json;

use super::*;
use crate::{RenderTranscriptRequest, render_input_from_stored, render_transcript_segments};

fn word(id: &str, start: f64, channel: f64) -> StoredTranscriptWord {
    StoredTranscriptWord {
        id: id.into(),
        text: Some(format!(" {id}")),
        start_ms: Some(start),
        end_ms: Some(start + 200.0),
        channel: Some(channel),
        speaker: None,
        metadata: None,
    }
}

fn hint(word_id: &str, kind: &str, value: Value) -> StoredSpeakerHint {
    StoredSpeakerHint {
        id: format!("{word_id}:{kind}"),
        word_id: Some(word_id.into()),
        hint_type: kind.into(),
        value,
    }
}

fn render(
    words: &[StoredTranscriptWord],
    hints: &[StoredSpeakerHint],
    humans: &[(&str, &str)],
) -> Vec<RenderedTranscriptSegment> {
    render_transcript_segments(RenderTranscriptRequest {
        speaker_context: None,
        preview: None,
        transcripts: vec![render_input_from_stored(Some(0), words, hints).unwrap()],
        participant_human_ids: vec![],
        self_human_id: Some("me".into()),
        humans: humans
            .iter()
            .map(|(id, name)| RenderTranscriptHuman {
                human_id: (*id).into(),
                name: (*name).into(),
            })
            .collect(),
    })
}

fn pass_word(id: &str, channel: i32, speaker: Option<i32>, uncertain: bool) -> SpeakerPassWord {
    SpeakerPassWord {
        word_id: id.into(),
        channel,
        speaker_index: speaker,
        confidence: if uncertain { 0.2 } else { 0.9 },
        uncertain,
    }
}

#[test]
fn old_format_transcripts_render_with_derived_sources() {
    // "You"-era data: object and double-encoded values, no source,
    // confidence or uncertainty fields anywhere.
    let words = vec![
        word("m1", 0.0, 0.0),
        word("r1", 1000.0, 1.0),
        word("r2", 2000.0, 1.0),
        word("r3", 3000.0, 1.0),
        word("r4", 4000.0, 1.0),
    ];
    let hints =
        vec![
        hint("r1", "provider_speaker_index", json!({"speaker_index": 0, "channel": 1})),
        hint(
            "r2",
            "provider_speaker_index",
            Value::String(json!({"speaker_index": 1, "channel": 1}).to_string()),
        ),
        hint("r3", "provider_speaker_index", json!({"speaker_index": 2, "channel": 1})),
        hint("r4", "provider_speaker_index", json!({"speaker_index": 3, "channel": 1})),
        hint(
            "r2",
            "automatic_speaker_assignment",
            Value::String(json!({"human_id": "marco"}).to_string()),
        ),
        hint(
            "r3",
            "user_speaker_assignment",
            Value::String(
                json!({"human_id": "ana", "scope": "speaker", "channel": 1, "speaker_index": 2})
                    .to_string(),
            ),
        ),
    ];
    let segments = render(&words, &hints, &[("marco", "Marco"), ("ana", "Ana")]);
    let by_label: HashMap<&str, &RenderedTranscriptSegment> = segments
        .iter()
        .map(|s| (s.speaker_label.as_str(), s))
        .collect();

    assert_eq!(
        by_label["Me"].attribution.source,
        AttributionSource::Channel
    );
    assert_eq!(by_label["Me"].speaker_id, "channel:0");
    assert_eq!(
        by_label["Speaker A"].attribution.source,
        AttributionSource::Diarization
    );
    assert_eq!(by_label["Speaker A"].speaker_id, "diar:1:0");
    assert_eq!(
        by_label["Marco"].attribution.source,
        AttributionSource::Voiceprint
    );
    assert_eq!(by_label["Marco"].speaker_id, "marco");
    assert_eq!(
        by_label["Ana"].attribution.source,
        AttributionSource::Manual
    );
    // Letters count every remote cluster, named ones included.
    assert_eq!(
        by_label["Speaker D"].attribution.source,
        AttributionSource::Diarization
    );
    assert!(segments.iter().all(|s| !s.attribution.uncertain));
    assert!(segments.iter().all(|s| s.attribution.confidence.is_none()));
}

#[test]
fn render_without_raw_hints_falls_back_to_the_segment_key() {
    let words = vec![word("m1", 0.0, 0.0), word("r1", 1000.0, 1.0)];
    let mut input = render_input_from_stored(Some(0), &words, &[]).unwrap();
    input.speaker_hints = None;
    let segments = render_transcript_segments(RenderTranscriptRequest {
        speaker_context: None,
        preview: None,
        transcripts: vec![input],
        participant_human_ids: vec![],
        self_human_id: None,
        humans: vec![],
    });
    assert_eq!(segments[0].speaker_label, "Me");
    assert_eq!(segments[1].speaker_label, "Them");
    assert!(
        segments
            .iter()
            .all(|s| s.attribution.source == AttributionSource::Channel)
    );
}

#[test]
fn pass_hints_carry_source_confidence_and_uncertainty() {
    let words = vec![
        word("a", 0.0, 0.0),
        word("b", 300.0, 0.0),
        word("c", 600.0, 0.0),
        word("d", 900.0, 0.0),
    ];
    let pass = SpeakerPassHints {
        words: vec![
            pass_word("a", 2, Some(0), false),
            pass_word("b", 2, Some(0), true),
            pass_word("c", 2, Some(1), false),
            pass_word("d", 2, Some(1), false),
        ],
        assignments: vec![SpeakerPassAssignment {
            channel: 2,
            speaker_index: 0,
            human_id: "me".into(),
            confidence: 0.91,
        }],
    };
    let hints = apply_speaker_pass_hints(&[], &pass);
    let segments = render(&words, &hints, &[("me", "Alex")]);
    assert_eq!(segments.len(), 2);
    assert_eq!(segments[0].speaker_label, "Alex");
    assert_eq!(
        segments[0].attribution.source,
        AttributionSource::Voiceprint
    );
    assert!((segments[0].attribution.confidence.unwrap() - 0.91).abs() < 1e-3);
    assert_eq!(
        segments[0].attribution.uncertain_word_ids,
        vec!["b".to_string()]
    );
    // Half of the segment's words uncertain -> the segment is uncertain.
    assert!(segments[0].attribution.uncertain);
    assert_eq!(segments[1].speaker_label, "Speaker 2");
    assert_eq!(
        segments[1].attribution.source,
        AttributionSource::Diarization
    );
    assert!(!segments[1].attribution.uncertain);
    assert_eq!(segments[1].speaker_id, "diar:2:1");
}

#[test]
fn rerunning_the_pass_replaces_its_hints_and_keeps_manual_ones() {
    let words = vec![
        word("a", 0.0, 1.0),
        word("b", 300.0, 1.0),
        word("c", 600.0, 1.0),
    ];
    let first = apply_speaker_pass_hints(
        &[],
        &SpeakerPassHints {
            words: vec![
                pass_word("a", 1, Some(0), false),
                pass_word("b", 1, Some(1), false),
                pass_word("c", 1, Some(1), false),
            ],
            assignments: vec![],
        },
    );
    let mut renamed = first.clone();
    renamed.push(hint(
        "b",
        "user_speaker_assignment",
        Value::String(
            json!({"human_id": "bob", "scope": "speaker", "channel": 1, "speaker_index": 1})
                .to_string(),
        ),
    ));
    let locked = manually_assigned_word_ids(&words, &renamed);
    assert_eq!(locked, HashSet::from(["b".to_string(), "c".to_string()]));

    // The rerun only sees word "a" and clusters it as 1 this time: shifted
    // past the manually named index so it never inherits "Bob".
    let second = apply_speaker_pass_hints(
        &renamed,
        &SpeakerPassHints {
            words: vec![pass_word("a", 1, Some(1), false)],
            assignments: vec![],
        },
    );
    assert_eq!(
        second
            .iter()
            .filter(|h| h.hint_type == "user_speaker_assignment")
            .count(),
        1
    );
    assert_eq!(
        second
            .iter()
            .filter(|h| h.hint_type == "provider_speaker_index")
            .count(),
        3
    );
    let segments = render(&words, &second, &[("bob", "Bob")]);
    let labels: Vec<_> = segments.iter().map(|s| s.speaker_label.as_str()).collect();
    assert_eq!(labels, vec!["Speaker A", "Bob"]);
    assert_eq!(segments[1].attribution.source, AttributionSource::Manual);
    // Idempotent: applying the same result again changes nothing.
    let again = apply_speaker_pass_hints(
        &second,
        &SpeakerPassHints {
            words: vec![pass_word("a", 1, Some(1), false)],
            assignments: vec![],
        },
    );
    assert_eq!(again, second);
}

#[test]
fn single_voice_system_audio_drops_stale_provider_indices() {
    let words = vec![word("a", 0.0, 1.0), word("b", 300.0, 1.0)];
    let stale = vec![
        hint(
            "a",
            "provider_speaker_index",
            json!({"speaker_index": 0, "channel": 1}),
        ),
        hint(
            "b",
            "provider_speaker_index",
            json!({"speaker_index": 3, "channel": 1}),
        ),
    ];
    let hints = apply_speaker_pass_hints(
        &stale,
        &SpeakerPassHints {
            words: vec![
                pass_word("a", 1, None, false),
                pass_word("b", 1, None, false),
            ],
            assignments: vec![],
        },
    );
    assert!(hints.is_empty());
    let segments = render(&words, &hints, &[]);
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].speaker_label, "Them");
    assert_eq!(segments[0].attribution.source, AttributionSource::Channel);
}

#[test]
fn renaming_a_cluster_relabels_all_its_segments() {
    let words = vec![
        word("a", 0.0, 0.0),
        word("b", 1000.0, 0.0),
        word("c", 2000.0, 0.0),
        word("d", 3000.0, 0.0),
    ];
    let mut hints = apply_speaker_pass_hints(
        &[],
        &SpeakerPassHints {
            words: vec![
                pass_word("a", 2, Some(0), false),
                pass_word("b", 2, Some(1), false),
                pass_word("c", 2, Some(0), false),
                pass_word("d", 2, Some(1), false),
            ],
            assignments: vec![],
        },
    );
    let before = render(&words, &hints, &[]);
    assert_eq!(
        before
            .iter()
            .map(|s| s.speaker_label.as_str())
            .collect::<Vec<_>>(),
        vec!["Speaker 1", "Speaker 2", "Speaker 1", "Speaker 2"]
    );
    hints.push(hint(
        "b",
        "user_speaker_assignment",
        Value::String(
            json!({"human_id": "dana", "scope": "speaker", "channel": 2, "speaker_index": 1})
                .to_string(),
        ),
    ));
    let after = render(&words, &hints, &[("dana", "Dana")]);
    assert_eq!(
        after
            .iter()
            .map(|s| s.speaker_label.as_str())
            .collect::<Vec<_>>(),
        vec!["Speaker 1", "Dana", "Speaker 1", "Dana"]
    );
}

#[test]
fn letters_extend_past_z() {
    assert_eq!(letters(0), "A");
    assert_eq!(letters(25), "Z");
    assert_eq!(letters(26), "AA");
    assert_eq!(letters(27), "AB");
}
