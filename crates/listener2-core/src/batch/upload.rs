use std::path::{Path, PathBuf};
use std::time::Duration;

use anlg_audio_utils::Source;
use owhisper_client::BatchUploadLimit;

pub(in crate::batch) fn is_recovery_directory(path: &std::path::Path) -> bool {
    path.file_name()
        .is_some_and(|name| name == "audio-recovery")
}

pub(in crate::batch) fn temporary_audio_directory(
    source: &str,
) -> std::io::Result<tempfile::TempDir> {
    let parent = std::path::Path::new(source).parent();
    if let Some(parent) = parent
        && is_recovery_directory(parent)
    {
        // Derived audio must share the capture's deletion boundary, including
        // cancellation while a blocking encoder is still finishing.
        tempfile::tempdir_in(parent)
    } else {
        tempfile::tempdir()
    }
}

pub(super) fn audio_duration(file_path: &str) -> Option<Duration> {
    anlg_audio_utils::source_from_path(file_path)
        .ok()
        .and_then(|source| source.total_duration())
}

/// Recordings past a provider's upload cap or per-request duration cap are
/// re-encoded as mono upload segments (M4A on macOS, FLAC elsewhere) and
/// sent one request at a time.
pub(super) fn segment_plan(
    file_path: &str,
    audio_duration: Option<Duration>,
    limit: Option<BatchUploadLimit>,
) -> Option<Duration> {
    let limit = limit?;
    let size = std::fs::metadata(file_path).ok()?.len();
    let too_long = audio_duration.is_some_and(|duration| duration > limit.max_duration);

    (size > limit.max_bytes || too_long).then_some(limit.max_duration)
}

pub(super) struct SegmentedUpload {
    _temp_dir: tempfile::TempDir,
    paths: Vec<PathBuf>,
}

impl SegmentedUpload {
    pub(super) fn paths(&self) -> &[PathBuf] {
        &self.paths
    }
}

pub(super) async fn split_batch_upload(
    file_path: &str,
    segment_duration: Duration,
    provider: &str,
) -> crate::Result<SegmentedUpload> {
    let failure = |message: &str| crate::BatchFailure::DirectRequestFailed {
        provider: provider.to_string(),
        message: message.to_string(),
    };

    let temp_dir = temporary_audio_directory(file_path).map_err(|_error| {
        tracing::error!(
            error.type = "temp_dir_create_failed",
            "batch_audio_segment_temp_dir_failed"
        );
        failure("Anarlog couldn't prepare this recording for transcription.")
    })?;

    let source = PathBuf::from(file_path);
    let output_dir = temp_dir.path().to_path_buf();
    let paths = tokio::task::spawn_blocking(move || {
        anlg_audio_encode::encode_mono_segments(&source, &output_dir, segment_duration)
    })
    .await
    .map_err(|_error| {
        tracing::error!(
            error.type = "local_task_join_failed",
            "batch_audio_segment_task_failed"
        );
        failure("Anarlog couldn't prepare this recording for transcription.")
    })?
    .map_err(|_error| {
        tracing::error!(
            error.type = "audio_encode_failed",
            "batch_audio_segment_failed"
        );
        failure("Anarlog couldn't split this recording for transcription.")
    })?;

    if paths.is_empty() {
        return Err(failure("This recording has no audio to transcribe.").into());
    }

    tracing::info!(
        segments = paths.len(),
        segment_seconds = segment_duration.as_secs(),
        "batch audio split for provider upload limits"
    );

    Ok(SegmentedUpload {
        _temp_dir: temp_dir,
        paths,
    })
}

/// A provider-ready copy of a recording, deleted with its directory.
pub(super) struct ProviderCopy {
    _temp_dir: tempfile::TempDir,
    path: PathBuf,
}

impl ProviderCopy {
    pub(super) fn path(&self) -> &Path {
        &self.path
    }
}

/// macOS recordings are AAC streams (`.aac`). Some providers (OpenAI and Groq
/// Whisper among them) accept M4A but not raw AAC, so an `.aac` file is
/// re-encoded as an upload file (`.m4a` on macOS, `.flac` elsewhere), same
/// channels and rate, before any upload. Other files go up as they are.
pub(super) async fn provider_compatible_copy(
    file_path: &str,
    provider: &str,
) -> crate::Result<Option<ProviderCopy>> {
    let is_aac_stream = Path::new(file_path)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("aac"));
    if !is_aac_stream {
        return Ok(None);
    }
    let failure = || crate::BatchFailure::DirectRequestFailed {
        provider: provider.to_string(),
        message: "Anarlog couldn't prepare this recording for transcription.".to_string(),
    };
    let temp_dir = temporary_audio_directory(file_path).map_err(|_error| {
        tracing::error!(
            error.type = "temp_dir_create_failed",
            "batch_audio_upload_copy_temp_dir_failed"
        );
        failure()
    })?;
    let path = temp_dir
        .path()
        .join(format!("audio.{}", anlg_audio_encode::UPLOAD_EXTENSION));
    let source = PathBuf::from(file_path);
    let target = path.clone();
    tokio::task::spawn_blocking(move || anlg_audio_encode::transcode_for_upload(&source, &target))
        .await
        .map_err(|_error| {
            tracing::error!(
                error.type = "local_task_join_failed",
                "batch_audio_upload_copy_task_failed"
            );
            failure()
        })?
        .map_err(|_error| {
            tracing::error!(error.type = "audio_encode_failed", "batch_audio_upload_copy_failed");
            failure()
        })?;
    Ok(Some(ProviderCopy {
        _temp_dir: temp_dir,
        path,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_uploads_stay_inside_the_capture_deletion_boundary() {
        let root = tempfile::tempdir().unwrap();
        let recovery = root.path().join("audio-recovery");
        std::fs::create_dir(&recovery).unwrap();
        let source = recovery.join("chunk.mp3");
        let temporary = temporary_audio_directory(source.to_str().unwrap()).unwrap();
        assert!(temporary.path().starts_with(&recovery));
        std::fs::write(temporary.path().join("derived.wav"), b"private audio").unwrap();
        std::fs::remove_dir_all(&recovery).unwrap();
        assert!(!temporary.path().exists());
        assert!(temporary_audio_directory(source.to_str().unwrap()).is_err());
    }

    #[tokio::test]
    async fn aac_recordings_are_re_encoded_for_upload_and_others_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let aac = dir.path().join("audio.aac");
        let mut encoder = anlg_audio_encode::StereoStreamEncoder::new(16_000).unwrap();
        let mut bytes = Vec::new();
        let tone: Vec<f32> = (0..16_000).map(|i| (i as f32 * 0.05).sin() * 0.3).collect();
        encoder.encode_f32(&tone, &tone, &mut bytes).unwrap();
        encoder.flush(&mut bytes).unwrap();
        std::fs::write(&aac, &bytes).unwrap();

        let copy = provider_compatible_copy(aac.to_str().unwrap(), "test")
            .await
            .unwrap()
            .expect("an AAC stream is converted");
        assert_eq!(
            copy.path().extension().unwrap(),
            anlg_audio_encode::UPLOAD_EXTENSION
        );
        let converted = copy.path().to_path_buf();
        drop(copy);
        assert!(!converted.exists(), "the upload copy is deleted after use");
        assert!(aac.exists(), "the recording itself is untouched");

        let wav = dir.path().join("audio.wav");
        std::fs::write(&wav, b"RIFF").unwrap();
        assert!(
            provider_compatible_copy(wav.to_str().unwrap(), "test")
                .await
                .unwrap()
                .is_none()
        );
    }
}
