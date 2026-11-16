//! Speaker attribution: where every rendered speaker label came from.
//!
//! Attribution is derived, never stored as its own column. The speaker hints
//! already saved with a transcript carry the evidence:
//!
//! * `user_speaker_assignment` -> [`AttributionSource::Manual`]
//! * `automatic_speaker_assignment` -> the hint's `source` (`voiceprint` and
//!   `elimination` -> [`AttributionSource::Voiceprint`], `platform_tag` ->
//!   [`AttributionSource::PlatformTag`]); hints written before sources existed
//!   default to voiceprint, the only automatic writer of that era
//! * `provider_speaker_index` -> [`AttributionSource::Diarization`]
//! * nothing -> [`AttributionSource::Channel`] (mic = the user, system audio =
//!   "Them")
//!
//! Hint values may also carry `confidence` (0..1) and `uncertain` (bool).
//! Old hints without those fields parse unchanged.
//!
//! A segment is uncertain when at least half of its words are uncertain
//! (`UNCERTAIN_SEGMENT_WORD_SHARE`); the individual uncertain words are listed
//! in [`SegmentAttribution::uncertain_word_ids`] so a UI can mark a short
//! overlapped stretch inside an otherwise confident turn.

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::{
    ChannelProfile, RenderTranscriptHuman, RenderTranscriptInput, RenderedTranscriptSegment,
    StoredSpeakerHint, StoredTranscriptWord,
};

/// Marks hints written by the post-stop speaker pass so a re-run can replace
/// exactly what it wrote before.
pub const SPEAKER_PASS_WRITER: &str = "speaker_pass";
/// Share of a segment's words that must be uncertain for the whole segment to
/// be flagged uncertain.
pub const UNCERTAIN_SEGMENT_WORD_SHARE: f32 = 0.5;

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Default,
    serde::Serialize,
    serde::Deserialize,
    specta::Type,
)]
#[serde(rename_all = "snake_case")]
pub enum AttributionSource {
    #[default]
    Channel,
    PlatformTag,
    Diarization,
    Voiceprint,
    Manual,
}

#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct SegmentAttribution {
    pub source: AttributionSource,
    #[serde(default)]
    pub confidence: Option<f32>,
    #[serde(default)]
    pub uncertain: bool,
    #[serde(default)]
    pub uncertain_word_ids: Vec<String>,
}

/// A raw speaker hint passed through to the renderer so attribution can be
/// derived in one place. Lenient on purpose: the frontend forwards whatever
/// it stored.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct RenderSpeakerHint {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub word_id: Option<String>,
    #[serde(rename = "type", default)]
    pub hint_type: String,
    #[serde(default)]
    pub value: Value,
}

