//! Speaker-attribution harness on synthetic recordings.
//!
//! Voices come from macOS `say` (generated at test time into a cache dir and
//! reused), so these tests are `#[ignore]`d and run explicitly on a Mac:
//!
//! ```sh
//! cargo test -p listener2-core --test speaker_attribution -- --ignored --nocapture --test-threads=1
//! ```
//!
//! Word timings are constructed from the known utterance placement (STT is
//! not under test); the real diarizer, embedding model, voiceprint matching,
//! hint writer and renderer run end to end.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use anlg_transcript::{
    AttributionSource, RenderTranscriptHuman, RenderTranscriptRequest, RenderedTranscriptSegment,
    StoredSpeakerHint, StoredTranscriptWord, apply_speaker_pass_hints, render_input_from_stored,
    render_transcript_segments,
};
use listener2_core::speaker_pass::{
    PlatformSpeakerSpan, SELF_MATCH_MIN_SCORE, SelfVoiceprint, SpeakerPassInputWord,
    SpeakerPassMode, SpeakerPassOutcome, SpeakerPassRequest, load_mono_16k, platform_assignments,
    run_speaker_pass, voice_embedding,
};

const RATE: usize = 16_000;
const USER_VOICE: &str = "Daniel";
const OTHER_VOICE: &str = "Karen";
const THIRD_VOICE: &str = "Ralph";
const IMPOSTOR_VOICE: &str = "Moira";
const SELF_ID: &str = "self-human";

const USER_LINES: &[&str] = &[
    "Good morning everyone, thanks for joining the planning call today.",
    "I think we should start with the budget review and then move on to hiring.",
    "That makes sense to me, let us lock the numbers by Friday afternoon.",
    "I can take the action item to send the summary to the leadership team.",
    "Great, I will also schedule a follow up meeting for next Tuesday morning.",
];
const OTHER_LINES: &[&str] = &[
    "Thanks for having me, I have a few updates on the marketing launch.",
    "The campaign assets are ready but the landing page still needs review.",
    "We are about two weeks behind on the partner announcements this quarter.",
    "I would like more support from design before the end of the month.",
    "Sounds good, I will share the updated timeline in the channel later.",
];
const THIRD_LINES: &[&str] = &[
    "From the engineering side the release candidate is almost finished.",
    "We still have three open bugs that block the mobile build right now.",
    "I expect the fixes to land sometime early next week if testing goes well.",
    "Please send me the list of features that marketing wants to highlight.",
    "I can join the follow up meeting, just add me to the invitation please.",
];
const ENROLL_TEXT: &str = "My voice is my passport. I am reading this paragraph so the app can \
    learn how I sound. The quick brown fox jumps over the lazy dog while the orchestra tunes \
    their instruments. Yesterday I walked to the library, borrowed three books about \
    astronomy, and spent the evening reading about distant galaxies and the life of stars. \
    Tomorrow I plan to cook dinner for my family and maybe watch a documentary about the ocean.";

