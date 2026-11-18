//! The user's own voiceprint, enrolled explicitly and kept only in the OS
//! keychain (store2 secret scope [`VOICE_ENROLLMENT_SCOPE`], native-only).
//!
//! Enrollment captures the default microphone in Rust, trims it to speech
//! with the segmentation model, requires
//! [`MIN_ENROLLMENT_SPEECH_SECONDS`] of speech, and stores the L2-normalised
//! mean of ~3 s WeSpeaker chunk embeddings. Audio stays in memory and is
//! dropped after the embedding is computed. Neither audio nor embeddings are
//! ever logged, synced, exported or written to the database.
//!
//! While capturing, a [`VoiceEnrollmentLevelEvent`] is emitted about every
//! 100 ms (`rms` 0..1 of the last chunk, `seconds` captured so far) so the UI
//! can draw a level meter.

use std::sync::{Arc, Mutex};

use anlg_transcription_core::listener2::speaker_pass::{
    MIN_ENROLLMENT_SPEECH_SECONDS, MIN_SPAN_SPEECH_SECONDS, SelfVoiceprint, blend_voiceprints,
    embed_audio_span, voice_embedding,
};
use futures_util::StreamExt;
use tauri::Manager;
use tauri_plugin_fs_sync::FsSyncPluginExt;
use tauri_specta::Event;
use tokio_util::sync::CancellationToken;

/// Keychain scope of the enrolled voiceprint. Native-only in store2; not a
/// database table, so no sync, E2EE replication or export can reach it.
pub const VOICE_ENROLLMENT_SCOPE: &str = "voice_enrollment";
const VOICE_ENROLLMENT_KEY: &str = "self";
const MODEL_PROVIDER: &str = "pyannote";
const MODEL_VERSION: &str = "wespeaker-embedding-onnx-1";
const SAMPLE_RATE: u32 = 16_000;
const CHUNK_SIZE: usize = 1_600;
const MAX_CAPTURE_SECONDS: usize = 120;
pub const NOT_ENOUGH_SPEECH: &str = "Not enough speech, try again";

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct VoiceEnrollmentStatus {
    pub enrolled: bool,
    pub created_at_ms: Option<i64>,
    pub updated_at_ms: Option<i64>,
    pub speech_seconds: Option<f32>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type, tauri_specta::Event)]
pub struct VoiceEnrollmentLevelEvent {
    pub rms: f32,
    pub seconds: f32,
}

/// Stored secret value (JSON). `embedding` is base64 little-endian f32.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct EnrollmentRecord {
    pub version: u32,
    pub model_provider: String,
    pub model_version: String,
    pub embedding: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub speech_seconds: f32,
}

impl EnrollmentRecord {
    pub(crate) fn new(embedding: &[f32], speech_seconds: f32, now_ms: i64) -> Self {
        Self {
            version: 1,
            model_provider: MODEL_PROVIDER.to_owned(),
            model_version: MODEL_VERSION.to_owned(),
            embedding: crate::voiceprint::encode_embedding(embedding),
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
            speech_seconds,
        }
    }

    pub(crate) fn parse(raw: &str) -> Option<Self> {
        let record: Self = serde_json::from_str(raw).ok()?;
        (record.model_provider == MODEL_PROVIDER && record.model_version == MODEL_VERSION)
            .then_some(record)
    }

    pub(crate) fn embedding(&self) -> Option<Vec<f32>> {
        crate::voiceprint::decode_embedding(&self.embedding)
    }

    pub(crate) fn status(record: Option<&Self>) -> VoiceEnrollmentStatus {
        match record {
            Some(record) => VoiceEnrollmentStatus {
                enrolled: true,
                created_at_ms: Some(record.created_at_ms),
                updated_at_ms: Some(record.updated_at_ms),
                speech_seconds: Some(record.speech_seconds),
            },
            None => VoiceEnrollmentStatus {
                enrolled: false,
                created_at_ms: None,
                updated_at_ms: None,
                speech_seconds: None,
            },
        }
    }
}

/// Where the record lives; the keychain in the app, memory in tests.
pub(crate) trait EnrollmentSecretStore {
    fn read(&self) -> Result<Option<String>, String>;
    fn write(&self, value: &str) -> Result<(), String>;
    fn delete(&self) -> Result<(), String>;
}

struct KeychainStore<'a, R: tauri::Runtime>(&'a tauri::AppHandle<R>);