impl From<&StoredSpeakerHint> for RenderSpeakerHint {
    fn from(hint: &StoredSpeakerHint) -> Self {
        Self {
            id: Some(hint.id.clone()),
            word_id: hint.word_id.clone(),
            hint_type: hint.hint_type.clone(),
            value: hint.value.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct WordAttribution {
    pub source: AttributionSource,
    pub confidence: Option<f32>,
    pub uncertain: bool,
}

pub(crate) fn parse_value(value: &Value) -> Option<Value> {
    let parsed = match value {
        Value::String(raw) => serde_json::from_str(raw).ok()?,
        Value::Object(_) => value.clone(),
        _ => return None,
    };
    parsed.is_object().then_some(parsed)
}

fn value_confidence(value: &Value) -> Option<f32> {
    value
        .get("confidence")
        .and_then(Value::as_f64)
        .filter(|c| c.is_finite())
        .map(|c| c.clamp(0.0, 1.0) as f32)
}

fn value_uncertain(value: &Value) -> bool {
    value.get("uncertain").and_then(Value::as_bool) == Some(true)
}

fn channel_number(value: &Value) -> Option<i32> {
    let channel = value.get("channel")?.as_f64()?;
    (channel == 0.0 || channel == 1.0 || channel == 2.0).then_some(channel as i32)
}

fn profile_number(channel: ChannelProfile) -> i32 {
    channel as i32
}

/// Maps an automatic assignment's `source` onto an attribution source and
/// whether it should be treated as uncertain (unknown writers are never
/// trusted silently).
fn automatic_source(value: &Value) -> (AttributionSource, bool) {
    match value.get("source").and_then(Value::as_str) {
        None | Some("voiceprint") | Some("elimination") => (AttributionSource::Voiceprint, false),
        Some("platform_tag") | Some("platform") => (AttributionSource::PlatformTag, false),
        Some("diarization") => (AttributionSource::Diarization, false),
        Some("channel") => (AttributionSource::Channel, false),
        Some("manual") => (AttributionSource::Manual, false),
        Some(_) => (AttributionSource::Voiceprint, true),
    }
}

#[derive(Debug, Clone)]
struct AssignmentEvidence {
    human_id: String,
    value: Value,
}

type SpeakerKey = (i32, Option<i32>);

#[derive(Debug, Default)]
struct TranscriptEvidence {
    provider_by_word: HashMap<String, Value>,
    user_by_word: HashMap<String, AssignmentEvidence>,
    user_by_speaker: HashMap<SpeakerKey, AssignmentEvidence>,
    auto_by_speaker: HashMap<SpeakerKey, AssignmentEvidence>,
    auto_by_word: HashMap<String, AssignmentEvidence>,
}

/// Evidence index over every transcript's raw hints, keyed by word id.
#[derive(Debug, Default)]
pub struct AttributionIndex {
    has_hints: bool,
    by_word: HashMap<String, usize>,
    transcripts: Vec<TranscriptEvidence>,
}

impl AttributionIndex {
    pub fn from_inputs(inputs: &[RenderTranscriptInput]) -> Self {
        let mut index = Self::default();
        for input in inputs {
            let Some(hints) = input.speaker_hints.as_ref() else {
                continue;
            };
            index.has_hints = true;
            let position = index.transcripts.len();
            let mut evidence = TranscriptEvidence::default();
            let word_key: HashMap<&str, SpeakerKey> = input
                .words
                .iter()
                .map(|word| (word.id.as_str(), (word.channel, word.speaker_index)))
                .collect();
            for word in &input.words {
                index.by_word.insert(word.id.clone(), position);
            }
            for hint in hints {
                let Some(value) = parse_value(&hint.value) else {
                    continue;
                };
                let word_id = hint.word_id.clone().unwrap_or_default();
                match hint.hint_type.as_str() {
                    "provider_speaker_index" => {
                        if !word_id.is_empty() {
                            evidence.provider_by_word.insert(word_id, value);
                        }
                    }
                    kind @ ("user_speaker_assignment" | "automatic_speaker_assignment") => {
                        let Some(human_id) = value
                            .get("human_id")
                            .and_then(Value::as_str)
                            .filter(|id| !id.is_empty())
                            .map(str::to_owned)
                        else {
                            continue;
                        };
                        let user = kind == "user_speaker_assignment";
                        let item = AssignmentEvidence {
                            human_id,
                            value: value.clone(),
                        };
                        let scope = value.get("scope").and_then(Value::as_str);
                        if scope == Some("segment") {
                            let ids = value
                                .get("word_ids")
                                .and_then(Value::as_array)
                                .map(|ids| {
                                    ids.iter()
                                        .filter_map(Value::as_str)
                                        .map(str::to_owned)
                                        .collect::<Vec<_>>()
                                })
                                .unwrap_or_default();
                            for id in ids {
                                if user {
                                    evidence.user_by_word.insert(id, item.clone());
                                } else {
                                    evidence.auto_by_word.insert(id, item.clone());
                                }
                            }
                            continue;
                        }
                        let key = if scope == Some("speaker") {
                            let Some(channel) = channel_number(&value) else {
                                continue;
                            };
                            let speaker_index = value
                                .get("speaker_index")
                                .and_then(Value::as_f64)
                                .map(|index| index as i32);
                            (channel, speaker_index)
                        } else {
                            // Legacy: the assignment names whoever speaks at
                            // its anchor word.
                            match word_key.get(word_id.as_str()) {
                                Some(key) => *key,
                                None => continue,
                            }
                        };
                        if user {
                            evidence.user_by_speaker.insert(key, item);
                        } else {
                            evidence.auto_by_speaker.insert(key, item);
                        }
                    }
                    _ => {}
                }
            }
            index.transcripts.push(evidence);
        }
        index
    }

    fn evidence(&self, word_id: Option<&str>) -> Option<&TranscriptEvidence> {
        let position = *self.by_word.get(word_id?)?;
        self.transcripts.get(position)
    }

    /// Attribution of one rendered word inside `segment`.
    pub fn word_attribution(
        &self,
        segment: &RenderedTranscriptSegment,
        word_id: Option<&str>,
        channel: ChannelProfile,
    ) -> WordAttribution {
        let key = &segment.key;
        let speaker_key = (profile_number(channel), key.speaker_index);
        let evidence = self.evidence(word_id);
        let provider = evidence
            .zip(word_id)
            .and_then(|(evidence, id)| evidence.provider_by_word.get(id));
        let provider_uncertain = provider.is_some_and(value_uncertain);
        let provider_confidence = provider.and_then(value_confidence);

        if let Some(human_id) = key.speaker_human_id.as_deref() {
            if let Some(evidence) = evidence {
                let channel_key = (speaker_key.0, None);
                let user = word_id
                    .and_then(|id| evidence.user_by_word.get(id))
                    .or_else(|| evidence.user_by_speaker.get(&speaker_key))
                    .or_else(|| evidence.user_by_speaker.get(&channel_key))
                    .filter(|item| item.human_id == human_id);
                if user.is_some() {
                    return WordAttribution {
                        source: AttributionSource::Manual,
                        confidence: None,
                        uncertain: false,
                    };
                }
                let automatic = word_id
                    .and_then(|id| evidence.auto_by_word.get(id))
                    .or_else(|| evidence.auto_by_speaker.get(&speaker_key))
                    .or_else(|| evidence.auto_by_speaker.get(&channel_key))
                    .filter(|item| item.human_id == human_id);
                if let Some(item) = automatic {
                    let (source, unknown) = automatic_source(&item.value);
                    return WordAttribution {
                        source,
                        confidence: value_confidence(&item.value),
                        uncertain: unknown || value_uncertain(&item.value) || provider_uncertain,
                    };
                }
                return WordAttribution {
                    source: AttributionSource::Channel,
                    confidence: None,
                    uncertain: provider_uncertain,
                };
            }
            // No raw hints were passed: an explicit human on a segment can
            // only come from an assignment; without evidence call it manual.
            return WordAttribution {
                source: if self.has_hints {
                    AttributionSource::Channel
                } else {
                    AttributionSource::Manual
                },
                confidence: None,
                uncertain: false,
            };
        }

        if segment.provisional_speaker.is_some() {
            return WordAttribution {
                source: AttributionSource::Channel,
                confidence: None,
                uncertain: false,
            };
        }

        // The mic channel is the user regardless of any provider speaker
        // index; the label comes from the channel.
        if key.speaker_index.is_some() && key.channel != ChannelProfile::DirectMic {
            let source = provider
                .and_then(|value| value.get("source").and_then(Value::as_str))
                .map(|source| match source {
                    "channel" => AttributionSource::Channel,
                    _ => AttributionSource::Diarization,
                })
                .unwrap_or(AttributionSource::Diarization);
            return WordAttribution {
                source,
                confidence: provider_confidence,
                uncertain: provider_uncertain,
            };
        }

        WordAttribution {
            source: AttributionSource::Channel,
            confidence: None,
            uncertain: provider_uncertain,
        }
    }

    pub fn segment_attribution(&self, segment: &RenderedTranscriptSegment) -> SegmentAttribution {
        let mut counts: Vec<(AttributionSource, usize)> = Vec::new();
        let mut confidences = Vec::new();
        let mut uncertain_word_ids = Vec::new();
        let mut uncertain_count = 0usize;
        for word in &segment.words {
            let attribution = self.word_attribution(segment, word.id.as_deref(), word.channel);
            match counts
                .iter_mut()
                .find(|(source, _)| *source == attribution.source)
            {
                Some((_, count)) => *count += 1,
                None => counts.push((attribution.source, 1)),
            }
            if let Some(confidence) = attribution.confidence {
                confidences.push(confidence);
            }
            if attribution.uncertain {
                uncertain_count += 1;
                if let Some(id) = &word.id {
                    uncertain_word_ids.push(id.clone());
                }
            }
        }
        let source = counts
            .iter()
            .fold(
                None::<(AttributionSource, usize)>,
                |best, (source, count)| match best {
                    Some((_, best_count)) if best_count >= *count => best,
                    _ => Some((*source, *count)),
                },
            )
            .map(|(source, _)| source)
            .unwrap_or_default();
        let total = segment.words.len().max(1);
        SegmentAttribution {
            source,
            confidence: (!confidences.is_empty())
                .then(|| confidences.iter().sum::<f32>() / confidences.len() as f32),
            uncertain: source != AttributionSource::Manual
                && uncertain_count as f32 >= total as f32 * UNCERTAIN_SEGMENT_WORD_SHARE
                && uncertain_count > 0,
            uncertain_word_ids,
        }
    }
}

/// Stable identifier for who a segment belongs to: the human id when one is
/// assigned, otherwise `diar:<channel>:<speaker_index>` or `channel:<channel>`.
pub fn segment_speaker_id(segment: &RenderedTranscriptSegment) -> String {
    if let Some(human_id) = &segment.key.speaker_human_id {
        return human_id.clone();
    }
    if let Some(human_id) = segment
        .provisional_speaker
        .as_ref()
        .and_then(|speaker| speaker.human_id.as_ref())
    {
        return human_id.clone();
    }
    let channel = segment.key.channel as i32;
    match segment.key.speaker_index {
        Some(index) if segment.key.channel != ChannelProfile::DirectMic => {
            format!("diar:{channel}:{index}")
        }
        _ => format!("channel:{channel}"),
    }
}

fn letters(mut index: usize) -> String {
    let mut out = Vec::new();
    loop {
        out.push((b'A' + (index % 26) as u8) as char);
        if index < 26 {
            break;
        }
        index = index / 26 - 1;
    }
    out.iter().rev().collect()
}

fn human_name<'a>(humans: &'a [RenderTranscriptHuman], id: &str) -> Option<&'a str> {
    humans
        .iter()
        .find(|human| human.human_id == id)
        .map(|human| human.name.trim())
        .filter(|name| !name.is_empty())
}

/// Final speaker labels, from real signals only:
///
/// * an assigned human -> their name (the user with no name -> "Me")
/// * mic channel -> the user's name, else "Me"; but a mic that diarized into
///   several voices (a shared room mic), or mic speech the capture context
///   declined to attribute to the user, is numbered like a mixed channel
/// * system channel with fewer than two diarized voices -> "Them"
/// * system channel with several diarized voices -> "Speaker A", "Speaker B"…
/// * mixed / shared-mic channel -> "Speaker 1", "Speaker 2"…
///
/// Letters and numbers follow order of first appearance across all clusters
/// of that channel, named ones included, so renaming one speaker never
/// renumbers the others.
pub fn finalize_speaker_labels(
    segments: &mut [RenderedTranscriptSegment],
    self_human_id: Option<&str>,
    humans: &[RenderTranscriptHuman],
    index: &AttributionIndex,
    capture_context_present: bool,
) {
    let self_human_id = self_human_id.filter(|id| !id.trim().is_empty());
    let self_label = self_human_id
        .and_then(|id| human_name(humans, id))
        .unwrap_or("Me")
        .to_owned();

    let mut order: Vec<usize> = (0..segments.len()).collect();
    order.sort_by_key(|i| segments[*i].start_ms);
    let mut mic_voices: Vec<i32> = Vec::new();
    for segment in segments.iter() {
        if segment.key.channel == ChannelProfile::DirectMic
            && let Some(speaker) = segment.key.speaker_index
            && !mic_voices.contains(&speaker)
        {
            mic_voices.push(speaker);
        }
    }
    let shared_mic = mic_voices.len() >= 2;
    let mic_is_anonymous = |segment: &RenderedTranscriptSegment| {
        segment.key.channel == ChannelProfile::DirectMic
            && segment.provisional_speaker.is_none()
            && (capture_context_present || shared_mic)
    };

    let mut remote: Vec<i32> = Vec::new();
    let mut numbered: Vec<(ChannelProfile, Option<i32>)> = Vec::new();
    for &i in &order {
        let segment = &segments[i];
        let key = &segment.key;
        match key.channel {
            ChannelProfile::RemoteParty => {
                if let Some(speaker) = key.speaker_index
                    && !remote.contains(&speaker)
                {
                    remote.push(speaker);
                }
            }
            ChannelProfile::MixedCapture => {
                if !numbered.contains(&(key.channel, key.speaker_index)) {
                    numbered.push((key.channel, key.speaker_index));
                }
            }
            ChannelProfile::DirectMic => {
                if mic_is_anonymous(segment)
                    && !numbered.contains(&(key.channel, key.speaker_index))
                {
                    numbered.push((key.channel, key.speaker_index));
                }
            }
        }
    }
    let several_remote = remote.len() >= 2;
    let number = |channel: ChannelProfile, speaker_index: Option<i32>| -> String {
        let position = numbered
            .iter()
            .position(|item| *item == (channel, speaker_index))
            .unwrap_or(numbered.len());
        format!("Speaker {}", position + 1)
    };
    let anonymous = |segment: &RenderedTranscriptSegment| -> String {
        let (channel, speaker_index) = (segment.key.channel, segment.key.speaker_index);
        match channel {
            ChannelProfile::DirectMic if mic_is_anonymous(segment) => {
                number(channel, speaker_index)
            }
            ChannelProfile::DirectMic => self_label.clone(),
            ChannelProfile::RemoteParty => match speaker_index {
                Some(speaker) if several_remote => {
                    let position = remote.iter().position(|s| *s == speaker).unwrap_or(0);
                    format!("Speaker {}", letters(position))
                }
                _ => "Them".to_owned(),
            },
            ChannelProfile::MixedCapture => number(channel, speaker_index),
        }
    };

    let mut labels = Vec::with_capacity(segments.len());
    for segment in segments.iter() {
        let label = if let Some(human_id) = segment.key.speaker_human_id.as_deref() {
            match human_name(humans, human_id) {
                Some(name) => name.to_owned(),
                None if Some(human_id) == self_human_id => self_label.clone(),
                None => anonymous(segment),
            }
        } else if let Some(provisional) = &segment.provisional_speaker {
            provisional.name.clone()
        } else {
            anonymous(segment)
        };
        labels.push(label);
    }
    for (segment, label) in segments.iter_mut().zip(labels) {
        segment.speaker_label = label;
        segment.speaker_id = segment_speaker_id(segment);
        segment.attribution = index.segment_attribution(segment);
    }
}

/// Words whose speaker the user already set by hand. The speaker pass never
/// relabels them.
pub fn manually_assigned_word_ids(
    words: &[StoredTranscriptWord],
    hints: &[StoredSpeakerHint],
) -> HashSet<String> {
    let mut key_by_word: HashMap<&str, SpeakerKey> = HashMap::new();
    for word in words {
        key_by_word.insert(
            word.id.as_str(),
            (word.channel.unwrap_or(0.0).round() as i32, None),
        );
    }
    for hint in hints
        .iter()
        .filter(|hint| hint.hint_type == "provider_speaker_index")
    {
        let (Some(word_id), Some(value)) = (hint.word_id.as_deref(), parse_value(&hint.value))
        else {
            continue;
        };
        if let Some(entry) = key_by_word.get_mut(word_id)
            && let Some(index) = value.get("speaker_index").and_then(Value::as_f64)
        {
            entry.1 = Some(index as i32);
            if let Some(channel) = channel_number(&value) {
                entry.0 = channel;
            }
        }
    }

    let mut locked_words = HashSet::new();
    let mut locked_speakers: HashSet<SpeakerKey> = HashSet::new();
    let mut locked_channels: HashSet<i32> = HashSet::new();
    for hint in hints
        .iter()
        .filter(|hint| hint.hint_type == "user_speaker_assignment")
    {
        let Some(value) = parse_value(&hint.value) else {
            continue;
        };
        match value.get("scope").and_then(Value::as_str) {
            Some("segment") => {
                for id in value
                    .get("word_ids")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                {
                    locked_words.insert(id.to_owned());
                }
            }
            Some("speaker") => {
                let Some(channel) = channel_number(&value) else {
                    continue;
                };
                match value.get("speaker_index").and_then(Value::as_f64) {
                    Some(index) => {
                        locked_speakers.insert((channel, Some(index as i32)));
                    }
                    None => {
                        locked_channels.insert(channel);
                    }
                }
            }
            _ => {
                if let Some(key) = hint.word_id.as_deref().and_then(|id| key_by_word.get(id)) {
                    match key.1 {
                        Some(_) => {
                            locked_speakers.insert(*key);
                        }
                        None => {
                            locked_channels.insert(key.0);
                        }
                    }
                }
            }
        }
    }
    for (word_id, key) in &key_by_word {
        if locked_speakers.contains(key) || locked_channels.contains(&key.0) {
            locked_words.insert((*word_id).to_owned());
        }
    }
    locked_words
}

/// One word's outcome from the post-stop speaker pass.
#[derive(Debug, Clone, PartialEq)]
pub struct SpeakerPassWord {
    pub word_id: String,
    /// Channel the word renders on (2 when an in-person mic was split).
    pub channel: i32,
    /// Diarized cluster, `None` when the word keeps channel attribution.
    pub speaker_index: Option<i32>,
    pub confidence: f32,
    pub uncertain: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpeakerPassAssignment {
    pub channel: i32,
    pub speaker_index: i32,
    pub human_id: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SpeakerPassHints {
    /// Every word the pass examined. Their previous non-manual speaker hints
    /// are replaced.
    pub words: Vec<SpeakerPassWord>,
    pub assignments: Vec<SpeakerPassAssignment>,
}

fn is_pass_hint(hint: &StoredSpeakerHint) -> bool {
    parse_value(&hint.value).is_some_and(|value| {
        value.get("writer").and_then(Value::as_str) == Some(SPEAKER_PASS_WRITER)
    })
}

/// Rewrites a transcript's hints with a speaker-pass result: previous
/// pass-written hints are dropped, provider indices and automatic
/// assignments anchored on re-examined words are replaced, manual
/// assignments are kept untouched. New cluster indices are shifted past any
/// index a manual speaker-scoped assignment already names on that channel,
/// so a rename never jumps onto a different voice.
pub fn apply_speaker_pass_hints(
    hints: &[StoredSpeakerHint],
    pass: &SpeakerPassHints,
) -> Vec<StoredSpeakerHint> {
    let examined: HashSet<&str> = pass.words.iter().map(|w| w.word_id.as_str()).collect();
    let mut reserved: HashMap<i32, i32> = HashMap::new();
    for hint in hints
        .iter()
        .filter(|hint| hint.hint_type == "user_speaker_assignment")
    {
        let Some(value) = parse_value(&hint.value) else {
            continue;
        };
        if value.get("scope").and_then(Value::as_str) != Some("speaker") {
            continue;
        }
        if let (Some(channel), Some(index)) = (
            channel_number(&value),
            value.get("speaker_index").and_then(Value::as_f64),
        ) {
            let entry = reserved.entry(channel).or_insert(0);
            *entry = (*entry).max(index as i32 + 1);
        }
    }
    // A manual assignment written on a previous pass's cluster index must
    // keep pointing at that voice: if the rerun clusters the same way the
    // locked words keep their old hints and the new indices start after it.
    let offset = |channel: i32| reserved.get(&channel).copied().unwrap_or(0);

    let mut next: Vec<StoredSpeakerHint> = hints
        .iter()
        .filter(|hint| {
            if hint.hint_type == "user_speaker_assignment" {
                return true;
            }
            // Cluster names the pass gave are always recomputed; its word
            // indices on words it did not re-examine (manually locked) stay.
            if hint.hint_type == "automatic_speaker_assignment" && is_pass_hint(hint) {
                return false;
            }
            let anchored = hint
                .word_id
                .as_deref()
                .is_some_and(|id| examined.contains(id));
            !(anchored
                && matches!(
                    hint.hint_type.as_str(),
                    "provider_speaker_index" | "automatic_speaker_assignment"
                ))
        })
        .cloned()
        .collect();

    for word in &pass.words {
        let Some(speaker_index) = word.speaker_index else {
            continue;
        };
        let speaker_index = speaker_index + offset(word.channel);
        let value = serde_json::json!({
            "speaker_index": speaker_index,
            "channel": word.channel,
            "source": "diarization",
            "confidence": round3(word.confidence),
            "uncertain": word.uncertain,
            "writer": SPEAKER_PASS_WRITER,
        });
        next.push(StoredSpeakerHint {
            id: format!("{}:provider_speaker_index", word.word_id),
            word_id: Some(word.word_id.clone()),
            hint_type: "provider_speaker_index".to_owned(),
            value: Value::String(value.to_string()),
        });
    }

    for assignment in &pass.assignments {
        let speaker_index = assignment.speaker_index + offset(assignment.channel);
        let Some(anchor) = pass.words.iter().find(|word| {
            word.channel == assignment.channel
                && word.speaker_index.map(|index| index + offset(word.channel))
                    == Some(speaker_index)
        }) else {
            continue;
        };
        let value = serde_json::json!({
            "human_id": assignment.human_id,
            "scope": "speaker",
            "channel": assignment.channel,
            "speaker_index": speaker_index,
            "source": "voiceprint",
            "confidence": round3(assignment.confidence),
            "writer": SPEAKER_PASS_WRITER,
        });
        next.push(StoredSpeakerHint {
            id: format!("{}:automatic_speaker_assignment", anchor.word_id),
            word_id: Some(anchor.word_id.clone()),
            hint_type: "automatic_speaker_assignment".to_owned(),
            value: Value::String(value.to_string()),
        });
    }
    next
}

fn round3(value: f32) -> f64 {
    (f64::from(value) * 1000.0).round() / 1000.0
}

#[cfg(test)]
mod tests;
