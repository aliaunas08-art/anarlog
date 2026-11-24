//! `attribute_session_speakers`: the post-stop speaker pass over a saved
//! transcript and its retained session audio. See
//! `listener2_core::speaker_pass` for the attribution rules and
//! `anlg_transcript::attribution` for the stored hint format.

use anlg_transcript::{
    StoredTranscriptWord, apply_speaker_pass_hints, manually_assigned_word_ids,
    normalize_stored_transcript, parse_stored_speaker_hints,
};
use anlg_transcription_core::listener2::speaker_pass::{
    PlatformSpeakerSpan, SpeakerPassInputWord, SpeakerPassMode, SpeakerPassRequest,
    platform_assignments, run_speaker_pass,
};
use tauri::Manager;
use tauri_plugin_fs_sync::FsSyncPluginExt;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct AttributionSummary {
    pub diarized_channels: Vec<i32>,
    pub speaker_count: u32,
    pub self_match_score: Option<f32>,
    pub self_matched: bool,
    pub uncertain_words: u32,
    pub skipped_reason: Option<String>,
    /// Remote voices named from the meeting platform's active-speaker tags.
    #[serde(default)]
    pub platform_named_speakers: u32,
}

impl AttributionSummary {
    fn skipped(reason: impl Into<String>) -> Self {
        Self {
            diarized_channels: Vec::new(),
            speaker_count: 0,
            self_match_score: None,
            self_matched: false,
            uncertain_words: 0,
            skipped_reason: Some(reason.into()),
            platform_named_speakers: 0,
        }
    }
}

/// A person the platform name may refer to.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct KnownPerson {
    pub human_id: String,
    pub name: String,
}