impl<R: tauri::Runtime> EnrollmentSecretStore for KeychainStore<'_, R> {
    fn read(&self) -> Result<Option<String>, String> {
        tauri_plugin_store2::read_secret_blocking(
            self.0,
            VOICE_ENROLLMENT_SCOPE,
            VOICE_ENROLLMENT_KEY,
        )
    }
    fn write(&self, value: &str) -> Result<(), String> {
        tauri_plugin_store2::write_secret_blocking(
            self.0,
            VOICE_ENROLLMENT_SCOPE,
            VOICE_ENROLLMENT_KEY,
            value,
        )
    }
    fn delete(&self) -> Result<(), String> {
        tauri_plugin_store2::delete_secret_blocking(
            self.0,
            VOICE_ENROLLMENT_SCOPE,
            VOICE_ENROLLMENT_KEY,
        )
    }
}

pub(crate) fn load_record(
    store: &dyn EnrollmentSecretStore,
) -> Result<Option<EnrollmentRecord>, String> {
    Ok(store.read()?.as_deref().and_then(EnrollmentRecord::parse))
}

/// The enrolled voiceprint named as `human_id`, if one is stored.
pub(crate) fn load_self_voiceprint(
    store: &dyn EnrollmentSecretStore,
    human_id: &str,
) -> Option<SelfVoiceprint> {
    if human_id.trim().is_empty() {
        return None;
    }
    let record = load_record(store).ok()??;
    Some(SelfVoiceprint {
        human_id: human_id.to_owned(),
        embedding: record.embedding()?,
    })
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

async fn blocking<R: tauri::Runtime, T: Send + 'static>(
    app: &tauri::AppHandle<R>,
    work: impl FnOnce(&dyn EnrollmentSecretStore) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || work(&KeychainStore(&app)))
        .await
        .map_err(|error| error.to_string())?
}

pub(crate) async fn session_owner_id(pool: &sqlx::SqlitePool, session_id: &str) -> Option<String> {
    sqlx::query_scalar::<_, String>(
        "SELECT owner_user_id FROM sessions WHERE id = ? AND deleted_at IS NULL LIMIT 1",
    )
    .bind(session_id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .filter(|id| !id.trim().is_empty())
}

/// The user's enrolled voiceprint, named as the session owner.
pub(crate) async fn self_voiceprint_for_session<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    session_id: &str,
) -> Option<SelfVoiceprint> {
    let pool = app
        .try_state::<tauri_plugin_db::ManagedState>()
        .map(|state| state.pool().clone())?;
    let owner = session_owner_id(&pool, session_id).await?;
    blocking(app, move |store| Ok(load_self_voiceprint(store, &owner)))
        .await
        .ok()
        .flatten()
}

struct ActiveEnrollment {
    cancellation: CancellationToken,
    task: tauri::async_runtime::JoinHandle<Vec<f32>>,
}

#[derive(Default)]
pub struct VoiceEnrollmentState {
    active: Mutex<Option<ActiveEnrollment>>,
}

impl VoiceEnrollmentState {
    fn take(&self) -> Option<ActiveEnrollment> {
        self.active
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
    }
}

fn state<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Arc<VoiceEnrollmentState> {
    app.state::<Arc<VoiceEnrollmentState>>().inner().clone()
}

#[tauri::command]
#[specta::specta]
pub async fn voice_enrollment_status<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
) -> Result<VoiceEnrollmentStatus, String> {
    anlg_aspc::require(
        &aspc_window,
        &aspc_window,
        anlg_aspc::DataType::Recording,
        anlg_aspc::Action::Read,
    )
    .map_err(|denied| denied.to_string())?;
    let record = blocking(&app, load_record).await?;
    Ok(EnrollmentRecord::status(record.as_ref()))
}

