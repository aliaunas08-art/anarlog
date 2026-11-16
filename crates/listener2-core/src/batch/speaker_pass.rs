//! Post-stop speaker pass: re-attributes a finished transcript's words from
//! the retained session audio, using only real signals.
//!
//! * Online meetings (stereo: L = mic, R = system audio): mic words stay the
//!   user by channel. System-audio words are diarized on the system channel
//!   alone with open speaker bounds; one voice keeps the channel label
//!   ("Them"), several become clusters ("Speaker A", "Speaker B"…).
//! * In-person meetings (one shared mic, or both channels identical): the mic
//!   is diarized into clusters on the mixed channel (2). Each cluster centroid
//!   is compared with the user's enrolled voiceprint only; a cluster is named
//!   the user when its score clears [`SELF_MATCH_MIN_SCORE`] and beats every
//!   other cluster by [`SELF_MATCH_MIN_MARGIN`]. Otherwise nothing is named.
//!
//! Words inside overlapped speech, or far from any speaker turn, are marked
//! uncertain. Embeddings and audio never leave this module except as the
//! returned enrollment vector, and are never logged.

use std::path::Path;

use anlg_pyannote_local::{
    AudioSource, Diarization, DiarizationConfig, DiarizeRequest, Diarizer, SpeakerBounds,
    segmentation::{Segmenter, WINDOW_SAMPLES, frame_start_sample, frames_for_samples},
};
use anlg_transcript::{
    AttributionSource, SpeakerPassAssignment, SpeakerPassHints, SpeakerPassWord,
};
use anlg_voiceprint::cosine_similarity;

use super::diarize::{ChannelAudio, MixedAudioSource, SAMPLE_RATE};
use super::simple::{ResampledChannelFile, resample_audio_to_channel_files_until};

/// Cosine similarity a cluster centroid needs against the enrolled user
/// voiceprint before it is named the user. Calibrated on the synthetic
/// harness (`tests/speaker_attribution.rs`): the user's cluster scored 0.94
/// against an enrollment from a different utterance (utterance level 0.98),
/// other voices' clusters 0.04-0.32 (utterance level 0.09-0.18). Real
/// microphones and rooms pull same-voice scores down well below TTS, so the
/// threshold sits low in the gap rather than near the synthetic same-voice
/// scores.
pub const SELF_MATCH_MIN_SCORE: f32 = 0.55;
/// The user's best cluster must beat the runner-up by this much; two
/// similar-sounding clusters are never guessed between.
pub const SELF_MATCH_MIN_MARGIN: f32 = 0.12;
/// Clusters whose centroids are at least this similar are one voice. In the
/// harness one remote voice split into two clusters scoring 0.68 against each
/// other, while different voices' clusters scored 0.14-0.39.
pub const CLUSTER_MERGE_MIN_SIMILARITY: f32 = 0.6;
/// Minimum detected speech for a voice enrollment.
pub const MIN_ENROLLMENT_SPEECH_SECONDS: f32 = 15.0;
/// Minimum detected speech to blend a transcript span into the enrollment.
pub const MIN_SPAN_SPEECH_SECONDS: f32 = 2.0;
/// Speech chunk length for the enrollment's per-chunk embeddings.
const ENROLLMENT_CHUNK_SECONDS: f32 = 3.0;
/// A system channel whose mean absolute level stays below this carried no
/// call: an in-person recording that merely wrote a silent second channel.
const SILENT_CHANNEL_MEAN_ABS: f64 = 0.0005;
/// Share of a word inside overlapped speech that makes it uncertain.
const OVERLAP_UNCERTAIN_SHARE: f64 = 0.3;
/// A second speaker covering this share of a word makes it uncertain.
const SECOND_SPEAKER_UNCERTAIN_SHARE: f64 = 0.3;
/// Words further than this from every turn are uncertain.
const GAP_UNCERTAIN_SECONDS: f64 = 0.15;
const LOW_CONFIDENCE: f32 = 0.35;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum SpeakerPassMode {
    Online,
    InPerson,
    Auto,
}

/// One transcript word the pass may relabel. Callers leave out words the
/// user already attributed by hand.
#[derive(Debug, Clone, PartialEq)]
pub struct SpeakerPassInputWord {
    pub id: String,
    pub start_ms: f64,
    pub end_ms: f64,
    pub channel: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SelfVoiceprint {
    pub human_id: String,
    pub embedding: Vec<f32>,
}

pub struct SpeakerPassRequest<'a> {
    pub audio_path: &'a Path,
    pub words: &'a [SpeakerPassInputWord],
    pub mode: SpeakerPassMode,
    pub self_voiceprint: Option<&'a SelfVoiceprint>,
    /// Who the meeting platform marked as speaking, on the transcript's
    /// clock. Empty when nothing was recorded.
    pub platform_spans: &'a [PlatformSpeakerSpan],
    pub is_cancelled: &'a dyn Fn() -> bool,
}

/// A stretch during which the meeting platform marked remote participants
/// as speaking. Times are milliseconds on the transcript's clock; `names`
/// never includes the user.
#[derive(Debug, Clone, PartialEq)]
pub struct PlatformSpeakerSpan {
    pub start_ms: f64,
    pub end_ms: f64,
    pub names: Vec<String>,
}

