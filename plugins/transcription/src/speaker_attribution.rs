//! `attribute_session_speakers`: the post-stop speaker pass over a saved
//! transcript and its retained session audio. See
//! `listener2_core::speaker_pass` for the attribution rules and
//! `anlg_transcript::attribution` for the stored hint format.

use anlg_transcript::{
    StoredTranscriptWord, apply_speaker_pass_hints, manually_assigned_word_ids,
    normalize_stored_transcript, parse_stored_speaker_hints,
};
use anlg_transcription_core::listener2::speaker_pass::{
    SpeakerPassInputWord, SpeakerPassMode, SpeakerPassRequest, run_speaker_pass,
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
        }
    }
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
    session_id: String,
    transcript_id: String,
    mode: SpeakerPassMode,
) -> Result<AttributionSummary, String> {
    let pool = app
        .try_state::<tauri_plugin_db::ManagedState>()
        .map(|state| state.pool().clone())
        .ok_or_else(|| "database is not ready yet".to_string())?;

    let Some((words_json, hints_json)) = sqlx::query_as::<_, (String, String)>(
        "SELECT words_json, speaker_hints_json FROM transcripts
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

    let outcome = tauri::async_runtime::spawn_blocking(move || {
        let never = || false;
        run_speaker_pass(&SpeakerPassRequest {
            audio_path: &audio_path,
            words: &input,
            mode,
            self_voiceprint: self_print.as_ref(),
            is_cancelled: &never,
        })
    })
    .await
    .map_err(|error| error.to_string())??;

    let summary = AttributionSummary {
        diarized_channels: outcome.diarized_channels.clone(),
        speaker_count: outcome.speaker_count,
        self_match_score: outcome.self_match_score,
        self_matched: outcome.self_matched,
        uncertain_words: outcome.uncertain_words,
        skipped_reason: outcome.skipped_reason.clone(),
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