#[tauri::command]
#[specta::specta]
pub async fn start_voice_enrollment<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
) -> Result<(), String> {
    anlg_aspc::require(
        &aspc_window,
        &aspc_window,
        anlg_aspc::DataType::Recording,
        anlg_aspc::Action::Create,
    )
    .map_err(|denied| denied.to_string())?;
    let state = state(&app);
    let mut active = state
        .active
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if active.is_some() {
        return Err("Voice enrollment is already recording".to_owned());
    }
    let audio = app
        .state::<Arc<dyn anlg_audio::AudioProvider>>()
        .inner()
        .clone();
    let mut stream = audio
        .open_mic_capture(None, SAMPLE_RATE, CHUNK_SIZE)
        .map_err(|error| error.to_string())?;
    let cancellation = CancellationToken::new();
    let token = cancellation.clone();
    let emitter = app.clone();
    let task = tauri::async_runtime::spawn(async move {
        let mut samples: Vec<f32> = Vec::new();
        let max = MAX_CAPTURE_SECONDS * SAMPLE_RATE as usize;
        loop {
            tokio::select! {
                _ = token.cancelled() => break,
                frame = stream.next() => {
                    let Some(Ok(frame)) = frame else { break };
                    let mic = frame.raw_mic;
                    let rms = (mic.iter().map(|s| s * s).sum::<f32>() / mic.len().max(1) as f32).sqrt();
                    samples.extend_from_slice(&mic);
                    let _ = VoiceEnrollmentLevelEvent {
                        rms: rms.min(1.0),
                        seconds: samples.len() as f32 / SAMPLE_RATE as f32,
                    }
                    .emit(&emitter);
                    if samples.len() >= max {
                        break;
                    }
                }
            }
        }
        samples
    });
    *active = Some(ActiveEnrollment { cancellation, task });
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn finish_voice_enrollment<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<VoiceEnrollmentStatus, String> {
    let active = state(&app)
        .take()
        .ok_or_else(|| "Voice enrollment is not recording".to_owned())?;
    active.cancellation.cancel();
    let samples = active.task.await.map_err(|error| error.to_string())?;
    blocking(&app, move |store| {
        let print = voice_embedding(&samples)?;
        drop(samples);
        if print.speech_seconds < MIN_ENROLLMENT_SPEECH_SECONDS {
            return Err(NOT_ENOUGH_SPEECH.to_owned());
        }
        save_enrollment(store, &print.embedding, print.speech_seconds, now_ms())
    })
    .await
}

pub(crate) fn save_enrollment(
    store: &dyn EnrollmentSecretStore,
    embedding: &[f32],
    speech_seconds: f32,
    now_ms: i64,
) -> Result<VoiceEnrollmentStatus, String> {
    let record = EnrollmentRecord::new(embedding, speech_seconds, now_ms);
    let raw = serde_json::to_string(&record).map_err(|error| error.to_string())?;
    store.write(&raw)?;
    Ok(EnrollmentRecord::status(Some(&record)))
}

#[tauri::command]
#[specta::specta]
pub async fn cancel_voice_enrollment<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<(), String> {
    if let Some(active) = state(&app).take() {
        active.cancellation.cancel();
        let _ = active.task.await;
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn delete_voice_enrollment<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    window: tauri::Window<R>,
) -> Result<VoiceEnrollmentStatus, String> {
    anlg_aspc::require(
        &app,
        &window,
        anlg_aspc::DataType::Recording,
        anlg_aspc::Action::Delete,
    )?;
    cancel_voice_enrollment(app.clone()).await?;
    blocking(&app, |store| {
        store.delete()?;
        Ok(EnrollmentRecord::status(None))
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn improve_voice_enrollment_from_segment<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    session_id: String,
    channel: i32,
    start_ms: i64,
    end_ms: i64,
) -> Result<VoiceEnrollmentStatus, String> {
    let _guard = app.fs_sync().lock_session_audio(&session_id).await;
    let path = app
        .fs_sync()
        .session_audio_path(&session_id)?
        .ok_or_else(|| "This meeting's audio is no longer available".to_owned())?;
    blocking(&app, move |store| {
        let record = load_record(store)?
            .ok_or_else(|| "Set up your voice before improving it".to_owned())?;
        let stored = record
            .embedding()
            .ok_or_else(|| "Stored voice profile is unreadable".to_owned())?;
        let span = embed_audio_span(&path, channel, start_ms, end_ms)
            .map_err(|_| NOT_ENOUGH_SPEECH.to_owned())?;
        if span.speech_seconds < MIN_SPAN_SPEECH_SECONDS {
            return Err(NOT_ENOUGH_SPEECH.to_owned());
        }
        improve_record(
            store,
            record,
            &stored,
            &span.embedding,
            span.speech_seconds,
            now_ms(),
        )
    })
    .await
}

pub(crate) fn improve_record(
    store: &dyn EnrollmentSecretStore,
    record: EnrollmentRecord,
    stored: &[f32],
    new: &[f32],
    new_seconds: f32,
    now_ms: i64,
) -> Result<VoiceEnrollmentStatus, String> {
    let blended = blend_voiceprints(stored, record.speech_seconds, new, new_seconds)
        .ok_or_else(|| "Voice sample does not match the stored profile format".to_owned())?;
    let next = EnrollmentRecord {
        embedding: crate::voiceprint::encode_embedding(&blended),
        updated_at_ms: now_ms,
        speech_seconds: record.speech_seconds + new_seconds,
        ..record
    };
    let raw = serde_json::to_string(&next).map_err(|error| error.to_string())?;
    store.write(&raw)?;
    Ok(EnrollmentRecord::status(Some(&next)))
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    #[derive(Default)]
    struct MemoryStore(RefCell<Option<String>>);

    impl EnrollmentSecretStore for MemoryStore {
        fn read(&self) -> Result<Option<String>, String> {
            Ok(self.0.borrow().clone())
        }
        fn write(&self, value: &str) -> Result<(), String> {
            *self.0.borrow_mut() = Some(value.to_owned());
            Ok(())
        }
        fn delete(&self) -> Result<(), String> {
            *self.0.borrow_mut() = None;
            Ok(())
        }
    }

    fn unit(index: usize) -> Vec<f32> {
        let mut v = vec![0.0; 256];
        v[index] = 1.0;
        v
    }

    #[test]
    fn enrollment_round_trips_and_delete_removes_it() {
        let store = MemoryStore::default();
        assert!(!EnrollmentRecord::status(load_record(&store).unwrap().as_ref()).enrolled);
        assert!(load_self_voiceprint(&store, "me").is_none());

        let status = save_enrollment(&store, &unit(0), 18.5, 1_000).unwrap();
        assert!(status.enrolled);
        assert_eq!(status.created_at_ms, Some(1_000));
        assert_eq!(status.speech_seconds, Some(18.5));
        let print = load_self_voiceprint(&store, "me").unwrap();
        assert_eq!(print.human_id, "me");
        assert_eq!(print.embedding, unit(0));
        // The stored secret never contains raw floats in JSON, only base64.
        assert!(!store.0.borrow().as_ref().unwrap().contains("[0.0"));

        store.delete().unwrap();
        let status = EnrollmentRecord::status(load_record(&store).unwrap().as_ref());
        assert!(!status.enrolled);
        assert_eq!(status.created_at_ms, None);
        // A later in-person pass gets no voiceprint, so it can name nobody.
        assert!(load_self_voiceprint(&store, "me").is_none());
    }

    #[test]
    fn improving_blends_and_keeps_created_at() {
        let store = MemoryStore::default();
        save_enrollment(&store, &unit(0), 20.0, 1_000).unwrap();
        let record = load_record(&store).unwrap().unwrap();
        let stored = record.embedding().unwrap();
        let status = improve_record(&store, record, &stored, &unit(1), 20.0, 2_000).unwrap();
        assert_eq!(status.created_at_ms, Some(1_000));
        assert_eq!(status.updated_at_ms, Some(2_000));
        assert_eq!(status.speech_seconds, Some(40.0));
        let blended = load_record(&store).unwrap().unwrap().embedding().unwrap();
        assert!((blended[0] - blended[1]).abs() < 1e-5 && blended[0] > 0.5);
    }

    #[test]
    fn unnamed_owner_or_foreign_model_yields_no_voiceprint() {
        let store = MemoryStore::default();
        save_enrollment(&store, &unit(0), 20.0, 1_000).unwrap();
        assert!(load_self_voiceprint(&store, "").is_none());
        let mut record = load_record(&store).unwrap().unwrap();
        record.model_version = "other".into();
        store
            .write(&serde_json::to_string(&record).unwrap())
            .unwrap();
        assert!(load_self_voiceprint(&store, "me").is_none());
    }

    #[test]
    fn enrollment_scope_is_native_only_and_outside_synced_tables() {
        // Not a database table, so it can never be an E2EE / sync domain.
        assert!(
            !anlg_db_app::E2EE_DOMAIN_TABLES
                .iter()
                .any(|table| table.contains("voice_enrollment"))
        );
        assert_eq!(VOICE_ENROLLMENT_SCOPE, "voice_enrollment");
    }
}