/// A remote voice named from platform tags.
#[derive(Debug, Clone, PartialEq)]
pub struct PlatformNaming {
    /// Diarized cluster on the system channel; `None` is the whole channel.
    pub speaker_index: Option<i32>,
    pub name: String,
    /// Share of the voice's tagged words that carried this name.
    pub confidence: f32,
    /// The same name won more than one voice: a shared device or room.
    pub uncertain: bool,
    pub tagged_words: u32,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SpeakerPassOutcome {
    /// `Online` or `InPerson`; never `Auto`.
    pub mode: Option<SpeakerPassMode>,
    pub hints: SpeakerPassHints,
    pub diarized_channels: Vec<i32>,
    pub speaker_count: u32,
    /// Best cluster score against the enrolled user voiceprint.
    pub self_match_score: Option<f32>,
    pub self_matched: bool,
    /// Every cluster's score against the enrolled voiceprint, in cluster
    /// order (for calibration and diagnostics; scores only, no vectors).
    pub cluster_self_scores: Vec<f32>,
    pub uncertain_words: u32,
    pub skipped_reason: Option<String>,
    /// Remote voices the platform's active-speaker tags named.
    pub platform_names: Vec<PlatformNaming>,
    /// System-channel words with exactly one platform-tagged speaker.
    pub platform_tagged_words: u32,
}

pub fn run_speaker_pass(request: &SpeakerPassRequest<'_>) -> Result<SpeakerPassOutcome, String> {
    let path_str = request
        .audio_path
        .to_str()
        .ok_or_else(|| "audio path is not valid UTF-8".to_string())?;
    let source = anlg_audio_utils::source_from_path(request.audio_path)
        .map_err(|error| error.to_string())?;
    let files =
        resample_audio_to_channel_files_until(path_str, source, || (request.is_cancelled)())?;
    if files.is_empty() {
        return Ok(skipped("recording has no audio channels"));
    }

    let mode = match request.mode {
        SpeakerPassMode::Online if files.len() >= 2 => SpeakerPassMode::Online,
        SpeakerPassMode::Online | SpeakerPassMode::InPerson => SpeakerPassMode::InPerson,
        SpeakerPassMode::Auto => {
            if files.len() < 2 || channel_mean_abs(&files[1])? < SILENT_CHANNEL_MEAN_ABS {
                SpeakerPassMode::InPerson
            } else {
                SpeakerPassMode::Online
            }
        }
    };

    let mut diarizer =
        Diarizer::new(DiarizationConfig::default()).map_err(|error| error.to_string())?;
    let outcome = match mode {
        SpeakerPassMode::Online => online_pass(&mut diarizer, &files, request)?,
        _ => in_person_pass(&mut diarizer, &files, request)?,
    };
    Ok(SpeakerPassOutcome {
        mode: Some(mode),
        ..outcome
    })
}

fn skipped(reason: &str) -> SpeakerPassOutcome {
    SpeakerPassOutcome {
        skipped_reason: Some(reason.to_owned()),
        ..Default::default()
    }
}

fn diarize(
    diarizer: &mut Diarizer,
    files: &[ResampledChannelFile],
    audio: ChannelAudio,
    request: &SpeakerPassRequest<'_>,
) -> Result<Diarization, String> {
    let mut source = MixedAudioSource::for_audio(files, audio)?;
    let is_cancelled = || (request.is_cancelled)();
    let diarization = diarizer
        .diarize(
            &mut source,
            &DiarizeRequest {
                // No participant count: open bounds, the audio decides.
                bounds: SpeakerBounds::default(),
                known_speakers: &[],
                is_cancelled: Some(&is_cancelled),
                on_progress: None,
            },
        )
        .map_err(|error| error.to_string())?;
    Ok(merge_similar_clusters(diarization))
}

/// Merges clusters whose centroids are so alike they are one voice. Local
/// clustering tends to split one speaker whose turns are separated by long
/// silences (a remote voice heard only between the user's turns).
pub fn merge_similar_clusters(mut diarization: Diarization) -> Diarization {
    loop {
        let speakers = &diarization.speakers;
        let mut best: Option<(usize, usize, f32)> = None;
        for a in 0..speakers.len() {
            for b in a + 1..speakers.len() {
                let Some(score) = cosine_similarity(&speakers[a].centroid, &speakers[b].centroid)
                else {
                    continue;
                };
                if best.is_none_or(|(_, _, best)| score > best) {
                    best = Some((a, b, score));
                }
            }
        }
        let Some((a, b, score)) = best else {
            break;
        };
        if score < CLUSTER_MERGE_MIN_SIMILARITY {
            break;
        }
        let keep = diarization.speakers[a].index;
        let drop = diarization.speakers[b].index;
        let duration = |speaker: usize| -> f32 {
            diarization
                .segments
                .iter()
                .filter(|segment| segment.speaker == speaker)
                .map(|segment| (segment.end - segment.start) as f32)
                .sum()
        };
        let (weight_keep, weight_drop) = (duration(keep).max(0.01), duration(drop).max(0.01));
        let merged: Vec<f32> = diarization.speakers[a]
            .centroid
            .iter()
            .zip(&diarization.speakers[b].centroid)
            .map(|(x, y)| (x * weight_keep + y * weight_drop) / (weight_keep + weight_drop))
            .collect();
        diarization.speakers[a].centroid = merged;
        diarization.speakers.remove(b);
        for segment in &mut diarization.segments {
            if segment.speaker == drop {
                segment.speaker = keep;
            }
        }
        // Keep indices dense and in order of first appearance.
        let mut order: Vec<usize> = Vec::new();
        let mut segments = diarization.segments.clone();
        segments.sort_by(|x, y| x.start.total_cmp(&y.start));
        for segment in &segments {
            if !order.contains(&segment.speaker) {
                order.push(segment.speaker);
            }
        }
        let position = |speaker: usize| order.iter().position(|s| *s == speaker).unwrap_or(0);
        for segment in &mut diarization.segments {
            segment.speaker = position(segment.speaker);
        }
        for speaker in &mut diarization.speakers {
            speaker.index = position(speaker.index);
        }
        diarization.speakers.sort_by_key(|speaker| speaker.index);
        diarization.overlaps = overlap_regions(&diarization.segments);
    }
    diarization
}

/// Stretches where two different speakers' turns are active at once.
fn overlap_regions(segments: &[anlg_pyannote_local::SpeakerSegment]) -> Vec<(f64, f64)> {
    let mut events: Vec<(f64, i32, usize)> = Vec::new();
    for segment in segments {
        events.push((segment.start, 1, segment.speaker));
        events.push((segment.end, -1, segment.speaker));
    }
    events.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let mut active: std::collections::HashMap<usize, i32> = std::collections::HashMap::new();
    let mut regions: Vec<(f64, f64)> = Vec::new();
    let mut open: Option<f64> = None;
    for (time, delta, speaker) in events {
        *active.entry(speaker).or_default() += delta;
        let distinct = active.values().filter(|count| **count > 0).count();
        match (open, distinct >= 2) {
            (None, true) => open = Some(time),
            (Some(start), false) => {
                if time > start {
                    regions.push((start, time));
                }
                open = None;
            }
            _ => {}
        }
    }
    regions
}

fn online_pass(
    diarizer: &mut Diarizer,
    files: &[ResampledChannelFile],
    request: &SpeakerPassRequest<'_>,
) -> Result<SpeakerPassOutcome, String> {
    let system_words: Vec<&SpeakerPassInputWord> = request
        .words
        .iter()
        .filter(|word| word.channel == 1)
        .collect();
    if system_words.is_empty() {
        return Ok(skipped("no system-audio words to attribute"));
    }
    let diarization = diarize(diarizer, files, ChannelAudio::File(1), request)?;
    let speaker_count = diarization.speaker_count() as u32;
    let mut outcome = SpeakerPassOutcome {
        diarized_channels: vec![1],
        speaker_count,
        ..Default::default()
    };
    if speaker_count <= 1 {
        // One voice (or none found): the channel already says who it is.
        outcome.hints.words = system_words
            .iter()
            .map(|word| SpeakerPassWord {
                word_id: word.id.clone(),
                channel: 1,
                speaker_index: None,
                confidence: 1.0,
                uncertain: false,
            })
            .collect();
        let clusters = vec![None; system_words.len()];
        let (names, tagged) =
            name_voices_from_platform(&system_words, &clusters, request.platform_spans);
        outcome.platform_names = names;
        outcome.platform_tagged_words = tagged;
        return Ok(outcome);
    }
    let labels = label_words(&system_words, &diarization);
    let clusters: Vec<Option<i32>> = labels
        .iter()
        .map(|label| label.speaker.map(|speaker| speaker as i32))
        .collect();
    let (names, tagged) =
        name_voices_from_platform(&system_words, &clusters, request.platform_spans);
    outcome.platform_names = names;
    outcome.platform_tagged_words = tagged;
    outcome.uncertain_words = labels.iter().filter(|label| label.uncertain).count() as u32;
    outcome.hints.words = system_words
        .iter()
        .zip(labels)
        .map(|(word, label)| SpeakerPassWord {
            word_id: word.id.clone(),
            channel: 1,
            speaker_index: label.speaker.map(|speaker| speaker as i32),
            confidence: label.confidence,
            uncertain: label.uncertain,
        })
        .collect();
    Ok(outcome)
}

fn in_person_pass(
    diarizer: &mut Diarizer,
    files: &[ResampledChannelFile],
    request: &SpeakerPassRequest<'_>,
) -> Result<SpeakerPassOutcome, String> {
    let words: Vec<&SpeakerPassInputWord> = if files.len() >= 2 {
        request
            .words
            .iter()
            .filter(|word| word.channel != 1)
            .collect()
    } else {
        request.words.iter().collect()
    };
    if words.is_empty() {
        return Ok(skipped("no microphone words to attribute"));
    }
    let diarization = diarize(diarizer, files, ChannelAudio::File(0), request)?;
    let speaker_count = diarization.speaker_count();
    let mut outcome = SpeakerPassOutcome {
        diarized_channels: vec![2],
        speaker_count: speaker_count as u32,
        ..Default::default()
    };
    if speaker_count == 0 {
        outcome.diarized_channels.clear();
        outcome.skipped_reason = Some("no speech found in the recording".to_owned());
        return Ok(outcome);
    }

    let scores: Vec<f32> = match request.self_voiceprint {
        Some(print) => diarization
            .speakers
            .iter()
            .map(|speaker| cosine_similarity(&speaker.centroid, &print.embedding).unwrap_or(-1.0))
            .collect(),
        None => Vec::new(),
    };
    let self_cluster = pick_self_cluster(&scores);
    outcome.self_match_score = scores.iter().copied().max_by(f32::total_cmp);
    outcome.self_matched = self_cluster.is_some();
    outcome.cluster_self_scores = scores.clone();

    if speaker_count == 1 && request.self_voiceprint.is_none() {
        // One voice and nothing to compare it with: the mic channel already
        // says this is the user.
        outcome.diarized_channels.clear();
        outcome.hints.words = words
            .iter()
            .map(|word| SpeakerPassWord {
                word_id: word.id.clone(),
                channel: word.channel,
                speaker_index: None,
                confidence: 1.0,
                uncertain: false,
            })
            .collect();
        return Ok(outcome);
    }

    let labels = label_words(&words, &diarization);
    outcome.uncertain_words = labels.iter().filter(|label| label.uncertain).count() as u32;
    outcome.hints.words = words
        .iter()
        .zip(labels)
        .map(|(word, label)| SpeakerPassWord {
            word_id: word.id.clone(),
            channel: 2,
            speaker_index: label.speaker.map(|speaker| speaker as i32),
            confidence: label.confidence,
            uncertain: label.uncertain,
        })
        .collect();
    if let (Some(cluster), Some(print)) = (self_cluster, request.self_voiceprint) {
        let index = diarization.speakers[cluster].index as i32;
        outcome.hints.assignments.push(SpeakerPassAssignment {
            channel: 2,
            speaker_index: Some(index),
            human_id: print.human_id.clone(),
            confidence: scores[cluster],
            source: AttributionSource::Voiceprint,
            name: None,
            uncertain: false,
        });
    }
    Ok(outcome)
}

/// The platform's indicator trails the voice it reflects (render delay plus
/// the sampling period), so a word is looked up slightly into the future.
const PLATFORM_LOOKAHEAD_MS: f64 = 1_500.0;
const PLATFORM_LOOKBEHIND_MS: f64 = 250.0;
/// A voice needs this many tagged words, and this share of its words
/// tagged, before any name is considered.
const PLATFORM_MIN_TAGGED_WORDS: usize = 5;
const PLATFORM_MIN_TAGGED_SHARE: f32 = 0.3;
/// The winning name must carry this share of the voice's tagged words.
const PLATFORM_MIN_NAME_SHARE: f32 = 0.7;

/// The one remote name the platform showed around a word, if exactly one.
pub fn platform_tag_for_word(
    word: &SpeakerPassInputWord,
    spans: &[PlatformSpeakerSpan],
) -> Option<String> {
    let mid = (word.start_ms + word.end_ms) / 2.0;
    let (from, to) = (mid - PLATFORM_LOOKBEHIND_MS, mid + PLATFORM_LOOKAHEAD_MS);
    let first = spans.partition_point(|span| span.end_ms <= from);
    let mut found: Option<&str> = None;
    for span in spans[first..].iter().take_while(|span| span.start_ms < to) {
        if span.end_ms <= from {
            continue;
        }
        for name in &span.names {
            match found {
                None => found = Some(name),
                Some(existing) if existing.eq_ignore_ascii_case(name) => {}
                Some(_) => return None,
            }
        }
    }
    found.map(str::to_owned)
}

/// Names voices on the system channel from platform active-speaker tags.
/// `clusters[i]` is the diarized voice of `words[i]` (`None` = the single
/// remote voice). A voice is named only when enough of its words were
/// tagged and one name clearly dominates; a name that dominates several
/// voices names them all but flags them uncertain (one device, several
/// people). Returns the namings and how many words carried a tag.
pub fn name_voices_from_platform(
    words: &[&SpeakerPassInputWord],
    clusters: &[Option<i32>],
    spans: &[PlatformSpeakerSpan],
) -> (Vec<PlatformNaming>, u32) {
    if spans.is_empty() {
        return (Vec::new(), 0);
    }
    let mut spans = spans.to_vec();
    spans.sort_by(|a, b| a.start_ms.total_cmp(&b.start_ms));

    struct Votes {
        total: usize,
        tagged: usize,
        names: Vec<(String, usize)>,
    }
    let mut voices: Vec<(Option<i32>, Votes)> = Vec::new();
    let mut tagged_words = 0u32;
    for (word, cluster) in words.iter().zip(clusters) {
        let position = match voices.iter().position(|(key, _)| key == cluster) {
            Some(position) => position,
            None => {
                voices.push((
                    *cluster,
                    Votes {
                        total: 0,
                        tagged: 0,
                        names: Vec::new(),
                    },
                ));
                voices.len() - 1
            }
        };
        let votes = &mut voices[position].1;
        votes.total += 1;
        let Some(name) = platform_tag_for_word(word, &spans) else {
            continue;
        };
        tagged_words += 1;
        votes.tagged += 1;
        match votes
            .names
            .iter_mut()
            .find(|(existing, _)| existing.eq_ignore_ascii_case(&name))
        {
            Some((_, count)) => *count += 1,
            None => votes.names.push((name, 1)),
        }
    }

    let mut namings: Vec<PlatformNaming> = voices
        .into_iter()
        .filter_map(|(speaker_index, votes)| {
            if votes.tagged < PLATFORM_MIN_TAGGED_WORDS
                || (votes.tagged as f32) < PLATFORM_MIN_TAGGED_SHARE * votes.total as f32
            {
                return None;
            }
            let (name, count) = votes
                .names
                .into_iter()
                .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0)))?;
            let share = count as f32 / votes.tagged as f32;
            (share >= PLATFORM_MIN_NAME_SHARE).then_some(PlatformNaming {
                speaker_index,
                name,
                confidence: share,
                uncertain: false,
                tagged_words: votes.tagged as u32,
            })
        })
        .collect();
    let shared: Vec<String> = namings
        .iter()
        .filter(|naming| {
            namings
                .iter()
                .filter(|other| other.name.eq_ignore_ascii_case(&naming.name))
                .count()
                >= 2
        })
        .map(|naming| naming.name.clone())
        .collect();
    for naming in &mut namings {
        if shared.contains(&naming.name) {
            naming.uncertain = true;
        }
    }
    namings.sort_by_key(|naming| naming.speaker_index);
    (namings, tagged_words)
}

