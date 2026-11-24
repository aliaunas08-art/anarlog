use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("unsupported channel count: {0} (expected 1 or 2)")]
    UnsupportedChannelCount(u16),

    #[error("unsupported float bit depth: {0}")]
    UnsupportedFloatBitDepth(u16),

    #[error("unsupported integer bit depth: {0}")]
    UnsupportedIntBitDepth(u16),

    #[error("invalid segment duration: {0:?}")]
    InvalidSegmentDuration(std::time::Duration),

    #[error("AudioToolbox {what} failed (OSStatus {status})")]
    AudioToolbox { what: &'static str, status: i32 },

    #[error("sample rate {0} Hz isn't supported by AAC")]
    UnsupportedSampleRate(u32),

    #[error("audio file path isn't valid UTF-8")]
    NonUtf8Path,

    #[error("FLAC encode error: {0}")]
    Flac(String),

    #[error(transparent)]
    Wav(#[from] hound::Error),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    AudioUtils(#[from] anlg_audio_utils::Error),
}