fn fixture_dir() -> PathBuf {
    let dir = std::env::temp_dir().join("anarlog-speaker-fixtures-v1");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn slug(text: &str) -> String {
    let mut hash: u64 = 1469598103934665603;
    for byte in text.bytes() {
        hash = (hash ^ u64::from(byte)).wrapping_mul(1099511628211);
    }
    format!("{hash:016x}")
}

/// 16 kHz mono speech for `text` in `voice`, leading/trailing silence trimmed.
fn tts(voice: &str, text: &str) -> Vec<f32> {
    let dir = fixture_dir();
    let wav = dir.join(format!("{voice}-{}.wav", slug(text)));
    if !wav.exists() {
        let aiff = dir.join(format!("{voice}-{}.aiff", slug(text)));
        let status = Command::new("say")
            .args(["-v", voice, "-o"])
            .arg(&aiff)
            .arg(text)
            .status()
            .expect("macOS `say` is required for this harness");
        assert!(status.success(), "say failed for {voice}");
        let status = Command::new("afconvert")
            .args(["-f", "WAVE", "-d", "LEI16@16000", "-c", "1"])
            .arg(&aiff)
            .arg(&wav)
            .status()
            .expect("afconvert is required for this harness");
        assert!(status.success());
        let _ = std::fs::remove_file(aiff);
    }
    let mut reader = hound::WavReader::open(&wav).unwrap();
    let samples: Vec<f32> = reader
        .samples::<i16>()
        .map(|s| f32::from(s.unwrap()) / 32768.0)
        .collect();
    let threshold = 0.01;
    let first = samples
        .iter()
        .position(|s| s.abs() > threshold)
        .unwrap_or(0);
    let last = samples
        .iter()
        .rposition(|s| s.abs() > threshold)
        .unwrap_or(samples.len().saturating_sub(1));
    samples[first..=last].to_vec()
}

#[derive(Debug, Clone)]
struct TruthWord {
    id: String,
    text: String,
    start_ms: f64,
    end_ms: f64,
    channel: i32,
    speaker: String,
}

#[derive(Default)]
struct Track {
    samples: Vec<f32>,
}

impl Track {
    fn place(&mut self, at: usize, audio: &[f32]) {
        if self.samples.len() < at + audio.len() {
            self.samples.resize(at + audio.len(), 0.0);
        }
        for (slot, sample) in self.samples[at..].iter_mut().zip(audio) {
            *slot += sample * 0.8;
        }
    }
}

struct Scene {
    tracks: Vec<Track>,
    words: Vec<TruthWord>,
    cursor: usize,
    next_id: usize,
}

impl Scene {
    fn new(channels: usize) -> Self {
        Self {
            tracks: (0..channels).map(|_| Track::default()).collect(),
            words: Vec::new(),
            cursor: RATE / 2,
            next_id: 0,
        }
    }

    fn words_for(
        &mut self,
        text: &str,
        at: usize,
        len: usize,
        channel: i32,
        speaker: &str,
    ) -> Vec<TruthWord> {
        let tokens: Vec<&str> = text.split_whitespace().collect();
        let start_ms = at as f64 * 1000.0 / RATE as f64;
        let duration_ms = len as f64 * 1000.0 / RATE as f64;
        let step = duration_ms / tokens.len() as f64;
        tokens
            .iter()
            .enumerate()
            .map(|(index, token)| {
                self.next_id += 1;
                TruthWord {
                    id: format!("w{}", self.next_id),
                    text: format!(" {token}"),
                    start_ms: start_ms + step * index as f64,
                    end_ms: start_ms + step * (index as f64 + 1.0) - step * 0.1,
                    channel,
                    speaker: speaker.to_owned(),
                }
            })
            .collect()
    }

    /// Next turn on `track`, after the previous one plus a short pause.
    fn say(&mut self, track: usize, channel: i32, voice: &str, text: &str) {
        let audio = tts(voice, text);
        let at = self.cursor;
        self.tracks[track].place(at, &audio);
        let words = self.words_for(text, at, audio.len(), channel, voice);
        self.words.extend(words);
        self.cursor = at + audio.len() + RATE * 6 / 10;
    }

    /// Two voices at once on `track`, both starting at the cursor.
    fn overlap(&mut self, track: usize, channel: i32, a: (&str, &str), b: (&str, &str)) {
        let audio_a = tts(a.0, a.1);
        let audio_b = tts(b.0, b.1);
        let at = self.cursor;
        self.tracks[track].place(at, &audio_a);
        self.tracks[track].place(at, &audio_b);
        let mut words = self.words_for(a.1, at, audio_a.len(), channel, a.0);
        words.extend(self.words_for(b.1, at, audio_b.len(), channel, b.0));
        for word in &mut words {
            word.speaker = "overlap".to_owned();
        }
        self.words.extend(words);
        self.cursor = at + audio_a.len().max(audio_b.len()) + RATE * 6 / 10;
    }

    fn write(&self, name: &str) -> PathBuf {
        let path = fixture_dir().join(format!("{name}.wav"));
        let len = self
            .tracks
            .iter()
            .map(|track| track.samples.len())
            .max()
            .unwrap_or(0)
            + RATE / 2;
        let spec = hound::WavSpec {
            channels: self.tracks.len() as u16,
            sample_rate: RATE as u32,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        for index in 0..len {
            for track in &self.tracks {
                let sample = track.samples.get(index).copied().unwrap_or(0.0);
                writer
                    .write_sample((sample.clamp(-1.0, 1.0) * 32767.0) as i16)
                    .unwrap();
            }
        }
        writer.finalize().unwrap();
        path
    }
}

fn input_words(words: &[TruthWord]) -> Vec<SpeakerPassInputWord> {
    words
        .iter()
        .map(|word| SpeakerPassInputWord {
            id: word.id.clone(),
            start_ms: word.start_ms,
            end_ms: word.end_ms,
            channel: word.channel,
        })
        .collect()
}

fn stored_words(words: &[TruthWord]) -> Vec<StoredTranscriptWord> {
    words
        .iter()
        .map(|word| StoredTranscriptWord {
            id: word.id.clone(),
            text: Some(word.text.clone()),
            start_ms: Some(word.start_ms.round()),
            end_ms: Some(word.end_ms.round()),
            channel: Some(f64::from(word.channel)),
            speaker: None,
            metadata: None,
        })
        .collect()
}

fn pass(
    path: &Path,
    words: &[TruthWord],
    mode: SpeakerPassMode,
    print: Option<&SelfVoiceprint>,
) -> SpeakerPassOutcome {
    pass_with_spans(path, words, mode, print, &[])
}

fn pass_with_spans(
    path: &Path,
    words: &[TruthWord],
    mode: SpeakerPassMode,
    print: Option<&SelfVoiceprint>,
    platform_spans: &[PlatformSpeakerSpan],
) -> SpeakerPassOutcome {
    let input = input_words(words);
    let never = || false;
    let outcome = run_speaker_pass(&SpeakerPassRequest {
        audio_path: path,
        words: &input,
        mode,
        self_voiceprint: print,
        platform_spans,
        is_cancelled: &never,
    })
    .expect("speaker pass runs");
    println!(
        "pass mode={:?} speakers={} channels={:?} self_score={:?} cluster_scores={:?} matched={} uncertain={} skipped={:?}",
        outcome.mode,
        outcome.speaker_count,
        outcome.diarized_channels,
        outcome.self_match_score,
        outcome.cluster_self_scores,
        outcome.self_matched,
        outcome.uncertain_words,
        outcome.skipped_reason
    );
    outcome
}

fn render(
    words: &[TruthWord],
    hints: &[StoredSpeakerHint],
    self_name: Option<&str>,
    extra_humans: &[(&str, &str)],
) -> Vec<RenderedTranscriptSegment> {
    let stored = stored_words(words);
    let input = render_input_from_stored(Some(0), &stored, hints).unwrap();
    let mut humans: Vec<RenderTranscriptHuman> = extra_humans
        .iter()
        .map(|(id, name)| RenderTranscriptHuman {
            human_id: (*id).to_owned(),
            name: (*name).to_owned(),
        })
        .collect();
    if let Some(name) = self_name {
        humans.push(RenderTranscriptHuman {
            human_id: SELF_ID.to_owned(),
            name: name.to_owned(),
        });
    }
    render_transcript_segments(RenderTranscriptRequest {
        speaker_context: None,
        preview: None,
        transcripts: vec![input],
        participant_human_ids: vec![],
        self_human_id: Some(SELF_ID.to_owned()),
        humans,
    })
}

/// Label each truth word received, by word id.
fn labels_by_word(
    segments: &[RenderedTranscriptSegment],
) -> HashMap<String, (String, &RenderedTranscriptSegment)> {
    let mut map = HashMap::new();
    for segment in segments {
        for word in &segment.words {
            if let Some(id) = &word.id {
                map.insert(id.clone(), (segment.speaker_label.clone(), segment));
            }
        }
    }
    map
}

/// Share of words whose label agrees with ground truth under the best
/// one-to-one mapping from labels to truth speakers (greedy by count).
fn agreement(
    words: &[TruthWord],
    labels: &HashMap<String, (String, &RenderedTranscriptSegment)>,
) -> f64 {
    let mut counts: HashMap<(String, String), usize> = HashMap::new();
    let mut total = 0usize;
    for word in words {
        let Some((label, _)) = labels.get(&word.id) else {
            continue;
        };
        total += 1;
        *counts
            .entry((label.clone(), word.speaker.clone()))
            .or_default() += 1;
    }
    let mut pairs: Vec<_> = counts.into_iter().collect();
    pairs.sort_by(|a, b| b.1.cmp(&a.1));
    let mut used_labels = Vec::new();
    let mut used_truth = Vec::new();
    let mut correct = 0usize;
    for ((label, truth), count) in pairs {
        if used_labels.contains(&label) || used_truth.contains(&truth) {
            continue;
        }
        used_labels.push(label);
        used_truth.push(truth);
        correct += count;
    }
    correct as f64 / total.max(1) as f64
}

fn enroll(voice: &str) -> SelfVoiceprint {
    let samples = tts(voice, ENROLL_TEXT);
    let print = voice_embedding(&samples).expect("enrollment embeds");
    println!("enrolled {voice}: {:.1}s speech", print.speech_seconds);
    assert!(print.speech_seconds >= 15.0, "enrollment fixture too short");
    SelfVoiceprint {
        human_id: SELF_ID.to_owned(),
        embedding: print.embedding,
    }
}

fn in_person_scene() -> (PathBuf, Vec<TruthWord>) {
    let mut scene = Scene::new(1);
    for turn in 0..5 {
        scene.say(0, 0, USER_VOICE, USER_LINES[turn]);
        scene.say(0, 0, OTHER_VOICE, OTHER_LINES[turn]);
    }
    let path = scene.write("in_person");
    (path, scene.words)
}

#[test]
#[ignore = "needs macOS `say`; run explicitly"]
fn online_one_to_one_labels_mic_as_self_and_system_as_them() {
    let mut scene = Scene::new(2);
    for turn in 0..5 {
        scene.say(0, 0, USER_VOICE, USER_LINES[turn]);
        scene.say(1, 1, OTHER_VOICE, OTHER_LINES[turn]);
    }
    let path = scene.write("online_1to1");
    let outcome = pass(&path, &scene.words, SpeakerPassMode::Auto, None);
    assert_eq!(outcome.mode, Some(SpeakerPassMode::Online));
    assert_eq!(outcome.speaker_count, 1, "one remote voice");
    let hints = apply_speaker_pass_hints(&[], &outcome.hints);

    for (self_name, expected) in [(Some("Alex"), "Alex"), (None, "Me")] {
        let segments = render(&scene.words, &hints, self_name, &[]);
        for segment in &segments {
            match segment.key.channel {
                anlg_transcript::ChannelProfile::DirectMic => {
                    assert_eq!(segment.speaker_label, expected)
                }
                anlg_transcript::ChannelProfile::RemoteParty => {
                    assert_eq!(segment.speaker_label, "Them")
                }
                other => panic!("unexpected channel {other:?}"),
            }
            assert_eq!(segment.attribution.source, AttributionSource::Channel);
            assert!(!segment.attribution.uncertain);
        }
    }
}

#[test]
#[ignore = "needs macOS `say`; run explicitly"]
fn online_multi_party_splits_system_audio_into_lettered_speakers() {
    let mut scene = Scene::new(2);
    for turn in 0..5 {
        scene.say(0, 0, USER_VOICE, USER_LINES[turn]);
        scene.say(1, 1, OTHER_VOICE, OTHER_LINES[turn]);
        scene.say(1, 1, THIRD_VOICE, THIRD_LINES[turn]);
    }
    let path = scene.write("online_multi");
    let outcome = pass(&path, &scene.words, SpeakerPassMode::Online, None);
    let truth: HashMap<&str, &str> = scene
        .words
        .iter()
        .map(|w| (w.id.as_str(), w.speaker.as_str()))
        .collect();
    let mut confusion: HashMap<(Option<i32>, &str), usize> = HashMap::new();
    for word in &outcome.hints.words {
        *confusion
            .entry((word.speaker_index, truth[word.word_id.as_str()]))
            .or_default() += 1;
    }
    println!("multi-party confusion: {confusion:?}");
    assert_eq!(outcome.speaker_count, 2, "two remote voices");
    let hints = apply_speaker_pass_hints(&[], &outcome.hints);
    let segments = render(&scene.words, &hints, Some("Alex"), &[]);
    let labels = labels_by_word(&segments);

    let remote: Vec<TruthWord> = scene
        .words
        .iter()
        .filter(|w| w.channel == 1)
        .cloned()
        .collect();
    let score = agreement(&remote, &labels);
    println!("multi-party remote agreement: {:.1}%", score * 100.0);
    assert!(score >= 0.85, "agreement {score}");
    let remote_labels: std::collections::BTreeSet<_> = remote
        .iter()
        .filter_map(|w| labels.get(&w.id).map(|(label, _)| label.clone()))
        .collect();
    assert_eq!(
        remote_labels.into_iter().collect::<Vec<_>>(),
        vec!["Speaker A".to_owned(), "Speaker B".to_owned()]
    );
    for segment in segments
        .iter()
        .filter(|s| s.key.channel == anlg_transcript::ChannelProfile::RemoteParty)
    {
        assert_eq!(segment.attribution.source, AttributionSource::Diarization);
        assert!(segment.speaker_id.starts_with("diar:1:"));
    }
    for segment in segments
        .iter()
        .filter(|s| s.key.channel == anlg_transcript::ChannelProfile::DirectMic)
    {
        assert_eq!(segment.speaker_label, "Alex");
    }
}

#[test]
#[ignore = "needs macOS `say`; run explicitly"]
fn in_person_names_only_the_enrolled_user() {
    let (path, words) = in_person_scene();
    let user_words: Vec<&TruthWord> = words.iter().filter(|w| w.speaker == USER_VOICE).collect();

    // Enrolled from a different utterance of the same voice.
    let print = enroll(USER_VOICE);
    let outcome = pass(&path, &words, SpeakerPassMode::InPerson, Some(&print));
    assert_eq!(outcome.speaker_count, 2);
    assert!(outcome.self_matched);
    assert!(outcome.self_match_score.unwrap() >= SELF_MATCH_MIN_SCORE);
    let hints = apply_speaker_pass_hints(&[], &outcome.hints);
    let segments = render(&words, &hints, Some("Alex"), &[]);
    let labels = labels_by_word(&segments);
    let score = agreement(&words, &labels);
    println!("in-person agreement: {:.1}%", score * 100.0);
    assert!(score >= 0.85);
    let user_as_self = user_words
        .iter()
        .filter(|w| labels.get(&w.id).is_some_and(|(label, _)| label == "Alex"))
        .count();
    assert!(user_as_self as f64 >= user_words.len() as f64 * 0.85);
    for segment in &segments {
        assert_eq!(
            segment.key.channel,
            anlg_transcript::ChannelProfile::MixedCapture
        );
        if segment.speaker_label == "Alex" {
            assert_eq!(segment.attribution.source, AttributionSource::Voiceprint);
            assert!(segment.attribution.confidence.unwrap() >= SELF_MATCH_MIN_SCORE);
            assert_eq!(segment.speaker_id, SELF_ID);
        } else {
            assert_eq!(segment.speaker_label, "Speaker 2");
            assert_eq!(segment.attribution.source, AttributionSource::Diarization);
        }
    }

    // No enrollment: anonymous clusters, nobody named.
    let outcome = pass(&path, &words, SpeakerPassMode::InPerson, None);
    assert!(!outcome.self_matched);
    let hints = apply_speaker_pass_hints(&[], &outcome.hints);
    let segments = render(&words, &hints, Some("Alex"), &[]);
    let names: std::collections::BTreeSet<_> =
        segments.iter().map(|s| s.speaker_label.clone()).collect();
    assert_eq!(
        names.into_iter().collect::<Vec<_>>(),
        vec!["Speaker 1".to_owned(), "Speaker 2".to_owned()]
    );

    // A different enrolled voice never names anyone.
    let impostor = enroll(IMPOSTOR_VOICE);
    let outcome = pass(&path, &words, SpeakerPassMode::InPerson, Some(&impostor));
    assert!(!outcome.self_matched, "no false positive");
    let hints = apply_speaker_pass_hints(&[], &outcome.hints);
    let segments = render(&words, &hints, Some("Alex"), &[]);
    assert!(segments.iter().all(|s| s.speaker_label != "Alex"));
}

#[test]
#[ignore = "needs macOS `say`; run explicitly"]
fn overlapped_speech_is_uncertain() {
    let mut scene = Scene::new(1);
    for turn in 0..4 {
        scene.say(0, 0, USER_VOICE, USER_LINES[turn]);
        scene.say(0, 0, OTHER_VOICE, OTHER_LINES[turn]);
    }
    scene.overlap(
        0,
        0,
        (USER_VOICE, USER_LINES[4]),
        (OTHER_VOICE, OTHER_LINES[4]),
    );
    scene.say(0, 0, USER_VOICE, USER_LINES[0]);
    scene.say(0, 0, OTHER_VOICE, OTHER_LINES[0]);
    let path = scene.write("overlap");
    let outcome = pass(&path, &scene.words, SpeakerPassMode::InPerson, None);
    let hints = apply_speaker_pass_hints(&[], &outcome.hints);
    let segments = render(&scene.words, &hints, Some("Alex"), &[]);
    let labels = labels_by_word(&segments);

    let overlapped: Vec<&TruthWord> = scene
        .words
        .iter()
        .filter(|w| w.speaker == "overlap")
        .collect();
    let flagged = overlapped
        .iter()
        .filter(|w| {
            labels
                .get(&w.id)
                .is_some_and(|(_, segment)| segment.attribution.uncertain_word_ids.contains(&w.id))
        })
        .count();
    let clean: Vec<&TruthWord> = scene
        .words
        .iter()
        .filter(|w| w.speaker != "overlap")
        .collect();
    let clean_flagged = clean
        .iter()
        .filter(|w| {
            labels
                .get(&w.id)
                .is_some_and(|(_, segment)| segment.attribution.uncertain_word_ids.contains(&w.id))
        })
        .count();
    println!(
        "overlap: {flagged}/{} overlapped words uncertain, {clean_flagged}/{} clean words uncertain",
        overlapped.len(),
        clean.len()
    );
    assert!(flagged as f64 >= overlapped.len() as f64 * 0.7);
    assert!((clean_flagged as f64) <= clean.len() as f64 * 0.15);
    // Uncertain words are never reported as confidently attributed.
    for hint in &hints {
        let value: serde_json::Value = serde_json::from_str(hint.value.as_str().unwrap()).unwrap();
        if value["uncertain"] == true {
            assert!(value["confidence"].as_f64().unwrap() < 0.5);
        }
    }
}

#[test]
#[ignore = "needs macOS `say`; run explicitly"]
fn renaming_a_cluster_propagates_to_every_segment_and_the_llm_render() {
    let (path, words) = in_person_scene();
    let outcome = pass(&path, &words, SpeakerPassMode::InPerson, None);
    let hints = apply_speaker_pass_hints(&[], &outcome.hints);
    let segments = render(&words, &hints, Some("Alex"), &[]);
    let speaker_two = segments
        .iter()
        .find(|s| s.speaker_label == "Speaker 2")
        .expect("a second speaker");
    let index = speaker_two.key.speaker_index.unwrap();
    let anchor = speaker_two.words[0].id.clone().unwrap();
    let cluster_segments = segments
        .iter()
        .filter(|s| s.key.speaker_index == Some(index))
        .count();

    let mut renamed = hints.clone();
    renamed.push(StoredSpeakerHint {
        id: format!("{anchor}:user_speaker_assignment"),
        word_id: Some(anchor),
        hint_type: "user_speaker_assignment".to_owned(),
        value: serde_json::Value::String(
            serde_json::json!({"human_id": "bob", "scope": "speaker", "channel": 2, "speaker_index": index})
                .to_string(),
        ),
    });
    // `render` is the same path the LLM transcript uses
    // (render_input_from_stored -> render_transcript_segments).
    let segments = render(&words, &renamed, Some("Alex"), &[("bob", "Bob")]);
    let bob: Vec<_> = segments
        .iter()
        .filter(|s| s.speaker_label == "Bob")
        .collect();
    assert_eq!(bob.len(), cluster_segments);
    assert!(
        bob.iter()
            .all(|s| s.attribution.source == AttributionSource::Manual)
    );
    assert!(bob.iter().all(|s| s.speaker_id == "bob"));
    assert!(segments.iter().all(|s| s.speaker_label != "Speaker 2"));
    let transcript_text = segments
        .iter()
        .map(|s| format!("{}: {}", s.speaker_label, s.text))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(transcript_text.contains("Bob: "));

    // A re-run never overrides the manual rename.
    let rerun = pass(&path, &words, SpeakerPassMode::InPerson, None);
    let locked = anlg_transcript::manually_assigned_word_ids(&stored_words(&words), &renamed);
    let mut rerun_hints = rerun.hints.clone();
    rerun_hints.words.retain(|w| !locked.contains(&w.word_id));
    let rehinted = apply_speaker_pass_hints(&renamed, &rerun_hints);
    let segments = render(&words, &rehinted, Some("Alex"), &[("bob", "Bob")]);
    let bob_words: usize = segments
        .iter()
        .filter(|s| s.speaker_label == "Bob")
        .map(|s| s.words.len())
        .sum();
    assert_eq!(bob_words, locked.len());
}

#[test]
#[ignore = "needs macOS `say`; run explicitly"]
fn deleted_enrollment_gives_no_self_label() {
    let (path, words) = in_person_scene();
    let mut stored: Option<SelfVoiceprint> = Some(enroll(USER_VOICE));
    assert!(stored.is_some());
    stored = None; // delete
    let outcome = pass(&path, &words, SpeakerPassMode::InPerson, stored.as_ref());
    assert!(!outcome.self_matched);
    assert!(outcome.hints.assignments.is_empty());
}

#[test]
#[ignore = "needs macOS `say`; run explicitly"]
fn calibration_scores() {
    // Same voice, different text vs. different voices, utterance level.
    let user = enroll(USER_VOICE);
    for (voice, lines) in [
        (USER_VOICE, USER_LINES),
        (OTHER_VOICE, OTHER_LINES),
        (THIRD_VOICE, THIRD_LINES),
        (IMPOSTOR_VOICE, USER_LINES),
    ] {
        let mut audio = Vec::new();
        for line in lines {
            audio.extend(tts(voice, line));
            audio.extend(std::iter::repeat_n(0.0, RATE / 2));
        }
        let print = voice_embedding(&audio).unwrap();
        let score = anlg_voiceprint::cosine_similarity(&user.embedding, &print.embedding).unwrap();
        println!("calibration: enrolled {USER_VOICE} vs {voice}: {score:.3}");
    }
    let _ = load_mono_16k;
}

/// Active-speaker spans a meeting platform would report for the remote
/// turns: each turn shows up ~0.8 s late, lingers ~1 s, and loses one
/// sample in the middle (a slow accessibility walk).
fn platform_spans_for(
    words: &[TruthWord],
    names: &HashMap<&str, &str>,
) -> Vec<PlatformSpeakerSpan> {
    let mut turns: Vec<(String, f64, f64)> = Vec::new();
    for word in words.iter().filter(|w| w.channel == 1) {
        match turns.last_mut() {
            Some((speaker, _, end)) if *speaker == word.speaker && word.start_ms - *end < 700.0 => {
                *end = word.end_ms;
            }
            _ => turns.push((word.speaker.clone(), word.start_ms, word.end_ms)),
        }
    }
    let mut spans = Vec::new();
    for (speaker, start, end) in turns {
        let name = names[speaker.as_str()].to_owned();
        let (from, to) = (start + 800.0, end + 1_000.0);
        let mid = (from + to) / 2.0;
        spans.push(PlatformSpeakerSpan {
            start_ms: from,
            end_ms: mid - 500.0,
            names: vec![name.clone()],
        });
        spans.push(PlatformSpeakerSpan {
            start_ms: mid + 500.0,
            end_ms: to,
            names: vec![name],
        });
    }
    spans
}

#[test]
#[ignore = "needs macOS `say`; run explicitly"]
fn online_multi_party_is_named_by_platform_tags() {
    let mut scene = Scene::new(2);
    for turn in 0..5 {
        scene.say(0, 0, USER_VOICE, USER_LINES[turn]);
        scene.say(1, 1, OTHER_VOICE, OTHER_LINES[turn]);
        scene.say(1, 1, THIRD_VOICE, THIRD_LINES[turn]);
    }
    let path = scene.write("online_multi");
    let names: HashMap<&str, &str> =
        HashMap::from([(OTHER_VOICE, "Priya Raman"), (THIRD_VOICE, "Sam Lee")]);
    let spans = platform_spans_for(&scene.words, &names);
    let mut outcome = pass_with_spans(&path, &scene.words, SpeakerPassMode::Online, None, &spans);
    println!(
        "platform names: {:?} tagged_words={}",
        outcome.platform_names, outcome.platform_tagged_words
    );
    assert_eq!(outcome.speaker_count, 2);
    assert_eq!(outcome.platform_names.len(), 2, "both voices named");
    // Priya is a calendar invitee; Sam is not, so Sam gets a platform id.
    outcome.hints.assignments = platform_assignments(&outcome.platform_names, |name| {
        if name == "Priya Raman" {
            "h-priya".to_owned()
        } else {
            anlg_transcript::platform_human_id(name)
        }
    });
    let hints = apply_speaker_pass_hints(&[], &outcome.hints);
    let segments = render(
        &scene.words,
        &hints,
        Some("Alex"),
        &[("h-priya", "Priya Raman")],
    );
    let labels = labels_by_word(&segments);
    let remote: Vec<&TruthWord> = scene.words.iter().filter(|w| w.channel == 1).collect();
    let correct = remote
        .iter()
        .filter(|w| {
            labels.get(&w.id).map(|(label, _)| label.as_str()) == Some(names[w.speaker.as_str()])
        })
        .count();
    let share = correct as f64 / remote.len() as f64;
    println!("platform-named remote words correct: {:.1}%", share * 100.0);
    assert!(share >= 0.9, "named correctly {share}");
    for segment in segments
        .iter()
        .filter(|s| s.key.channel == anlg_transcript::ChannelProfile::RemoteParty)
    {
        assert_eq!(segment.attribution.source, AttributionSource::PlatformTag);
        assert!(!segment.attribution.uncertain || segment.words.len() <= 3);
    }
    assert!(
        segments
            .iter()
            .filter(|s| s.key.channel == anlg_transcript::ChannelProfile::DirectMic)
            .all(|s| s.speaker_label == "Alex")
    );

    // Renaming the platform-named voice by hand wins and propagates.
    let sam_word = remote
        .iter()
        .find(|w| w.speaker == THIRD_VOICE)
        .expect("Sam spoke");
    let sam_index = outcome
        .hints
        .words
        .iter()
        .find(|w| w.word_id == sam_word.id)
        .and_then(|w| w.speaker_index)
        .expect("Sam's word is diarized");
    let mut renamed = hints.clone();
    renamed.push(StoredSpeakerHint {
        id: "manual".into(),
        word_id: Some(sam_word.id.clone()),
        hint_type: "user_speaker_assignment".into(),
        value: serde_json::Value::String(
            serde_json::json!({"human_id": "h-samuel", "scope": "speaker", "channel": 1, "speaker_index": sam_index})
                .to_string(),
        ),
    });
    let segments = render(
        &scene.words,
        &renamed,
        Some("Alex"),
        &[("h-priya", "Priya Raman"), ("h-samuel", "Samuel L.")],
    );
    let labels = labels_by_word(&segments);
    let sam_words: Vec<_> = remote.iter().filter(|w| w.speaker == THIRD_VOICE).collect();
    let renamed_share = sam_words
        .iter()
        .filter(|w| labels.get(&w.id).map(|(l, _)| l.as_str()) == Some("Samuel L."))
        .count() as f64
        / sam_words.len() as f64;
    assert!(renamed_share >= 0.9, "rename propagated {renamed_share}");
    assert!(!segments.iter().any(|s| s.speaker_label == "Sam Lee"));
}

#[test]
#[ignore = "needs macOS `say`; run explicitly"]
fn online_one_to_one_is_named_by_platform_tags() {
    let mut scene = Scene::new(2);
    for turn in 0..4 {
        scene.say(0, 0, USER_VOICE, USER_LINES[turn]);
        scene.say(1, 1, OTHER_VOICE, OTHER_LINES[turn]);
    }
    let path = scene.write("online_platform_1to1");
    let names: HashMap<&str, &str> = HashMap::from([(OTHER_VOICE, "Priya Raman")]);
    let spans = platform_spans_for(&scene.words, &names);
    let mut outcome = pass_with_spans(&path, &scene.words, SpeakerPassMode::Online, None, &spans);
    println!("1:1 platform names: {:?}", outcome.platform_names);
    assert_eq!(outcome.platform_names.len(), 1);
    assert_eq!(outcome.platform_names[0].speaker_index, None);
    outcome.hints.assignments =
        platform_assignments(&outcome.platform_names, anlg_transcript::platform_human_id);
    let hints = apply_speaker_pass_hints(&[], &outcome.hints);
    let segments = render(&scene.words, &hints, Some("Alex"), &[]);
    for segment in &segments {
        match segment.key.channel {
            anlg_transcript::ChannelProfile::RemoteParty => {
                assert_eq!(segment.speaker_label, "Priya Raman");
                assert_eq!(segment.attribution.source, AttributionSource::PlatformTag);
            }
            _ => assert_eq!(segment.speaker_label, "Alex"),
        }
    }
}