/// Turns platform namings into speaker-pass assignments on the system
/// channel. `resolve` maps a platform name to a person id (a matching
/// calendar invitee, else a synthetic `platform:` id).
pub fn platform_assignments(
    names: &[PlatformNaming],
    resolve: impl Fn(&str) -> String,
) -> Vec<SpeakerPassAssignment> {
    names
        .iter()
        .map(|naming| SpeakerPassAssignment {
            channel: 1,
            speaker_index: naming.speaker_index,
            human_id: resolve(&naming.name),
            confidence: naming.confidence,
            source: AttributionSource::PlatformTag,
            name: Some(naming.name.clone()),
            uncertain: naming.uncertain,
        })
        .collect()
}

/// Index of the cluster that is unambiguously the enrolled user, if any.
pub fn pick_self_cluster(scores: &[f32]) -> Option<usize> {
    let (best, best_score) = scores
        .iter()
        .copied()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(&b.1))?;
    if best_score < SELF_MATCH_MIN_SCORE {
        return None;
    }
    let runner_up = scores
        .iter()
        .copied()
        .enumerate()
        .filter(|(index, _)| *index != best)
        .map(|(_, score)| score)
        .max_by(f32::total_cmp);
    match runner_up {
        Some(second) if best_score - second < SELF_MATCH_MIN_MARGIN => None,
        _ => Some(best),
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WordLabel {
    pub speaker: Option<usize>,
    pub confidence: f32,
    pub uncertain: bool,
}

fn overlap(a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.1.min(b.1) - a.0.max(b.0)).max(0.0)
}