fn normalize_name(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Converts the stored active-speaker timeline (Unix ms) onto the
/// transcript clock, dropping the user's own names.
pub(crate) fn platform_spans_from_timeline(
    timeline: &serde_json::Value,
    transcript_started_at_ms: i64,
    self_names: &[String],
) -> Vec<PlatformSpeakerSpan> {
    if transcript_started_at_ms <= 0 {
        return Vec::new();
    }
    let own: Vec<String> = self_names
        .iter()
        .map(|name| normalize_name(name))
        .filter(|name| !name.is_empty())
        .collect();
    let Some(intervals) = timeline.get("intervals").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    intervals
        .iter()
        .filter_map(|interval| {
            let start = interval.get("start_ms")?.as_f64()?;
            let end = interval.get("end_ms")?.as_f64()?;
            let names: Vec<String> = interval
                .get("speakers")?
                .as_array()?
                .iter()
                .filter_map(|name| name.as_str())
                .map(str::trim)
                .filter(|name| !name.is_empty() && !own.contains(&normalize_name(name)))
                .map(str::to_owned)
                .collect();
            (end > start && !names.is_empty()).then(|| PlatformSpeakerSpan {
                start_ms: start - transcript_started_at_ms as f64,
                end_ms: end - transcript_started_at_ms as f64,
                names,
            })
        })
        .collect()
}

/// A platform name resolves to the one meeting participant with that exact
/// name (case and spacing aside); otherwise to a synthetic `platform:` id
/// that renders as the name. No fuzzy or first-name matching.
pub(crate) fn resolve_platform_name(name: &str, participants: &[KnownPerson]) -> String {
    let wanted = normalize_name(name);
    let mut matches = participants
        .iter()
        .filter(|person| normalize_name(&person.name) == wanted);
    match (matches.next(), matches.next()) {
        (Some(person), None) => person.human_id.clone(),
        _ => anlg_transcript::platform_human_id(name),
    }
}

struct PlatformContext {
    spans: Vec<PlatformSpeakerSpan>,
    participants: Vec<KnownPerson>,
}

async fn load_platform_context(
    pool: &sqlx::SqlitePool,
    session_id: &str,
    transcript_started_at_ms: i64,
) -> Result<PlatformContext, String> {
    let row = sqlx::query_as::<_, (Option<String>, Option<String>, Option<String>)>(
        "SELECT json_extract(s.metadata_json, '$.platform_speakers'),
                h.name,
                json_extract(h.metadata_json, '$.aliases')
         FROM sessions s
         LEFT JOIN humans h ON h.id = s.owner_user_id AND h.deleted_at IS NULL
         WHERE s.id = ? AND s.deleted_at IS NULL
         LIMIT 1",
    )
    .bind(session_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| error.to_string())?;
    let Some((Some(timeline), owner_name, aliases)) = row else {
        return Ok(PlatformContext {
            spans: Vec::new(),
            participants: Vec::new(),
        });
    };
    let timeline: serde_json::Value = serde_json::from_str(&timeline).unwrap_or_default();
    let mut self_names: Vec<String> = owner_name.into_iter().collect();
    if let Some(aliases) =
        aliases.and_then(|raw| serde_json::from_str::<Vec<serde_json::Value>>(&raw).ok())
    {
        self_names.extend(aliases.iter().filter_map(|a| a.as_str().map(str::to_owned)));
    }
    let spans = platform_spans_from_timeline(&timeline, transcript_started_at_ms, &self_names);
    let participants = sqlx::query_as::<_, (String, String)>(
        "SELECT DISTINCT h.id, h.name
         FROM session_participants p
         JOIN humans h ON h.id = p.human_id AND h.deleted_at IS NULL
         WHERE p.session_id = ? AND p.deleted_at IS NULL AND p.source <> 'excluded'
           AND h.name <> ''",
    )
    .bind(session_id)
    .fetch_all(pool)
    .await
    .map_err(|error| error.to_string())?
    .into_iter()
    .map(|(human_id, name)| KnownPerson { human_id, name })
    .collect();
    Ok(PlatformContext {
        spans,
        participants,
    })
}

/// Words with timing that the pass may relabel: everything except words the
/// user attributed by hand.
pub(crate) fn pass_input_words(
    words: &[StoredTranscriptWord],
    hints: &[anlg_transcript::StoredSpeakerHint],
) -> Vec<SpeakerPassInputWord> {
    let locked = manually_assigned_word_ids(words, hints);
    words
        .iter()
        .filter(|word| !locked.contains(&word.id))
        .filter_map(|word| {
            let (Some(start_ms), Some(end_ms)) = (word.start_ms, word.end_ms) else {
                return None;
            };
            (end_ms > start_ms).then(|| SpeakerPassInputWord {
                id: word.id.clone(),
                start_ms,
                end_ms,
                channel: word.channel.unwrap_or(0.0).round() as i32,
            })
        })
        .collect()
}

#[tauri::command]
#[specta::specta]
pub async fn attribute_session_speakers<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
    session_id: String,
    transcript_id: String,
    mode: SpeakerPassMode,
) -> Result<AttributionSummary, String> {
    anlg_aspc::require_ignoring_lock(
        &aspc_window,
        &aspc_window,
        anlg_aspc::DataType::Transcript,
        anlg_aspc::Action::Edit,
    )
    .map_err(|denied| denied.to_string())?;
    let pool = app
        .try_state::<tauri_plugin_db::ManagedState>()
        .map(|state| state.pool().clone())
        .ok_or_else(|| "database is not ready yet".to_string())?;

    let Some((words_json, hints_json, started_at_ms)) = sqlx::query_as::<_, (String, String, i64)>(
        "SELECT words_json, speaker_hints_json, started_at_ms FROM transcripts
         WHERE id = ? AND session_id = ? AND deleted_at IS NULL
         LIMIT 1",
    )
    .bind(&transcript_id)
    .bind(&session_id)
    .fetch_optional(&pool)
    .await
    .map_err(|error| error.to_string())?
    else {
        return Ok(AttributionSummary::skipped("transcript not found"));
    };

    let raw_words: Vec<serde_json::Value> =
        serde_json::from_str(&words_json).map_err(|error| error.to_string())?;
    let (words, hints) = normalize_stored_transcript(
        &transcript_id,
        raw_words,
        parse_stored_speaker_hints(&hints_json),
    );
    let input = pass_input_words(&words, &hints);
    if input.is_empty() {
        return Ok(AttributionSummary::skipped(
            "no words left to attribute (all assigned by hand or untimed)",
        ));
    }

    // Retention must not delete the audio while it is being read.
    let _audio_guard = app.fs_sync().lock_session_audio(&session_id).await;
    let Some(audio_path) = app.fs_sync().session_audio_path(&session_id)? else {
        return Ok(AttributionSummary::skipped(
            "session audio is not available",
        ));
    };
    let self_print = crate::voice_enrollment::self_voiceprint_for_session(&app, &session_id).await;
    // Platform tags are optional evidence: a failed lookup only means the
    // pass falls back to diarized letters.
    let platform = load_platform_context(&pool, &session_id, started_at_ms)
        .await
        .unwrap_or_else(|error| {
            tracing::warn!(%error, "platform_speaker_context_unavailable");
            PlatformContext {
                spans: Vec::new(),
                participants: Vec::new(),
            }
        });
    let platform_spans = platform.spans;

    let mut outcome = tauri::async_runtime::spawn_blocking(move || {
        let never = || false;
        run_speaker_pass(&SpeakerPassRequest {
            audio_path: &audio_path,
            words: &input,
            mode,
            self_voiceprint: self_print.as_ref(),
            platform_spans: &platform_spans,
            is_cancelled: &never,
        })
    })
    .await
    .map_err(|error| error.to_string())??;
    let participants = platform.participants;
    outcome
        .hints
        .assignments
        .extend(platform_assignments(&outcome.platform_names, |name| {
            resolve_platform_name(name, &participants)
        }));

    let summary = AttributionSummary {
        diarized_channels: outcome.diarized_channels.clone(),
        speaker_count: outcome.speaker_count,
        self_match_score: outcome.self_match_score,
        self_matched: outcome.self_matched,
        uncertain_words: outcome.uncertain_words,
        skipped_reason: outcome.skipped_reason.clone(),
        platform_named_speakers: outcome.platform_names.len() as u32,
    };
    if outcome.hints.words.is_empty() {
        return Ok(summary);
    }

    let next = apply_speaker_pass_hints(&hints, &outcome.hints);
    let next_json = serde_json::to_string(&next).map_err(|error| error.to_string())?;
    let now = chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string();
    // Optimistic: only write over the exact row content the pass read, and
    // bump content_revision so live queries and sync see the change.
    let result = sqlx::query(
        "UPDATE transcripts
         SET speaker_hints_json = ?, content_revision = content_revision + 1, updated_at = ?
         WHERE id = ? AND session_id = ? AND words_json = ? AND speaker_hints_json = ?
           AND deleted_at IS NULL",
    )
    .bind(&next_json)
    .bind(&now)
    .bind(&transcript_id)
    .bind(&session_id)
    .bind(&words_json)
    .bind(&hints_json)
    .execute(&pool)
    .await
    .map_err(|error| error.to_string())?;
    if result.rows_affected() == 0 {
        return Err(
            "The transcript changed while speakers were being identified; try again".into(),
        );
    }
    tracing::info!(
        session_id = %session_id,
        transcript_id = %transcript_id,
        speakers = summary.speaker_count,
        self_matched = summary.self_matched,
        uncertain_words = summary.uncertain_words,
        platform_named = summary.platform_named_speakers,
        platform_tagged_words = outcome.platform_tagged_words,
        "speaker_attribution_completed"
    );
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anlg_transcript::StoredSpeakerHint;

    fn word(id: &str, channel: f64) -> StoredTranscriptWord {
        StoredTranscriptWord {
            id: id.into(),
            text: Some(id.into()),
            start_ms: Some(0.0),
            end_ms: Some(100.0),
            channel: Some(channel),
            speaker: None,
            metadata: None,
        }
    }

    #[test]
    fn timeline_moves_onto_the_transcript_clock_without_the_user() {
        let timeline = serde_json::json!({
            "version": 1,
            "intervals": [
                {"start_ms": 1_000_500, "end_ms": 1_002_000, "speakers": ["Sam Lee"], "self_speaking": false},
                {"start_ms": 1_002_000, "end_ms": 1_003_000, "speakers": ["ali  aunas"], "self_speaking": false},
                {"start_ms": 1_003_000, "end_ms": 1_004_000, "speakers": [], "self_speaking": true},
                {"start_ms": 1_005_000, "end_ms": 1_004_000, "speakers": ["Bad"], "self_speaking": false}
            ]
        });
        let spans = platform_spans_from_timeline(&timeline, 1_000_000, &["Ali Aunas".to_string()]);
        assert_eq!(
            spans,
            vec![PlatformSpeakerSpan {
                start_ms: 500.0,
                end_ms: 2_000.0,
                names: vec!["Sam Lee".into()],
            }]
        );
        assert!(platform_spans_from_timeline(&timeline, 0, &[]).is_empty());
        assert!(platform_spans_from_timeline(&serde_json::json!({}), 1, &[]).is_empty());
    }

    #[test]
    fn platform_names_match_only_an_exact_unique_participant() {
        let people = vec![
            KnownPerson {
                human_id: "h-sam".into(),
                name: "Sam Lee".into(),
            },
            KnownPerson {
                human_id: "h-jo1".into(),
                name: "Jo Park".into(),
            },
            KnownPerson {
                human_id: "h-jo2".into(),
                name: "jo  park".into(),
            },
        ];
        assert_eq!(resolve_platform_name("sam  LEE", &people), "h-sam");
        assert_eq!(resolve_platform_name("Sam", &people), "platform:Sam");
        assert_eq!(
            resolve_platform_name("Jo Park", &people),
            "platform:Jo Park"
        );
        assert_eq!(resolve_platform_name("Priya", &[]), "platform:Priya");
    }

    #[test]
    fn manually_assigned_words_are_never_passed_to_the_speaker_pass() {
        let words = vec![word("a", 1.0), word("b", 1.0), word("c", 1.0)];
        let hints = vec![
            StoredSpeakerHint {
                id: "a:p".into(),
                word_id: Some("a".into()),
                hint_type: "provider_speaker_index".into(),
                value: serde_json::json!({"speaker_index": 0, "channel": 1}),
            },
            StoredSpeakerHint {
                id: "c:p".into(),
                word_id: Some("c".into()),
                hint_type: "provider_speaker_index".into(),
                value: serde_json::json!({"speaker_index": 1, "channel": 1}),
            },
            StoredSpeakerHint {
                id: "a:u".into(),
                word_id: Some("a".into()),
                hint_type: "user_speaker_assignment".into(),
                value: serde_json::Value::String(
                    serde_json::json!({"human_id": "h", "scope": "speaker", "channel": 1, "speaker_index": 0})
                        .to_string(),
                ),
            },
        ];
        let input = pass_input_words(&words, &hints);
        let ids: Vec<_> = input.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(ids, vec!["b", "c"]);
    }
}