/// Speaker for each word: largest overlap with a turn, else the nearest turn.
/// Confidence falls with a competing speaker, overlapped speech, or distance
/// from the turn; such words are flagged uncertain rather than dropped.
pub fn label_words(words: &[&SpeakerPassInputWord], diarization: &Diarization) -> Vec<WordLabel> {
    let speaker_slots = diarization
        .segments
        .iter()
        .map(|segment| segment.speaker + 1)
        .max()
        .unwrap_or(0);
    words
        .iter()
        .map(|word| {
            let span = (word.start_ms / 1000.0, word.end_ms / 1000.0);
            let duration = (span.1 - span.0).max(0.01);
            let mut cover = vec![0.0f64; speaker_slots];
            let mut nearest: Option<(f64, usize)> = None;
            for segment in &diarization.segments {
                let shared = overlap(span, (segment.start, segment.end));
                if shared > 0.0 {
                    cover[segment.speaker] += shared;
                } else {
                    let distance = if segment.end <= span.0 {
                        span.0 - segment.end
                    } else {
                        segment.start - span.1
                    };
                    if nearest.is_none_or(|(best, _)| distance < best) {
                        nearest = Some((distance, segment.speaker));
                    }
                }
            }
            let overlapped: f64 = diarization
                .overlaps
                .iter()
                .map(|region| overlap(span, *region))
                .sum::<f64>()
                / duration;
            let mut ranked: Vec<(usize, f64)> = cover
                .iter()
                .copied()
                .enumerate()
                .filter(|(_, share)| *share > 0.0)
                .map(|(speaker, share)| (speaker, (share / duration).min(1.0)))
                .collect();
            ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
            match ranked.first() {
                Some(&(speaker, best)) => {
                    let second = ranked.get(1).map_or(0.0, |(_, share)| *share);
                    let confidence = (best * (1.0 - second) * (1.0 - overlapped.min(1.0)))
                        .clamp(0.0, 1.0) as f32;
                    WordLabel {
                        speaker: Some(speaker),
                        confidence,
                        uncertain: overlapped >= OVERLAP_UNCERTAIN_SHARE
                            || second >= SECOND_SPEAKER_UNCERTAIN_SHARE
                            || confidence < LOW_CONFIDENCE,
                    }
                }
                None => match nearest {
                    Some((distance, speaker)) => WordLabel {
                        speaker: Some(speaker),
                        confidence: (0.5 - distance).clamp(0.1, 0.5) as f32,
                        uncertain: distance > GAP_UNCERTAIN_SECONDS,
                    },
                    None => WordLabel {
                        speaker: None,
                        confidence: 0.0,
                        uncertain: true,
                    },
                },
            }
        })
        .collect()
}

fn channel_mean_abs(file: &ResampledChannelFile) -> Result<f64, String> {
    let mut source =
        MixedAudioSource::for_audio(std::slice::from_ref(file), ChannelAudio::File(0))?;
    let total = source.sample_count().map_err(|error| error.to_string())?;
    let mut buffer = vec![0.0f32; SAMPLE_RATE * 10];
    let mut sum = 0.0f64;
    let mut start = 0;
    while start < total {
        source
            .read(start, &mut buffer)
            .map_err(|error| error.to_string())?;
        let valid = buffer.len().min(total - start);
        sum += buffer[..valid]
            .iter()
            .map(|sample| f64::from(sample.abs()))
            .sum::<f64>();
        start += buffer.len();
    }
    Ok(if total == 0 { 0.0 } else { sum / total as f64 })
}

/// A speaker embedding computed from speech-only audio.
#[derive(Debug, Clone, PartialEq)]
pub struct VoiceEmbedding {
    /// L2-normalised mean of per-chunk WeSpeaker embeddings.
    pub embedding: Vec<f32>,
    /// Detected speech, in seconds.
    pub speech_seconds: f32,
}

fn normalize(vector: &[f32]) -> Option<Vec<f32>> {
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    (norm > f32::EPSILON && norm.is_finite()).then(|| vector.iter().map(|v| v / norm).collect())
}

/// Speech-only samples of 16 kHz mono audio, using the pyannote segmentation
/// model as the voice-activity detector (any active local speaker = speech).
pub fn speech_samples(samples: &[f32]) -> Result<Vec<f32>, String> {
    let mut segmenter = Segmenter::new().map_err(|error| error.to_string())?;
    let mut speech = Vec::new();
    let mut window = vec![0.0f32; WINDOW_SAMPLES];
    let mut start = 0;
    while start < samples.len() {
        let valid = WINDOW_SAMPLES.min(samples.len() - start);
        window[..valid].copy_from_slice(&samples[start..start + valid]);
        window[valid..].fill(0.0);
        let activity = segmenter
            .run_window(&window)
            .map_err(|error| error.to_string())?;
        let frames = frames_for_samples(valid).min(activity.frames.len());
        for frame in 0..frames {
            if !activity.frames[frame].iter().any(|active| *active) {
                continue;
            }
            let from = if frame == 0 {
                0
            } else {
                frame_start_sample(frame)
            };
            let to = frame_start_sample(frame + 1).min(valid);
            if from < to {
                speech.extend_from_slice(&window[from..to]);
            }
        }
        start += WINDOW_SAMPLES;
    }
    Ok(speech)
}

/// Embeds the speech in `samples` (16 kHz mono): voice-activity trimmed, cut
/// into ~3 s chunks, each embedded and normalised, then averaged and
/// normalised again. Errors when no chunk yields an embedding.
pub fn voice_embedding(samples: &[f32]) -> Result<VoiceEmbedding, String> {
    let speech = speech_samples(samples)?;
    let speech_seconds = speech.len() as f32 / SAMPLE_RATE as f32;
    let chunk = (ENROLLMENT_CHUNK_SECONDS * SAMPLE_RATE as f32) as usize;
    let mut extractor =
        anlg_embedding::EmbeddingExtractor::new().map_err(|error| error.to_string())?;
    let mut sum: Vec<f32> = Vec::new();
    let mut count = 0usize;
    let mut start = 0;
    while start < speech.len() {
        let mut end = (start + chunk).min(speech.len());
        // Fold a short tail into this chunk instead of embedding a fragment.
        if speech.len() - end < chunk / 2 {
            end = speech.len();
        }
        if end - start >= SAMPLE_RATE
            && let Some(embedding) = extractor
                .compute_optional(&speech[start..end])
                .map_err(|error| error.to_string())?
                .and_then(|embedding| normalize(&embedding))
        {
            if sum.is_empty() {
                sum = vec![0.0; embedding.len()];
            }
            for (total, value) in sum.iter_mut().zip(&embedding) {
                *total += value;
            }
            count += 1;
        }
        start = end;
    }
    if count == 0 {
        return Err("Not enough speech, try again".to_owned());
    }
    let embedding = normalize(&sum).ok_or_else(|| "Not enough speech, try again".to_owned())?;
    Ok(VoiceEmbedding {
        embedding,
        speech_seconds,
    })
}

/// Embeds the speech inside `[start_ms, end_ms)` of one recorded channel
/// (0 = mic, 1 = system audio; the mixed channel 2 reads the mic).
pub fn embed_audio_span(
    audio_path: &Path,
    channel: i32,
    start_ms: i64,
    end_ms: i64,
) -> Result<VoiceEmbedding, String> {
    if end_ms <= start_ms {
        return Err("empty audio span".to_owned());
    }
    let path_str = audio_path
        .to_str()
        .ok_or_else(|| "audio path is not valid UTF-8".to_string())?;
    let source =
        anlg_audio_utils::source_from_path(audio_path).map_err(|error| error.to_string())?;
    let files = resample_audio_to_channel_files_until(path_str, source, || false)?;
    let index = match channel {
        1 if files.len() >= 2 => 1,
        1 => return Err("recording has no separate system-audio channel".to_owned()),
        _ => 0,
    };
    let mut source = MixedAudioSource::for_audio(&files, ChannelAudio::File(index))?;
    let start = (start_ms.max(0) as usize) * SAMPLE_RATE / 1000;
    let end = (end_ms.max(0) as usize) * SAMPLE_RATE / 1000;
    let total = source.sample_count().map_err(|error| error.to_string())?;
    let end = end.min(total);
    if end <= start {
        return Err("audio span is outside the recording".to_owned());
    }
    let mut samples = vec![0.0f32; end - start];
    source
        .read(start, &mut samples)
        .map_err(|error| error.to_string())?;
    voice_embedding(&samples)
}

/// Weighted blend of a stored voiceprint with a new embedding, both weighted
/// by the speech seconds behind them. The stored side is capped so a long
/// history still lets new evidence move the print.
pub fn blend_voiceprints(
    stored: &[f32],
    stored_seconds: f32,
    new: &[f32],
    new_seconds: f32,
) -> Option<Vec<f32>> {
    if stored.len() != new.len() {
        return None;
    }
    let stored_weight = stored_seconds.clamp(1.0, 120.0);
    let new_weight = new_seconds.max(0.0);
    let stored = normalize(stored)?;
    let new = normalize(new)?;
    let mixed: Vec<f32> = stored
        .iter()
        .zip(&new)
        .map(|(a, b)| a * stored_weight + b * new_weight)
        .collect();
    normalize(&mixed)
}

/// Loads 16 kHz mono samples from an audio file (any channel layout is mixed
/// down). For tests and enrollment fixtures.
pub fn load_mono_16k(path: &Path) -> Result<Vec<f32>, String> {
    let source = anlg_audio_utils::source_from_path(path).map_err(|error| error.to_string())?;
    anlg_audio_utils::resample_audio(source, SAMPLE_RATE as u32).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use anlg_pyannote_local::SpeakerSegment;

    fn word(start_ms: f64, end_ms: f64) -> SpeakerPassInputWord {
        SpeakerPassInputWord {
            id: format!("{start_ms}"),
            start_ms,
            end_ms,
            channel: 1,
        }
    }

    fn segment(start: f64, end: f64, speaker: usize) -> SpeakerSegment {
        SpeakerSegment {
            start,
            end,
            speaker,
        }
    }

    fn span(start: f64, end: f64, names: &[&str]) -> PlatformSpeakerSpan {
        PlatformSpeakerSpan {
            start_ms: start,
            end_ms: end,
            names: names.iter().map(|name| name.to_string()).collect(),
        }
    }

    /// Words every 400 ms from `from` to `to` (ms).
    fn words_between(from: f64, to: f64) -> Vec<SpeakerPassInputWord> {
        let mut words = Vec::new();
        let mut at = from;
        while at < to {
            words.push(word(at, at + 300.0));
            at += 400.0;
        }
        words
    }

    #[test]
    fn platform_tags_name_each_diarized_voice() {
        // Sam talks 0-10 s, Priya 10-20 s; the indicator trails by ~1 s.
        let mut words = words_between(0.0, 10_000.0);
        let sam = words.len();
        words.extend(words_between(10_000.0, 20_000.0));
        let refs: Vec<&SpeakerPassInputWord> = words.iter().collect();
        let clusters: Vec<Option<i32>> = (0..words.len())
            .map(|i| Some(if i < sam { 0 } else { 1 }))
            .collect();
        let spans = vec![
            span(1_000.0, 11_000.0, &["Sam Lee"]),
            span(11_000.0, 21_000.0, &["Priya Raman"]),
        ];
        let (names, tagged) = name_voices_from_platform(&refs, &clusters, &spans);
        // Words at the hand-over see both names in the look-ahead and stay untagged.
        assert!(tagged as usize >= words.len() - 6, "tagged {tagged}");
        assert_eq!(
            names
                .iter()
                .map(|n| (n.speaker_index, n.name.as_str(), n.uncertain))
                .collect::<Vec<_>>(),
            vec![(Some(0), "Sam Lee", false), (Some(1), "Priya Raman", false)]
        );
        assert!(names.iter().all(|n| n.confidence >= 0.9));
    }

    #[test]
    fn platform_tags_need_a_clear_majority_and_enough_coverage() {
        let words = words_between(0.0, 10_000.0);
        let refs: Vec<&SpeakerPassInputWord> = words.iter().collect();
        let clusters = vec![None; words.len()];
        // Two names alternate on one voice: no name.
        let alternating: Vec<_> = (0..10)
            .map(|i| {
                let name = if i % 2 == 0 { "Sam" } else { "Priya" };
                span(i as f64 * 1_000.0, (i + 1) as f64 * 1_000.0, &[name])
            })
            .collect();
        assert!(
            name_voices_from_platform(&refs, &clusters, &alternating)
                .0
                .is_empty()
        );
        // A single tag over a long voice is not enough evidence.
        let sparse = vec![span(4_000.0, 4_600.0, &["Sam"])];
        assert!(
            name_voices_from_platform(&refs, &clusters, &sparse)
                .0
                .is_empty()
        );
        // Two people marked speaking at once tag nothing.
        let overlap = vec![span(0.0, 11_000.0, &["Sam", "Priya"])];
        let (names, tagged) = name_voices_from_platform(&refs, &clusters, &overlap);
        assert!(names.is_empty());
        assert_eq!(tagged, 0);
        // No timeline at all.
        assert!(
            name_voices_from_platform(&refs, &clusters, &[])
                .0
                .is_empty()
        );
    }

    #[test]
    fn one_name_on_two_voices_is_a_shared_device() {
        let mut words = words_between(0.0, 10_000.0);
        let first = words.len();
        words.extend(words_between(10_000.0, 20_000.0));
        let refs: Vec<&SpeakerPassInputWord> = words.iter().collect();
        let clusters: Vec<Option<i32>> = (0..words.len())
            .map(|i| Some(if i < first { 0 } else { 1 }))
            .collect();
        let spans = vec![span(0.0, 21_000.0, &["Boardroom"])];
        let (names, _) = name_voices_from_platform(&refs, &clusters, &spans);
        assert_eq!(names.len(), 2);
        assert!(names.iter().all(|n| n.uncertain && n.name == "Boardroom"));
    }

    #[test]
    fn a_single_remote_voice_takes_the_platform_name() {
        let words = words_between(0.0, 8_000.0);
        let refs: Vec<&SpeakerPassInputWord> = words.iter().collect();
        let clusters = vec![None; words.len()];
        let spans = vec![span(500.0, 9_000.0, &["Sam Lee"])];
        let (names, _) = name_voices_from_platform(&refs, &clusters, &spans);
        assert_eq!(names.len(), 1);
        assert_eq!(names[0].speaker_index, None);
        assert_eq!(names[0].name, "Sam Lee");
    }

    #[test]
    fn words_in_overlap_are_uncertain() {
        let diarization = Diarization {
            segments: vec![segment(0.0, 2.0, 0), segment(1.5, 4.0, 1)],
            speakers: vec![],
            overlaps: vec![(1.5, 2.0)],
        };
        let words = [
            word(100.0, 600.0),
            word(1550.0, 1950.0),
            word(3000.0, 3500.0),
        ];
        let refs: Vec<_> = words.iter().collect();
        let labels = label_words(&refs, &diarization);
        assert_eq!(labels[0].speaker, Some(0));
        assert!(!labels[0].uncertain);
        assert!(labels[1].uncertain);
        assert!(labels[1].confidence < LOW_CONFIDENCE);
        assert_eq!(labels[2].speaker, Some(1));
        assert!(!labels[2].uncertain);
    }

    #[test]
    fn words_far_from_turns_are_uncertain() {
        let diarization = Diarization {
            segments: vec![segment(0.0, 1.0, 0)],
            speakers: vec![],
            overlaps: vec![],
        };
        let words = [word(1050.0, 1200.0), word(3000.0, 3200.0)];
        let refs: Vec<_> = words.iter().collect();
        let labels = label_words(&refs, &diarization);
        assert!(!labels[0].uncertain);
        assert!(labels[1].uncertain);
    }

    #[test]
    fn self_cluster_needs_score_and_margin() {
        assert_eq!(pick_self_cluster(&[0.8, 0.2]), Some(0));
        assert_eq!(pick_self_cluster(&[0.3, 0.8]), Some(1));
        assert_eq!(pick_self_cluster(&[0.4, 0.2]), None);
        assert_eq!(pick_self_cluster(&[0.7, 0.65]), None);
        assert_eq!(pick_self_cluster(&[0.7]), Some(0));
        assert_eq!(pick_self_cluster(&[]), None);
    }

    #[test]
    fn blending_moves_toward_new_evidence() {
        let blended = blend_voiceprints(&[1.0, 0.0], 30.0, &[0.0, 1.0], 30.0).unwrap();
        assert!((blended[0] - blended[1]).abs() < 1e-5);
        assert!(blend_voiceprints(&[1.0], 1.0, &[1.0, 0.0], 1.0).is_none());
    }
}
